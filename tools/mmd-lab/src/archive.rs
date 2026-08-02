//! Deterministic SHA-256 content-addressed source archive.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;

/// Magic + version for MMD archive format.
pub const ARCHIVE_MAGIC: &[u8; 8] = b"MMDARC01";

/// Prefix used in content-address names: `sha256-<hex>`.
pub const CONTENT_ID_PREFIX: &str = "sha256-";

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("archive: {0}")]
    Msg(String),
}

/// One file inside the archive (path relative, `/` separators).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub path: String,
    pub data: Vec<u8>,
}

/// Content-addressed archive blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveBlob {
    pub bytes: Vec<u8>,
    pub sha256: String,
}

impl ArchiveBlob {
    pub fn content_id(&self) -> String {
        format!("{CONTENT_ID_PREFIX}{}", self.sha256)
    }
}

/// SHA-256 hex of bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Build deterministic archive from entries (sorts by path).
pub fn build_archive(mut entries: Vec<ArchiveEntry>) -> Result<ArchiveBlob, ArchiveError> {
    for e in &entries {
        if e.path.is_empty() || e.path.starts_with('/') || e.path.contains('\0') {
            return Err(ArchiveError::Msg(format!("invalid path: {:?}", e.path)));
        }
        if e.path.contains('\\') {
            return Err(ArchiveError::Msg(format!(
                "path must use / separators: {:?}",
                e.path
            )));
        }
        if Path::new(&e.path)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(ArchiveError::Msg(format!(
                "path must not contain ..: {:?}",
                e.path
            )));
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    // Reject duplicates after sort.
    for w in entries.windows(2) {
        if w[0].path == w[1].path {
            return Err(ArchiveError::Msg(format!(
                "duplicate path: {}",
                w[0].path
            )));
        }
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(ARCHIVE_MAGIC);
    let count = u32::try_from(entries.len())
        .map_err(|_| ArchiveError::Msg("too many entries".into()))?;
    bytes.extend_from_slice(&count.to_be_bytes());
    for e in &entries {
        let path_b = e.path.as_bytes();
        let path_len = u32::try_from(path_b.len())
            .map_err(|_| ArchiveError::Msg("path too long".into()))?;
        let data_len = u64::try_from(e.data.len())
            .map_err(|_| ArchiveError::Msg("file too large".into()))?;
        bytes.extend_from_slice(&path_len.to_be_bytes());
        bytes.extend_from_slice(path_b);
        bytes.extend_from_slice(&data_len.to_be_bytes());
        bytes.extend_from_slice(&e.data);
    }
    let sha256 = sha256_hex(&bytes);
    Ok(ArchiveBlob { bytes, sha256 })
}

/// Parse archive; return entries in file order.
#[allow(dead_code)] // used by tests + future extract path
pub fn parse_archive(bytes: &[u8]) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    if bytes.len() < 12 || &bytes[0..8] != ARCHIVE_MAGIC {
        return Err(ArchiveError::Msg("bad magic".into()));
    }
    let count = u32::from_be_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let mut off = 12usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if off + 4 > bytes.len() {
            return Err(ArchiveError::Msg("truncated path_len".into()));
        }
        let path_len =
            u32::from_be_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if off + path_len + 8 > bytes.len() {
            return Err(ArchiveError::Msg("truncated path/data_len".into()));
        }
        let path = std::str::from_utf8(&bytes[off..off + path_len])
            .map_err(|_| ArchiveError::Msg("path not utf8".into()))?
            .to_string();
        off += path_len;
        let data_len =
            u64::from_be_bytes(bytes[off..off + 8].try_into().unwrap()) as usize;
        off += 8;
        if off + data_len > bytes.len() {
            return Err(ArchiveError::Msg("truncated data".into()));
        }
        let data = bytes[off..off + data_len].to_vec();
        off += data_len;
        out.push(ArchiveEntry { path, data });
    }
    if off != bytes.len() {
        return Err(ArchiveError::Msg("trailing bytes".into()));
    }
    Ok(out)
}

