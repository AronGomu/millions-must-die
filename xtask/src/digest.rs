//! Shared SHA-256 helpers for bootstrap + shader checks.

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};
use thiserror::Error;

/// Digest errors.
#[derive(Debug, Error)]
pub enum DigestError {
    #[error("io error: {0}")]
    Io(String),
    #[error("hash mismatch for {file}: expected {expected}, got {actual}")]
    HashMismatch {
        file: String,
        expected: String,
        actual: String,
    },
}

/// Lowercase hex SHA-256 of raw bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Lowercase hex SHA-256 of file contents.
pub fn sha256_file(path: &Path) -> Result<String, DigestError> {
    let bytes = fs::read(path).map_err(|e| DigestError::Io(format!("{}: {e}", path.display())))?;
    Ok(sha256_hex(&bytes))
}

/// Require file digest equals expected lowercase hex.
pub fn verify_file_sha256(path: &Path, expected: &str) -> Result<(), DigestError> {
    let actual = sha256_file(path)?;
    if actual != expected {
        return Err(DigestError::HashMismatch {
            file: path.display().to_string(),
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

/// True when version req is an exact x.y.z pin (optional leading `=`).
pub fn is_exact_semver_pin(spec: &str) -> bool {
    let s = spec.trim();
    let s = s.strip_prefix('=').unwrap_or(s);
    if s.is_empty()
        || s.bytes()
            .any(|b| matches!(b, b'*' | b'^' | b'~' | b'>' | b'<' | b','))
    {
        return false;
    }
    let mut parts = s.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    is_numeric_ident(major) && is_numeric_ident(minor) && is_numeric_ident(patch)
}

fn is_numeric_ident(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// True when string is a full lowercase SHA-256 hex digest.
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) && s == s.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_pins() {
        assert!(is_exact_semver_pin("0.18.4"));
        assert!(is_exact_semver_pin("=0.18.4"));
        assert!(is_exact_semver_pin("3.4.12"));
        assert!(!is_exact_semver_pin("0.18"));
        assert!(!is_exact_semver_pin("^0.18.4"));
        assert!(!is_exact_semver_pin("0.18.*"));
        assert!(!is_exact_semver_pin("~0.18.4"));
        assert!(!is_exact_semver_pin(">=0.18.4"));
    }

    #[test]
    fn sha_shape() {
        assert!(is_sha256_hex(
            "f07b958a9ac5020fb7a44cadb957f658b2149c3c8abb4f63145fac9303249db7"
        ));
        assert!(!is_sha256_hex("abc"));
        assert!(!is_sha256_hex(
            "F07B958A9AC5020FB7A44CADB957F658B2149C3C8ABB4F63145FAC9303249DB7"
        ));
    }
}
