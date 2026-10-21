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
        if let Some(size) = size {
            if fresh || file.metadata()?.len() != size {
                file.set_len(size)?;
            }
        }
        Ok(SharedFile(Arc::new(file)))
    }

    /// Write `buf` at `offset` on a blocking thread.
    pub async fn write_at(&self, offset: u64, buf: Vec<u8>) -> io::Result<()> {
        let file = self.0.clone();
        tokio::task::spawn_blocking(move || write_all_at(&file, &buf, offset))
            .await
            .map_err(io::Error::other)?
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

#[cfg(not(windows))]
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

    #[test]
    fn reports_free_space() {
        if cfg!(windows) {
            assert!(available_space(Path::new(".")).unwrap() > 0);
        }
    }
}
