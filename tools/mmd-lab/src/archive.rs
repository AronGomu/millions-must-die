//! Deterministic SHA-256 content-addressed source archive.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;

/// Magic + version for MMD archive format.
pub const ARCHIVE_MAGIC: &[u8; 8] = b"MMDARC01";

/// Prefix used in content-address names: `sha256-<hex>`.
pub const CONTENT_ID_PREFIX: &str = "sha256-";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveLimits {
    pub max_depth: usize,
    pub max_visited_entries: usize,
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_decoded_bytes: u64,
    pub max_encoded_bytes: u64,
}

pub const PRODUCTION_ARCHIVE_LIMITS: ArchiveLimits = ArchiveLimits {
    max_depth: 32,
    max_visited_entries: 16_384,
    max_files: 8_192,
    max_file_bytes: 16 * 1024 * 1024,
    max_decoded_bytes: 64 * 1024 * 1024,
    max_encoded_bytes: 80 * 1024 * 1024,
};

#[derive(Debug, Error)]
pub enum ArchiveError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("archive: {0}")]
    Msg(String),
    #[error("archive limit exceeded: max_depth actual={actual} limit={limit} path={path}")]
    DepthLimitExceeded {
        actual: usize,
        limit: usize,
        path: String,
    },
    #[error("archive limit exceeded: max_visited_entries actual={actual} limit={limit}")]
    VisitedEntriesLimitExceeded { actual: usize, limit: usize },
    #[error("archive limit exceeded: max_files actual={actual} limit={limit}")]
    FileCountLimitExceeded { actual: usize, limit: usize },
    #[error("archive limit exceeded: max_file_bytes actual={actual} limit={limit} path={path}")]
    FileSizeLimitExceeded {
        actual: u64,
        limit: u64,
        path: String,
    },
    #[error("archive limit exceeded: max_decoded_bytes actual={actual} limit={limit} path={path}")]
    DecodedSizeLimitExceeded {
        actual: u64,
        limit: u64,
        path: String,
    },
    #[error("archive limit exceeded: max_encoded_bytes actual={actual} limit={limit}")]
    EncodedSizeLimitExceeded { actual: u64, limit: u64 },
    #[error("archive size overflow: {context}")]
    SizeOverflow { context: &'static str },
    #[error("archive allocation failed: {context} requested={requested}")]
    AllocationFailed {
        context: &'static str,
        requested: usize,
    },
}

fn checked_encoded_add(total: u64, amount: u64) -> Result<u64, ArchiveError> {
    total.checked_add(amount).ok_or(ArchiveError::SizeOverflow {
        context: "archive encoded length",
    })
}

fn reserve_additional<T>(
    values: &mut Vec<T>,
    additional: usize,
    context: &'static str,
) -> Result<(), ArchiveError> {
    values
        .try_reserve(additional)
        .map_err(|_| ArchiveError::AllocationFailed {
            context,
            requested: additional,
        })
}

fn reserve_exact<T>(
    values: &mut Vec<T>,
    additional: usize,
    context: &'static str,
) -> Result<(), ArchiveError> {
    values
        .try_reserve_exact(additional)
        .map_err(|_| ArchiveError::AllocationFailed {
            context,
            requested: additional,
        })
}