/// Verify blob hash matches expected hex (case-insensitive).
#[allow(dead_code)] // public coordinator API
pub fn verify_archive_hash(blob: &ArchiveBlob, expected_sha256: &str) -> Result<(), ArchiveError> {
    if !blob.sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(ArchiveError::Msg(format!(
            "archive hash mismatch: got {} expected {expected_sha256}",
            blob.sha256
        )));
    }
    // Recompute from bytes to catch struct tampering.
    let recomputed = sha256_hex(&blob.bytes);
    if !recomputed.eq_ignore_ascii_case(expected_sha256) {
        return Err(ArchiveError::Msg(format!(
            "archive bytes hash mismatch: got {recomputed} expected {expected_sha256}"
        )));
    }
    Ok(())
}

/// Verify raw bytes against expected digest; return blob.
pub fn verify_bytes_hash(bytes: &[u8], expected_sha256: &str) -> Result<ArchiveBlob, ArchiveError> {
    let sha256 = sha256_hex(bytes);
    if !sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(ArchiveError::Msg(format!(
            "archive hash mismatch: got {sha256} expected {expected_sha256}"
        )));
    }
    Ok(ArchiveBlob {
        bytes: bytes.to_vec(),
        sha256,
    })
}

/// Pack a small directory tree into a deterministic archive.
///
/// Skips `target`, `.git`, and other bulky/ephemeral dirs by default.
pub fn pack_tree(root: &Path) -> Result<ArchiveBlob, ArchiveError> {
    let root = fs::canonicalize(root)?;
    let mut entries = Vec::new();
    collect_entries(&root, &root, &mut entries)?;
    build_archive(entries)
}

fn collect_entries(
    root: &Path,
    dir: &Path,
    out: &mut Vec<ArchiveEntry>,
) -> Result<(), ArchiveError> {
    let mut children: Vec<PathBuf> = fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;
    children.sort();
    for path in children {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| ArchiveError::Msg(format!("non-utf8 name under {}", dir.display())))?;
        if should_skip_name(name) {
            continue;
        }
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            // Skip symlinks for determinism/safety.
            continue;
        }
        if meta.is_dir() {
            collect_entries(root, &path, out)?;
        } else if meta.is_file() {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| ArchiveError::Msg("strip_prefix failed".into()))?;
            let rel_s = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            let data = fs::read(&path)?;
            out.push(ArchiveEntry {
                path: rel_s,
                data,
            });
        }
    }
    Ok(())
}

fn should_skip_name(name: &str) -> bool {
    matches!(
        name,
        "target"
            | ".git"
            | ".agents"
            | ".claude"
            | ".pi-subagents"
            | ".agentsystem"
            | "node_modules"
            | ".tmp"
            | ".direnv"
    )
}

/// Write blob to `dir/{content_id}.mmdarc`.
pub fn write_archive_file(dir: &Path, blob: &ArchiveBlob) -> Result<PathBuf, ArchiveError> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.mmdarc", blob.content_id()));
    fs::write(&path, &blob.bytes)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn deterministic_hash_same_entries() {
        let e = vec![
            ArchiveEntry {
                path: "b.txt".into(),
                data: b"b".to_vec(),
            },
            ArchiveEntry {
                path: "a.txt".into(),
                data: b"a".to_vec(),
            },
        ];
        let a = build_archive(e.clone()).unwrap();
        let b = build_archive(e).unwrap();
        assert_eq!(a.sha256, b.sha256);
        assert_eq!(a.bytes, b.bytes);
        let parsed = parse_archive(&a.bytes).unwrap();
        assert_eq!(parsed[0].path, "a.txt");
        assert_eq!(parsed[1].path, "b.txt");
    }

    #[test]
    fn hash_mismatch_detected() {
        let blob = build_archive(vec![ArchiveEntry {
            path: "x".into(),
            data: b"1".to_vec(),
        }])
        .unwrap();
        let err = verify_archive_hash(&blob, &("0".repeat(64))).unwrap_err();
        assert!(err.to_string().contains("hash mismatch"));
    }

    #[test]
    fn pack_tree_roundtrip() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("hello.txt"), b"hi").unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/x.txt"), b"x").unwrap();
        fs::create_dir_all(dir.path().join("target")).unwrap();
        fs::write(dir.path().join("target/skip"), b"no").unwrap();
        let blob = pack_tree(dir.path()).unwrap();
        let entries = parse_archive(&blob.bytes).unwrap();
        let paths: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"hello.txt"));
        assert!(paths.contains(&"sub/x.txt"));
        assert!(!paths.iter().any(|p| p.contains("target")));
    }
}
