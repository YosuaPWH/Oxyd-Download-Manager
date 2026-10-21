//! Protects request headers (cookies, authorization) at rest.
//!
//! On Windows the data is encrypted with DPAPI for the current user, so the database alone
//! does not reveal session cookies. Elsewhere it is stored as plain JSON.

use crate::error::{Error, Result};

const PREFIX: &str = "dpapi:";

/// Encode `plain` for storage.
pub fn seal(plain: &str) -> Result<String> {
    #[cfg(windows)]
    {
        Ok(format!(
            "{PREFIX}{}",
            hex::encode(dpapi::protect(plain.as_bytes())?)
        ))
    }
    #[cfg(not(windows))]
    {
        Ok(plain.to_string())
    }
}

/// Decode a value written by [`seal`] (plain values from older versions pass through).
pub fn open(stored: &str) -> Result<String> {
    let Some(hexed) = stored.strip_prefix(PREFIX) else {
        return Ok(stored.to_string());
    };
    let bytes = hex::decode(hexed).map_err(|e| Error::InvalidState(e.to_string()))?;
    #[cfg(windows)]
    {
        let plain = dpapi::unprotect(&bytes)?;
        String::from_utf8(plain).map_err(|e| Error::InvalidState(e.to_string()))
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err(Error::InvalidState(
            "DPAPI data on a non-Windows system".into(),
        ))
    }
}

#[cfg(windows)]
mod dpapi {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    use crate::error::Result;

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    /// Copy the output blob and release it.
    ///
    /// SAFETY: `out` must have been filled by CryptProtectData/CryptUnprotectData.
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        v
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // SAFETY: input points to `data` for the duration of the call; out is released in take.
        let ok = unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { take(out) })
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // SAFETY: as in `protect`.
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { take(out) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_hides_plain_text() {
        let sealed = seal(r#"{"cookie":"session=secret"}"#).unwrap();
        if cfg!(windows) {
            assert!(sealed.starts_with(PREFIX));
            assert!(!sealed.contains("secret"));
        }
        assert_eq!(open(&sealed).unwrap(), r#"{"cookie":"session=secret"}"#);
        assert_eq!(open("{}").unwrap(), "{}");
    }
}