fn copy_error_path(path: &str) -> Result<String, ArchiveError> {
    let mut copy = String::new();
    copy.try_reserve_exact(path.len())
        .map_err(|_| ArchiveError::AllocationFailed {
            context: "archive error path",
            requested: path.len(),
        })?;
    copy.push_str(path);
    Ok(copy)
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
#[allow(dead_code)] // public coordinator API + tests
pub fn build_archive(entries: Vec<ArchiveEntry>) -> Result<ArchiveBlob, ArchiveError> {
    build_archive_with_limits(entries, PRODUCTION_ARCHIVE_LIMITS)
}

fn build_archive_with_limits(
    mut entries: Vec<ArchiveEntry>,
    limits: ArchiveLimits,
) -> Result<ArchiveBlob, ArchiveError> {
    let attempted_count = entries.len();
    if attempted_count > limits.max_files {
        return Err(ArchiveError::FileCountLimitExceeded {
            actual: attempted_count,
            limit: limits.max_files,
        });
    }
    let count =
        u32::try_from(attempted_count).map_err(|_| ArchiveError::Msg("too many entries".into()))?;

    let mut decoded_len = 0u64;
    let mut encoded_len = 12u64;
    if encoded_len > limits.max_encoded_bytes {
        return Err(ArchiveError::EncodedSizeLimitExceeded {
            actual: encoded_len,
            limit: limits.max_encoded_bytes,
        });
    }
    for entry in &entries {
        validate_archive_path(&entry.path)?;
        let path_len = u32::try_from(entry.path.len())
            .map_err(|_| ArchiveError::Msg("path too long".into()))?;
        let data_len = u64::try_from(entry.data.len())
            .map_err(|_| ArchiveError::Msg("file too large".into()))?;
        if data_len > limits.max_file_bytes {
            return Err(ArchiveError::FileSizeLimitExceeded {
                actual: data_len,
                limit: limits.max_file_bytes,
                path: copy_error_path(&entry.path)?,
            });
        }

        decoded_len = decoded_len
            .checked_add(data_len)
            .ok_or(ArchiveError::SizeOverflow {
                context: "archive decoded length",
            })?;
        if decoded_len > limits.max_decoded_bytes {
            return Err(ArchiveError::DecodedSizeLimitExceeded {
                actual: decoded_len,
                limit: limits.max_decoded_bytes,
                path: copy_error_path(&entry.path)?,
            });
        }

        encoded_len = checked_encoded_add(encoded_len, 4)?;
        encoded_len = checked_encoded_add(encoded_len, u64::from(path_len))?;
        encoded_len = checked_encoded_add(encoded_len, 8)?;
        encoded_len = checked_encoded_add(encoded_len, data_len)?;
        if encoded_len > limits.max_encoded_bytes {
            return Err(ArchiveError::EncodedSizeLimitExceeded {
                actual: encoded_len,
                limit: limits.max_encoded_bytes,
            });
        }
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    for pair in entries.windows(2) {
        if pair[0].path == pair[1].path {
            return Err(ArchiveError::Msg(format!(
                "duplicate path: {}",
                pair[0].path
            )));
        }
    }

    let final_len = usize::try_from(encoded_len).map_err(|_| ArchiveError::SizeOverflow {
        context: "archive encoded length",
    })?;
    let mut bytes = Vec::new();
    reserve_exact(&mut bytes, final_len, "archive bytes")?;
    bytes.extend_from_slice(ARCHIVE_MAGIC);
    bytes.extend_from_slice(&count.to_be_bytes());
    for entry in &entries {
        let path_bytes = entry.path.as_bytes();
        let path_len = u32::try_from(path_bytes.len())
            .map_err(|_| ArchiveError::Msg("path too long".into()))?;
        let data_len = u64::try_from(entry.data.len())
            .map_err(|_| ArchiveError::Msg("file too large".into()))?;
        bytes.extend_from_slice(&path_len.to_be_bytes());
        bytes.extend_from_slice(path_bytes);
        bytes.extend_from_slice(&data_len.to_be_bytes());
        bytes.extend_from_slice(&entry.data);
    }
    debug_assert_eq!(bytes.len(), final_len);
    let sha256 = sha256_hex(&bytes);
    Ok(ArchiveBlob { bytes, sha256 })
}

fn validate_archive_path(path: &str) -> Result<(), ArchiveError> {
    if path.is_empty() || path.starts_with('/') || path.contains('\0') {
        return Err(ArchiveError::Msg(format!("invalid path: {path:?}")));
    }
    if path.contains('\\') {
        return Err(ArchiveError::Msg(format!(
            "path must use / separators: {path:?}"
        )));
    }
    if Path::new(path)
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(ArchiveError::Msg(format!(
            "path must not contain ..: {path:?}"
        )));
    }
    Ok(())
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
        let path_len = u32::from_be_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if off + path_len + 8 > bytes.len() {
            return Err(ArchiveError::Msg("truncated path/data_len".into()));
        }
        let path = std::str::from_utf8(&bytes[off..off + path_len])
            .map_err(|_| ArchiveError::Msg("path not utf8".into()))?
            .to_string();
        off += path_len;
        let data_len = u64::from_be_bytes(bytes[off..off + 8].try_into().unwrap()) as usize;
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
    pack_tree_with_limits(root, PRODUCTION_ARCHIVE_LIMITS)
}

fn pack_tree_with_limits(root: &Path, limits: ArchiveLimits) -> Result<ArchiveBlob, ArchiveError> {
    let mut before_file_read = |_: &Path| Ok(());
    pack_tree_with_limits_and_hook(root, limits, &mut before_file_read)
}

fn pack_tree_with_limits_and_hook<F>(
    root: &Path,
    limits: ArchiveLimits,
    before_file_read: &mut F,
) -> Result<ArchiveBlob, ArchiveError>
where
    F: FnMut(&Path) -> io::Result<()>,
{
    let root = fs::canonicalize(root)?;
    let mut entries = Vec::new();
    let mut stack = Vec::new();
    reserve_additional(&mut stack, 1, "directory stack")?;
    stack.push((root.clone(), 0usize));
    let mut visited_entries = 0usize;
    let mut file_count = 0usize;
    let mut decoded_bytes = 0u64;

    while let Some((path, depth)) = stack.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if depth > limits.max_depth {
            return Err(ArchiveError::DepthLimitExceeded {
                actual: depth,
                limit: limits.max_depth,
                path: archive_relative_path(&root, &path)?,
            });
        }
        if metadata.is_dir() {
            let mut children = Vec::new();
            for child in fs::read_dir(&path)? {
                let child = child?;
                visited_entries =
                    visited_entries
                        .checked_add(1)
                        .ok_or(ArchiveError::SizeOverflow {
                            context: "archive visited entries",
                        })?;
                if visited_entries > limits.max_visited_entries {
                    return Err(ArchiveError::VisitedEntriesLimitExceeded {
                        actual: visited_entries,
                        limit: limits.max_visited_entries,
                    });
                }

                let child_path = child.path();
                let name = child_path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| {
                        ArchiveError::Msg(format!("non-utf8 name under {}", path.display()))
                    })?;
                if should_skip_name(name) {
                    continue;
                }
                let child_depth = depth.checked_add(1).ok_or(ArchiveError::SizeOverflow {
                    context: "archive traversal depth",
                })?;
                reserve_additional(&mut children, 1, "directory children")?;
                children.push((child_path, child_depth));
            }
            children.sort_by(|left, right| left.0.cmp(&right.0));
            for child in children.into_iter().rev() {
                reserve_additional(&mut stack, 1, "directory stack")?;
                stack.push(child);
            }
        } else if metadata.is_file() {
            let relative_path = archive_relative_path(&root, &path)?;
            file_count = file_count
                .checked_add(1)
                .ok_or(ArchiveError::SizeOverflow {
                    context: "archive file count",
                })?;
            if file_count > limits.max_files {
                return Err(ArchiveError::FileCountLimitExceeded {
                    actual: file_count,
                    limit: limits.max_files,
                });
            }

            let metadata_len = metadata.len();
            if metadata_len > limits.max_file_bytes {
                return Err(ArchiveError::FileSizeLimitExceeded {
                    actual: metadata_len,
                    limit: limits.max_file_bytes,
                    path: relative_path,
                });
            }
            let remaining_decoded = limits.max_decoded_bytes.checked_sub(decoded_bytes).ok_or(
                ArchiveError::SizeOverflow {
                    context: "archive decoded remaining",
                },
            )?;
            if metadata_len > remaining_decoded {
                let actual =
                    decoded_bytes
                        .checked_add(metadata_len)
                        .ok_or(ArchiveError::SizeOverflow {
                            context: "archive decoded length",
                        })?;
                return Err(ArchiveError::DecodedSizeLimitExceeded {
                    actual,
                    limit: limits.max_decoded_bytes,
                    path: relative_path,
                });
            }

            before_file_read(&path)?;
            let read_cap = limits
                .max_file_bytes
                .min(remaining_decoded)
                .checked_add(1)
                .ok_or(ArchiveError::SizeOverflow {
                    context: "archive read limit",
                })?;
            let file = File::open(&path)?;
            let mut reader = file.take(read_cap);
            let mut data = Vec::new();
            let mut chunk = [0u8; 64 * 1024];
            loop {
                let read = reader.read(&mut chunk)?;
                if read == 0 {
                    break;
                }
                reserve_additional(&mut data, read, "file data")?;
                data.extend_from_slice(&chunk[..read]);
            }
            let observed = u64::try_from(data.len()).map_err(|_| ArchiveError::SizeOverflow {
                context: "archive file length",
            })?;
            if observed > limits.max_file_bytes {
                return Err(ArchiveError::FileSizeLimitExceeded {
                    actual: observed,
                    limit: limits.max_file_bytes,
                    path: relative_path,
                });
            }
            let new_decoded =
                decoded_bytes
                    .checked_add(observed)
                    .ok_or(ArchiveError::SizeOverflow {
                        context: "archive decoded length",
                    })?;
            if new_decoded > limits.max_decoded_bytes {
                return Err(ArchiveError::DecodedSizeLimitExceeded {
                    actual: new_decoded,
                    limit: limits.max_decoded_bytes,
                    path: relative_path,
                });
            }
            decoded_bytes = new_decoded;
            reserve_additional(&mut entries, 1, "archive entries")?;
            entries.push(ArchiveEntry {
                path: relative_path,
                data,
            });
        }
    }

    build_archive_with_limits(entries, limits)
}

