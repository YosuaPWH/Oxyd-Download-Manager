//! Optional checksum verification after a download completes.

use std::io::Read;
use std::path::Path;

use md5::Md5;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Verify `path` against `spec` (`sha256:<hex>`, `md5:<hex>` or a bare hex digest whose
/// length decides the algorithm).
pub fn verify(path: &Path, spec: &str) -> Result<()> {
    let spec = spec.trim();
    let (algo, expected) = match spec.split_once(':') {
        Some((a, h)) => (a.to_ascii_lowercase(), h.trim().to_ascii_lowercase()),
        None if spec.len() == 32 => ("md5".into(), spec.to_ascii_lowercase()),
        None => ("sha256".into(), spec.to_ascii_lowercase()),
    };
    let actual = match algo.as_str() {
        "md5" => digest::<Md5>(path)?,
        "sha256" | "sha-256" => digest::<Sha256>(path)?,
        other => {
            return Err(Error::InvalidState(format!(
                "unknown checksum algorithm {other}"
            )))
        }
    };
    if actual == expected {
        Ok(())
    } else {
        Err(Error::Checksum { expected, actual })
    }
}

fn digest<D: Digest>(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = D::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifies_known_digests() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("abc");
        std::fs::write(&p, b"abc").unwrap();
        verify(&p, "md5:900150983cd24fb0d6963f7d28e17f72").unwrap();
        verify(
            &p,
            "SHA256:BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD",
        )
        .unwrap();
        verify(&p, "900150983cd24fb0d6963f7d28e17f72").unwrap();
        assert!(matches!(
            verify(&p, "md5:00000000000000000000000000000000"),
            Err(Error::Checksum { .. })
        ));
    }
}
