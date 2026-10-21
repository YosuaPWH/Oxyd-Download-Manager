//! Positional writes into a pre-allocated file shared by all segment workers.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SharedFile(Arc<File>);

impl SharedFile {
    /// Open (or create) the part file. When `fresh` the file is truncated and, if the size
    /// is known, pre-allocated to its final length.
    pub fn open(path: &Path, size: Option<u64>, fresh: bool) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(fresh)
            .open(path)?;
        if fresh {
            // Segments write far into the file right away. On a normal NTFS file every such
            // write first zero-fills everything before it (valid data length), stalling
            // for seconds on big files and writing the disk twice. Sparse files skip that.
            set_sparse(&file, true);
        }
        if let Some(size) = size {
            if fresh || file.metadata()?.len() != size {
                file.set_len(size)?;
            }
        }
        Ok(SharedFile(Arc::new(file)))
    }

    /// Write `buf` at `offset` on a blocking thread.
    pub async fn write_at(&self, offset: u64, buf: Vec<u8>) -> io::Result<()> {
        self.spawn_write(offset, buf)
            .await
            .map_err(io::Error::other)?
    }

    /// Start writing `buf` at `offset` on a blocking thread without waiting for it.
    pub fn spawn_write(
        &self,
        offset: u64,
        buf: Vec<u8>,
    ) -> tokio::task::JoinHandle<io::Result<()>> {
        let file = self.0.clone();
        tokio::task::spawn_blocking(move || write_all_at(&file, &buf, offset))
    }

    /// Flush file data to disk so recorded progress never exceeds what is persisted.
    pub async fn sync(&self) -> io::Result<()> {
        let file = self.0.clone();
        tokio::task::spawn_blocking(move || file.sync_data())
            .await
            .map_err(io::Error::other)?
    }

    pub fn set_len(&self, len: u64) -> io::Result<()> {
        self.0.set_len(len)
    }
}

/// Mark `file` sparse (or back to normal once fully written). Best effort: file systems
/// without sparse support (FAT32, exFAT) just keep the old behaviour.
#[cfg(windows)]
pub fn set_sparse(file: &File, sparse: bool) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Ioctl::{FILE_SET_SPARSE_BUFFER, FSCTL_SET_SPARSE};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    let input = FILE_SET_SPARSE_BUFFER {
        SetSparse: u8::from(sparse),
    };
    let mut returned = 0u32;
    // SAFETY: valid file handle and a correctly sized input buffer for FSCTL_SET_SPARSE.
    unsafe {
        DeviceIoControl(
            file.as_raw_handle() as _,
            FSCTL_SET_SPARSE,
            &input as *const _ as *const _,
            std::mem::size_of::<FILE_SET_SPARSE_BUFFER>() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        );
    }
}

#[cfg(not(windows))]
pub fn set_sparse(_file: &File, _sparse: bool) {}

/// Turn a completed part file back into a regular file (no sparse attribute).
pub fn finalize(path: &Path) {
    if let Ok(file) = OpenOptions::new().write(true).open(path) {
        set_sparse(&file, false);
    }
}

#[cfg(windows)]
fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        let n = file.seek_write(buf, offset)?;
        if n == 0 {
            return Err(io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        offset += n as u64;
    }
    Ok(())
}

#[cfg(unix)]
fn write_all_at(file: &File, buf: &[u8], offset: u64) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(buf, offset)
}

/// Free space available to the current user on the volume containing `dir`.
#[cfg(windows)]
pub fn available_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let existing = dir.ancestors().find(|p| p.exists())?;
    let wide: Vec<u16> = existing
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free = 0u64;
    // SAFETY: `wide` is NUL terminated and outlives the call; null out pointers are allowed.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free)
}

#[cfg(unix)]
pub fn available_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let existing = dir.ancestors().find(|p| p.exists())?;
    let path = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    // SAFETY: `path` is NUL terminated and `st` is a plain C struct filled by statvfs.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statvfs(path.as_ptr(), &mut st) };
    // Blocks available to unprivileged users, like GetDiskFreeSpaceEx on Windows.
    #[allow(clippy::unnecessary_cast)]
    (rc == 0).then(|| st.f_bavail as u64 * st.f_frsize as u64)
}

#[cfg(not(any(windows, unix)))]
pub fn available_space(_dir: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_at_offsets() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.part");
        let f = SharedFile::open(&p, Some(10), true).unwrap();
        f.write_at(5, b"world".to_vec()).await.unwrap();
        f.write_at(0, b"hello".to_vec()).await.unwrap();
        f.sync().await.unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"helloworld");
    }

    #[cfg(windows)]
    #[test]
    fn part_files_are_sparse_until_finalized() {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x200;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.part");
        let f = SharedFile::open(&p, Some(1 << 30), true).unwrap();
        let attrs = std::fs::metadata(&p).unwrap().file_attributes();
        assert_ne!(
            attrs & FILE_ATTRIBUTE_SPARSE_FILE,
            0,
            "part file not sparse"
        );
        drop(f);
        // Fully written files lose the flag (here: a small file written completely).
        let small = dir.path().join("small.part");
        let f = SharedFile::open(&small, Some(4), true).unwrap();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(f.write_at(0, b"data".to_vec()))
            .unwrap();
        drop(f);
        finalize(&small);
        let attrs = std::fs::metadata(&small).unwrap().file_attributes();
        assert_eq!(attrs & FILE_ATTRIBUTE_SPARSE_FILE, 0);
        assert_eq!(std::fs::read(&small).unwrap(), b"data");
    }

    #[test]
    fn reports_free_space() {
        if cfg!(any(windows, unix)) {
            assert!(available_space(Path::new(".")).unwrap() > 0);
            // A folder that does not exist yet is measured on its closest existing parent.
            assert!(available_space(&std::env::temp_dir().join("oxyd-missing/a/b")).is_some());
        }
    }
}
