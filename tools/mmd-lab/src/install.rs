//! Install trusted binary + out-of-tree digest manifest. Self-check before dispatch.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Schema version for trusted-tools.toml.
pub const TRUSTED_TOOLS_SCHEMA_VERSION: u32 = 1;

/// Filename under `$HOME/.config/mmd-lab/`.
pub const TRUSTED_TOOLS_FILE: &str = "trusted-tools.toml";

/// Default binary name.
pub const BINARY_NAME: &str = "mmd-lab";

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("HOME unset; cannot resolve trusted paths")]
    HomeUnset,
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("manifest parse: {0}")]
    ManifestParse(String),
    #[error("self-check failed: {0}")]
    SelfCheck(String),
}

/// Out-of-tree trusted tools manifest (never from candidate archive).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TrustedToolsManifest {
    pub schema_version: u32,
    pub binary_path: String,
    pub sha256: String,
    #[serde(default)]
    pub installed_at_unix: Option<u64>,
    #[serde(default)]
    pub source_note: Option<String>,
}

impl TrustedToolsManifest {
    pub fn new(binary_path: impl Into<String>, sha256: impl Into<String>) -> Self {
        let installed_at_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs());
        Self {
            schema_version: TRUSTED_TOOLS_SCHEMA_VERSION,
            binary_path: binary_path.into(),
            sha256: sha256.into(),
            installed_at_unix,
            source_note: Some("installed from trusted tree via mmd-lab install".into()),
        }
    }

    pub fn to_toml_string(&self) -> String {
        let mut out = String::new();
        out.push_str("# Trusted mmd-lab digest. Outside candidate worktree. Do not commit.\n");
        out.push_str(&format!("schema_version = {}\n", self.schema_version));
        out.push_str(&format!("binary_path = {:?}\n", self.binary_path));
        out.push_str(&format!("sha256 = {:?}\n", self.sha256));
        if let Some(t) = self.installed_at_unix {
            out.push_str(&format!("installed_at_unix = {t}\n"));
        }
        if let Some(note) = &self.source_note {
            out.push_str(&format!("source_note = {:?}\n", note));
        }
        out
    }
}

/// `$HOME/.config/mmd-lab/trusted-tools.toml`
pub fn default_manifest_path() -> Result<PathBuf, InstallError> {
    Ok(config_dir()?.join(TRUSTED_TOOLS_FILE))
}

/// `$HOME/.local/bin/mmd-lab`
pub fn default_binary_path() -> Result<PathBuf, InstallError> {
    let home = home_dir()?;
    Ok(home.join(".local").join("bin").join(BINARY_NAME))
}

/// `$HOME/.config/mmd-lab`
pub fn config_dir() -> Result<PathBuf, InstallError> {
    Ok(home_dir()?.join(".config").join("mmd-lab"))
}

fn home_dir() -> Result<PathBuf, InstallError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(InstallError::HomeUnset)
}

/// SHA-256 hex of file bytes.
pub fn sha256_file(path: &Path) -> Result<String, InstallError> {
    let bytes = fs::read(path)?;
    Ok(sha256_bytes(&bytes))
}

/// SHA-256 hex of byte slice.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Load trusted-tools.toml.
pub fn load_manifest(path: &Path) -> Result<TrustedToolsManifest, InstallError> {
    let text = fs::read_to_string(path)?;
    toml::from_str(&text).map_err(|e| InstallError::ManifestParse(e.to_string()))
}

/// Write manifest atomically-ish (write tmp + rename when same dir).
pub fn write_manifest(path: &Path, manifest: &TrustedToolsManifest) -> Result<(), InstallError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(manifest.to_toml_string().as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Copy `source_bin` → `dest_bin`, write digest manifest.
///
/// If source already is dest (cargo install path), skip copy — Linux rejects
/// overwriting a running executable (`Text file busy`).
pub fn install_binary(
    source_bin: &Path,
    dest_bin: &Path,
    manifest_path: &Path,
) -> Result<TrustedToolsManifest, InstallError> {
    if let Some(parent) = dest_bin.parent() {
        fs::create_dir_all(parent)?;
    }
    let same = paths_same_file(source_bin, dest_bin)?;
    if !same {
        // Write via temp + rename so a running dest can be replaced on Unix.
        let tmp = dest_bin.with_extension("bin.tmp");
        fs::copy(source_bin, &tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&tmp)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&tmp, perms)?;
        }
        fs::rename(&tmp, dest_bin)?;
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(dest_bin)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(dest_bin, perms)?;
        }
    }
    let digest = sha256_file(dest_bin)?;
    let abs = canonicalize_path(dest_bin)?;
    let manifest = TrustedToolsManifest::new(abs.to_string_lossy(), digest);
    write_manifest(manifest_path, &manifest)?;
    Ok(manifest)
}

fn paths_same_file(a: &Path, b: &Path) -> Result<bool, InstallError> {
    if a == b {
        return Ok(true);
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => Ok(ca == cb),
        _ => Ok(false),
    }
}