fn archive_relative_path(root: &Path, path: &Path) -> Result<String, ArchiveError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ArchiveError::Msg("strip_prefix failed".into()))?;
    let mut normalized = String::new();
    for component in relative.components() {
        let component = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| ArchiveError::Msg("archive path not utf8".into()))?;
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(component);
    }
    Ok(normalized)
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

    fn tiny_limits() -> ArchiveLimits {
        ArchiveLimits {
            max_depth: 8,
            max_visited_entries: 32,
            max_files: 8,
            max_file_bytes: 64,
            max_decoded_bytes: 64,
            max_encoded_bytes: 1_024,
        }
    }

    fn entry(path: &str, data: &[u8]) -> ArchiveEntry {
        ArchiveEntry {
            path: path.into(),
            data: data.to_vec(),
        }
    }

    #[cfg(unix)]
    fn symlink_dir(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[cfg(windows)]
    fn symlink_dir(target: &Path, link: &Path) {
        std::os::windows::fs::symlink_dir(target, link).unwrap();
    }

    #[test]
    fn deterministic_hash_same_entries() {
        let entries = vec![entry("b.txt", b"b"), entry("a.txt", b"a")];
        let a = build_archive(entries.clone()).unwrap();
        let b = build_archive(entries).unwrap();
        assert_eq!(a.sha256, b.sha256);
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(
            hex::encode(&a.bytes),
            "4d4d4441524330310000000200000005612e74787400000000000000016100000005622e747874000000000000000162"
        );
        assert_eq!(
            a.sha256,
            "ff1cee1ba99c0f3f2cbd6749bc4755e2d0b7e59b8f03f47a977a0f2f67e06295"
        );
        let parsed = parse_archive(&a.bytes).unwrap();
        let paths: Vec<_> = parsed.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "b.txt"]);
    }

    #[test]
    fn hash_mismatch_detected() {
        let blob = build_archive(vec![entry("x", b"1")]).unwrap();
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

        let first = pack_tree(dir.path()).unwrap();
        let second = pack_tree(dir.path()).unwrap();
        assert_eq!(first, second);
        let entries = parse_archive(&first.bytes).unwrap();
        let paths: Vec<_> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["hello.txt", "sub/x.txt"]);
        assert_eq!(entries[0].data, b"hi");
        assert_eq!(entries[1].data, b"x");
    }

    #[test]
    fn depth_limit_accepts_limit_and_rejects_deep_file() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        let limits = ArchiveLimits {
            max_depth: 2,
            ..tiny_limits()
        };
        assert!(pack_tree_with_limits(dir.path(), limits).is_ok());

        fs::write(dir.path().join("a/b/c"), b"x").unwrap();
        let err = pack_tree_with_limits(dir.path(), limits).unwrap_err();
        match &err {
            ArchiveError::DepthLimitExceeded {
                actual,
                limit,
                path,
            } => assert_eq!((*actual, *limit, path.as_str()), (3, 2, "a/b/c")),
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_depth actual=3 limit=2 path=a/b/c"
        );
    }

    #[test]
    fn visited_limit_counts_dirs_skips_and_symlinks() {
        let dir = tempdir().unwrap();
        let external = tempdir().unwrap();
        fs::create_dir(dir.path().join("a")).unwrap();
        fs::write(external.path().join("hidden"), b"x").unwrap();
        symlink_dir(external.path(), &dir.path().join("b"));
        let limits = ArchiveLimits {
            max_visited_entries: 2,
            ..tiny_limits()
        };
        let blob = pack_tree_with_limits(dir.path(), limits).unwrap();
        assert!(parse_archive(&blob.bytes).unwrap().is_empty());

        fs::create_dir(dir.path().join("target")).unwrap();
        let err = pack_tree_with_limits(dir.path(), limits).unwrap_err();
        match &err {
            ArchiveError::VisitedEntriesLimitExceeded { actual, limit } => {
                assert_eq!((*actual, *limit), (3, 2));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_visited_entries actual=3 limit=2"
        );
    }

    #[test]
    fn builder_file_count_limit_accepts_limit_and_rejects_limit_plus_one() {
        let limits = ArchiveLimits {
            max_files: 2,
            ..tiny_limits()
        };
        assert!(build_archive_with_limits(vec![entry("a", b""), entry("b", b"")], limits).is_ok());
        let err = build_archive_with_limits(
            vec![entry("a", b""), entry("b", b""), entry("c", b"")],
            limits,
        )
        .unwrap_err();
        match err {
            ArchiveError::FileCountLimitExceeded { actual, limit } => {
                assert_eq!((actual, limit), (3, 2));
                assert_eq!(
                    ArchiveError::FileCountLimitExceeded { actual, limit }.to_string(),
                    "archive limit exceeded: max_files actual=3 limit=2"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn pack_file_count_rejects_before_second_read() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a"), b"a").unwrap();
        fs::write(dir.path().join("b"), b"b").unwrap();
        let limits = ArchiveLimits {
            max_files: 1,
            ..tiny_limits()
        };
        let mut reads = 0;
        let err = pack_tree_with_limits_and_hook(dir.path(), limits, &mut |_| {
            reads += 1;
            Ok(())
        })
        .unwrap_err();
        match &err {
            ArchiveError::FileCountLimitExceeded { actual, limit } => {
                assert_eq!((*actual, *limit), (2, 1));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(reads, 1);
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_files actual=2 limit=1"
        );
    }

    #[test]
    fn builder_per_file_limit_accepts_limit_and_rejects_limit_plus_one() {
        let limits = ArchiveLimits {
            max_file_bytes: 4,
            ..tiny_limits()
        };
        assert!(build_archive_with_limits(vec![entry("x", b"1234")], limits).is_ok());
        let err = build_archive_with_limits(vec![entry("x", b"12345")], limits).unwrap_err();
        match &err {
            ArchiveError::FileSizeLimitExceeded {
                actual,
                limit,
                path,
            } => assert_eq!((*actual, *limit, path.as_str()), (5, 4, "x")),
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_file_bytes actual=5 limit=4 path=x"
        );
    }

    #[test]
    fn pack_per_file_limit_accepts_limit_and_rejects_limit_plus_one() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("x");
        fs::write(&path, b"1234").unwrap();
        let limits = ArchiveLimits {
            max_file_bytes: 4,
            ..tiny_limits()
        };
        assert!(pack_tree_with_limits(dir.path(), limits).is_ok());
        fs::write(&path, b"12345").unwrap();
        let mut reads = 0;
        let err = pack_tree_with_limits_and_hook(dir.path(), limits, &mut |_| {
            reads += 1;
            Ok(())
        })
        .unwrap_err();
        match &err {
            ArchiveError::FileSizeLimitExceeded {
                actual,
                limit,
                path,
            } => assert_eq!((*actual, *limit, path.as_str()), (5, 4, "x")),
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(reads, 0);
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_file_bytes actual=5 limit=4 path=x"
        );
    }

    #[test]
    fn builder_decoded_limit_accepts_limit_and_rejects_limit_plus_one() {
        let limits = ArchiveLimits {
            max_file_bytes: 8,
            max_decoded_bytes: 4,
            ..tiny_limits()
        };
        assert!(
            build_archive_with_limits(vec![entry("a", b"12"), entry("b", b"34")], limits).is_ok()
        );
        let err = build_archive_with_limits(vec![entry("a", b"12"), entry("b", b"345")], limits)
            .unwrap_err();
        match &err {
            ArchiveError::DecodedSizeLimitExceeded {
                actual,
                limit,
                path,
            } => assert_eq!((*actual, *limit, path.as_str()), (5, 4, "b")),
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_decoded_bytes actual=5 limit=4 path=b"
        );
    }

    #[test]
    fn pack_decoded_limit_accepts_limit_and_rejects_limit_plus_one() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a"), b"12").unwrap();
        fs::write(dir.path().join("b"), b"34").unwrap();
        let limits = ArchiveLimits {
            max_file_bytes: 8,
            max_decoded_bytes: 4,
            ..tiny_limits()
        };
        assert!(pack_tree_with_limits(dir.path(), limits).is_ok());
        fs::write(dir.path().join("b"), b"345").unwrap();
        let err = pack_tree_with_limits(dir.path(), limits).unwrap_err();
        match &err {
            ArchiveError::DecodedSizeLimitExceeded {
                actual,
                limit,
                path,
            } => assert_eq!((*actual, *limit, path.as_str()), (5, 4, "b")),
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_decoded_bytes actual=5 limit=4 path=b"
        );
    }

    #[test]
    fn builder_encoded_limit_accepts_limit_and_rejects_limit_plus_one() {
        let empty_at_limit = ArchiveLimits {
            max_encoded_bytes: 12,
            ..tiny_limits()
        };
        assert_eq!(
            build_archive_with_limits(Vec::new(), empty_at_limit)
                .unwrap()
                .bytes
                .len(),
            12
        );
        let empty_below_limit = ArchiveLimits {
            max_encoded_bytes: 11,
            ..tiny_limits()
        };
        let err = build_archive_with_limits(Vec::new(), empty_below_limit).unwrap_err();
        match &err {
            ArchiveError::EncodedSizeLimitExceeded { actual, limit } => {
                assert_eq!((*actual, *limit), (12, 11));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_encoded_bytes actual=12 limit=11"
        );

        let pass = ArchiveLimits {
            max_encoded_bytes: 25,
            ..tiny_limits()
        };
        assert_eq!(
            build_archive_with_limits(vec![entry("a", b"")], pass)
                .unwrap()
                .bytes
                .len(),
            25
        );
        let fail = ArchiveLimits {
            max_encoded_bytes: 24,
            ..tiny_limits()
        };
        let err = build_archive_with_limits(vec![entry("a", b"")], fail).unwrap_err();
        match &err {
            ArchiveError::EncodedSizeLimitExceeded { actual, limit } => {
                assert_eq!((*actual, *limit), (25, 24));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_encoded_bytes actual=25 limit=24"
        );
    }

    #[test]
    fn metadata_overhead_counts_toward_encoded_limit() {
        let limits = ArchiveLimits {
            max_decoded_bytes: 0,
            max_encoded_bytes: 24,
            ..tiny_limits()
        };
        let err = build_archive_with_limits(vec![entry("a", b"")], limits).unwrap_err();
        match &err {
            ArchiveError::EncodedSizeLimitExceeded { actual, limit } => {
                assert_eq!((*actual, *limit), (25, 24));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_encoded_bytes actual=25 limit=24"
        );
    }

    #[test]
    fn growth_after_metadata_obeys_active_budget_plus_one() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("x");
        fs::write(&path, b"1234").unwrap();
        let limits = ArchiveLimits {
            max_file_bytes: 4,
            max_decoded_bytes: 4,
            ..tiny_limits()
        };
        let mut mutations = 0;
        let err = pack_tree_with_limits_and_hook(dir.path(), limits, &mut |selected| {
            if selected == path {
                mutations += 1;
                fs::write(selected, b"12345")?;
            }
            Ok(())
        })
        .unwrap_err();
        match &err {
            ArchiveError::FileSizeLimitExceeded {
                actual,
                limit,
                path,
            } => assert_eq!((*actual, *limit, path.as_str()), (5, 4, "x")),
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(mutations, 1);
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_file_bytes actual=5 limit=4 path=x"
        );

        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a"), b"123").unwrap();
        let path = dir.path().join("b");
        fs::write(&path, b"4").unwrap();
        let limits = ArchiveLimits {
            max_file_bytes: 8,
            max_decoded_bytes: 4,
            ..tiny_limits()
        };
        let mut mutations = 0;
        let err = pack_tree_with_limits_and_hook(dir.path(), limits, &mut |selected| {
            if selected == path {
                mutations += 1;
                fs::write(selected, b"45")?;
            }
            Ok(())
        })
        .unwrap_err();
        match &err {
            ArchiveError::DecodedSizeLimitExceeded {
                actual,
                limit,
                path,
            } => assert_eq!((*actual, *limit, path.as_str()), (5, 4, "b")),
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(mutations, 1);
        assert_eq!(
            err.to_string(),
            "archive limit exceeded: max_decoded_bytes actual=5 limit=4 path=b"
        );
    }

    #[test]
    fn deterministic_first_failure_follows_sorted_order() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("a")).unwrap();
        fs::create_dir(dir.path().join("z")).unwrap();
        fs::write(dir.path().join("a/bad"), b"12345").unwrap();
        fs::write(dir.path().join("z/bad"), b"12345").unwrap();
        let limits = ArchiveLimits {
            max_file_bytes: 4,
            ..tiny_limits()
        };
        let first = pack_tree_with_limits(dir.path(), limits).unwrap_err();
        let second = pack_tree_with_limits(dir.path(), limits).unwrap_err();
        for err in [&first, &second] {
            match err {
                ArchiveError::FileSizeLimitExceeded {
                    actual,
                    limit,
                    path,
                } => assert_eq!((*actual, *limit, path.as_str()), (5, 4, "a/bad")),
                other => panic!("unexpected error: {other:?}"),
            }
        }
        assert_eq!(first.to_string(), second.to_string());
        assert_eq!(
            first.to_string(),
            "archive limit exceeded: max_file_bytes actual=5 limit=4 path=a/bad"
        );
    }

    #[test]
    fn encoded_size_overflow_is_bounded_archive_error() {
        let err = checked_encoded_add(u64::MAX, 1).unwrap_err();
        match err {
            ArchiveError::SizeOverflow { context } => {
                assert_eq!(context, "archive encoded length");
                assert_eq!(
                    ArchiveError::SizeOverflow { context }.to_string(),
                    "archive size overflow: archive encoded length"
                );
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn reservation_helpers_fail_without_allocation() {
        let mut additional = Vec::<u8>::new();
        let err = reserve_additional(&mut additional, usize::MAX, "archive bytes").unwrap_err();
        match &err {
            ArchiveError::AllocationFailed { context, requested } => {
                assert_eq!((*context, *requested), ("archive bytes", usize::MAX));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            format!(
                "archive allocation failed: archive bytes requested={}",
                usize::MAX
            )
        );
        assert_eq!((additional.len(), additional.capacity()), (0, 0));

        let mut exact = Vec::<u8>::new();
        let err = reserve_exact(&mut exact, usize::MAX, "archive bytes").unwrap_err();
        match &err {
            ArchiveError::AllocationFailed { context, requested } => {
                assert_eq!((*context, *requested), ("archive bytes", usize::MAX));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            format!(
                "archive allocation failed: archive bytes requested={}",
                usize::MAX
            )
        );
        assert_eq!((exact.len(), exact.capacity()), (0, 0));
    }
}