/// Install currently running executable as trusted lab binary.
pub fn install_current_exe(
    dest_bin: &Path,
    manifest_path: &Path,
) -> Result<TrustedToolsManifest, InstallError> {
    let exe = std::env::current_exe()?;
    install_binary(&exe, dest_bin, manifest_path)
}

/// Successful self-check payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfCheckOk {
    pub binary_path: PathBuf,
    pub sha256: String,
}

/// Verify absolute path + digest of *this* process against out-of-tree manifest.
///
/// Rejects candidate/workspace binaries that do not match the trusted install.
pub fn self_check(manifest_path: &Path) -> Result<SelfCheckOk, InstallError> {
    if !manifest_path.is_file() {
        return Err(InstallError::SelfCheck(format!(
            "manifest missing: {}",
            manifest_path.display()
        )));
    }
    let manifest = load_manifest(manifest_path)?;
    if manifest.schema_version != TRUSTED_TOOLS_SCHEMA_VERSION {
        return Err(InstallError::SelfCheck(format!(
            "unsupported schema_version {}",
            manifest.schema_version
        )));
    }
    if manifest.sha256.len() != 64 || !manifest.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(InstallError::SelfCheck(
            "manifest sha256 must be 64 hex chars".into(),
        ));
    }

    let trusted_path = PathBuf::from(&manifest.binary_path);
    if !trusted_path.is_absolute() {
        return Err(InstallError::SelfCheck(format!(
            "manifest binary_path must be absolute: {}",
            manifest.binary_path
        )));
    }
    if !trusted_path.is_file() {
        return Err(InstallError::SelfCheck(format!(
            "trusted binary missing: {}",
            trusted_path.display()
        )));
    }

    let running = std::env::current_exe()?;
    let running_canon = canonicalize_path(&running)?;
    let trusted_canon = canonicalize_path(&trusted_path)?;
    if running_canon != trusted_canon {
        return Err(InstallError::SelfCheck(format!(
            "running binary {} is not trusted install {}",
            running_canon.display(),
            trusted_canon.display()
        )));
    }

    let digest = sha256_file(&running_canon)?;
    if !digest.eq_ignore_ascii_case(&manifest.sha256) {
        return Err(InstallError::SelfCheck(format!(
            "digest mismatch: running={digest} trusted={}",
            manifest.sha256
        )));
    }

    Ok(SelfCheckOk {
        binary_path: trusted_canon,
        sha256: digest,
    })
}

/// Self-check against an explicit running binary path (tests; no current_exe).
#[cfg_attr(not(test), allow(dead_code))]
pub fn self_check_binary(
    manifest_path: &Path,
    running_bin: &Path,
) -> Result<SelfCheckOk, InstallError> {
    if !manifest_path.is_file() {
        return Err(InstallError::SelfCheck(format!(
            "manifest missing: {}",
            manifest_path.display()
        )));
    }
    let manifest = load_manifest(manifest_path)?;
    let trusted_path = PathBuf::from(&manifest.binary_path);
    if !trusted_path.is_absolute() {
        return Err(InstallError::SelfCheck(
            "manifest binary_path must be absolute".into(),
        ));
    }
    let running_canon = canonicalize_path(running_bin)?;
    let trusted_canon = canonicalize_path(&trusted_path)?;
    if running_canon != trusted_canon {
        return Err(InstallError::SelfCheck(format!(
            "running binary {} is not trusted install {}",
            running_canon.display(),
            trusted_canon.display()
        )));
    }
    let digest = sha256_file(&running_canon)?;
    if !digest.eq_ignore_ascii_case(&manifest.sha256) {
        return Err(InstallError::SelfCheck(format!(
            "digest mismatch: running={digest} trusted={}",
            manifest.sha256
        )));
    }
    Ok(SelfCheckOk {
        binary_path: trusted_canon,
        sha256: digest,
    })
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, InstallError> {
    fs::canonicalize(path).map_err(InstallError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn install_and_self_check_ok() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src-bin");
        fs::write(&src, b"trusted-lab-bytes-v1").unwrap();
        let dest = dir.path().join("bin").join("mmd-lab");
        let man = dir.path().join("trusted-tools.toml");
        let m = install_binary(&src, &dest, &man).unwrap();
        assert_eq!(m.sha256, sha256_file(&dest).unwrap());
        let ok = self_check_binary(&man, &dest).unwrap();
        assert_eq!(ok.sha256, m.sha256);
    }

    #[test]
    fn self_check_rejects_wrong_digest() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("mmd-lab");
        fs::write(&dest, b"trusted").unwrap();
        let abs = fs::canonicalize(&dest).unwrap();
        let man_path = dir.path().join("trusted-tools.toml");
        let mut m = TrustedToolsManifest::new(abs.to_string_lossy(), sha256_file(&dest).unwrap());
        m.sha256 = "0".repeat(64);
        write_manifest(&man_path, &m).unwrap();
        let err = self_check_binary(&man_path, &dest).unwrap_err();
        assert!(err.to_string().contains("digest mismatch"), "err={err}");
    }
}
