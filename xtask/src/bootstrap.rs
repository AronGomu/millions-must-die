//! SDL3 pin bootstrap + offline check.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::digest::{DigestError, is_exact_semver_pin, is_sha256_hex};

/// Bootstrap failures.
#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error("io error: {0}")]
    Io(String),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("unpinned SDL version: {0}")]
    Unpinned(String),
    #[error("lockfile mismatch: {0}")]
    Lockfile(String),
    #[error("digest error: {0}")]
    Digest(#[from] DigestError),
    #[error("check failed: {0}")]
    Check(String),
}

#[derive(Debug, Deserialize)]
struct VersionsFile {
    schema: Schema,
    sdl3: Sdl3Section,
}

#[derive(Debug, Deserialize)]
struct Schema {
    version: u32,
}

#[derive(Debug, Deserialize)]
struct Sdl3Section {
    source: Sdl3Source,
    crates: Sdl3Crates,
    cache: Sdl3Cache,
    build: Sdl3Builds,
}

#[derive(Debug, Deserialize)]
struct Sdl3Source {
    release: String,
    tarball: String,
    url: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct Sdl3Crates {
    sdl3: String,
    #[serde(rename = "sdl3-sys")]
    sdl3_sys: String,
    sdl3_sys_full: String,
}

#[derive(Debug, Deserialize)]
struct Sdl3Cache {
    cache_root_env: String,
    cache_root_default: String,
}

#[derive(Debug, Deserialize)]
struct Sdl3Builds {
    linux: BuildSpec,
    windows: BuildSpec,
    macos: BuildSpec,
}

#[derive(Debug, Deserialize)]
struct BuildSpec {
    shared: bool,
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    deferred: Option<String>,
}

/// Workspace root = parent of xtask package dir.
pub fn workspace_root_from_xtask_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate lives under workspace root")
        .to_path_buf()
}

fn versions_path(root: &Path) -> PathBuf {
    root.join("third_party/versions.toml")
}

/// Load pinned versions manifest.
fn load_versions(root: &Path) -> Result<VersionsFile, BootstrapError> {
    let path = versions_path(root);
    let raw = fs::read_to_string(&path).map_err(|e| BootstrapError::Io(e.to_string()))?;
    toml::from_str(&raw).map_err(|e| BootstrapError::Manifest(e.to_string()))
}

/// Reject floating / incomplete crate pins.
pub fn validate_crate_pins(sdl3: &str, sdl3_sys: &str) -> Result<(), BootstrapError> {
    if !is_exact_semver_pin(sdl3) {
        return Err(BootstrapError::Unpinned(format!(
            "sdl3 crate pin must be exact x.y.z, got {sdl3:?}"
        )));
    }
    if !is_exact_semver_pin(sdl3_sys) {
        return Err(BootstrapError::Unpinned(format!(
            "sdl3-sys crate pin must be exact x.y.z, got {sdl3_sys:?}"
        )));
    }
    Ok(())
}

/// Validate source pin fields.
fn validate_source_pin(source: &Sdl3Source) -> Result<(), BootstrapError> {
    if source.release.trim().is_empty() {
        return Err(BootstrapError::Unpinned("sdl3 source release empty".into()));
    }
    if !is_exact_semver_pin(&source.release) {
        return Err(BootstrapError::Unpinned(format!(
            "sdl3 source release must be exact x.y.z, got {:?}",
            source.release
        )));
    }
    if source.tarball.trim().is_empty() {
        return Err(BootstrapError::Manifest("sdl3 tarball name empty".into()));
    }
    if !(source.url.starts_with("https://") || source.url.starts_with("http://")) {
        return Err(BootstrapError::Manifest(
            "sdl3 source url must be http(s)".into(),
        ));
    }
    if !is_sha256_hex(&source.sha256) {
        return Err(BootstrapError::Unpinned(format!(
            "sdl3 source sha256 must be 64 lowercase hex chars, got {:?}",
            source.sha256
        )));
    }
    Ok(())
}

/// Ensure Cargo.lock records exact crate versions.
pub fn validate_cargo_lock(root: &Path, sdl3: &str, sdl3_sys: &str) -> Result<(), BootstrapError> {
    let lock_path = root.join("Cargo.lock");
    let lock = fs::read_to_string(&lock_path).map_err(|e| BootstrapError::Io(e.to_string()))?;
    require_lock_package(&lock, "sdl3", sdl3)?;
    require_lock_package(&lock, "sdl3-sys", sdl3_sys)?;
    Ok(())
}

fn require_lock_package(lock: &str, name: &str, version: &str) -> Result<(), BootstrapError> {
    let mut lines = lock.lines().peekable();
    while let Some(line) = lines.next() {
        if line != "[[package]]" {
            continue;
        }
        let mut pkg_name = None;
        let mut pkg_ver = None;
        while let Some(next) = lines.peek().copied() {
            if next.starts_with("[[") {
                break;
            }
            let l = lines.next().unwrap();
            if let Some(v) = l.strip_prefix("name = \"") {
                pkg_name = Some(v.trim_end_matches('"').to_string());
            } else if let Some(v) = l.strip_prefix("version = \"") {
                pkg_ver = Some(v.trim_end_matches('"').to_string());
            }
        }
        if pkg_name.as_deref() == Some(name) {
            let Some(found) = pkg_ver else {
                return Err(BootstrapError::Lockfile(format!(
                    "package {name} missing version field"
                )));
            };
            // Accept Cargo semver build metadata (e.g. 0.6.7+SDL-3.4.12).
            let base = found.split('+').next().unwrap_or(found.as_str());
            if base == version || found == version {
                return Ok(());
            }
            return Err(BootstrapError::Lockfile(format!(
                "package {name} version {found:?} != pinned {version}"
            )));
        }
    }
    Err(BootstrapError::Lockfile(format!(
        "package {name} {version} missing from Cargo.lock (add workspace pin + dep)"
    )))
}

/// Ensure workspace Cargo.toml pins match versions.toml.
pub fn validate_workspace_cargo_toml(
    root: &Path,
    sdl3: &str,
    sdl3_sys: &str,
) -> Result<(), BootstrapError> {
    let toml_path = root.join("Cargo.toml");
    let body = fs::read_to_string(&toml_path).map_err(|e| BootstrapError::Io(e.to_string()))?;
    // Accept either "0.18.4" or "=0.18.4" forms.
    let sdl3_ok = body.contains(&format!("sdl3 = {{ version = \"={sdl3}\""))
        || body.contains(&format!("sdl3 = {{ version = \"{sdl3}\""))
        || body.contains(&format!("sdl3 = \"={sdl3}\""))
        || body.contains(&format!("sdl3 = \"{sdl3}\""));
    let sys_ok = body.contains(&format!("sdl3-sys = {{ version = \"={sdl3_sys}\""))
        || body.contains(&format!("sdl3-sys = {{ version = \"{sdl3_sys}\""))
        || body.contains(&format!("sdl3-sys = \"={sdl3_sys}\""))
        || body.contains(&format!("sdl3-sys = \"{sdl3_sys}\""));
    if !sdl3_ok {
        return Err(BootstrapError::Check(format!(
            "workspace Cargo.toml missing exact sdl3 pin {sdl3}"
        )));
    }
    if !sys_ok {
        return Err(BootstrapError::Check(format!(
            "workspace Cargo.toml missing exact sdl3-sys pin {sdl3_sys}"
        )));
    }
    Ok(())
}

fn validate_builds(builds: &Sdl3Builds) -> Result<(), BootstrapError> {
    if !builds.linux.shared || builds.linux.commands.is_empty() {
        return Err(BootstrapError::Manifest(
            "linux shared build commands required".into(),
        ));
    }
    if builds.windows.commands.is_empty() {
        return Err(BootstrapError::Manifest(
            "windows build commands required (deferred T9 ok)".into(),
        ));
    }
    if builds.macos.commands.is_empty() {
        return Err(BootstrapError::Manifest(
            "macos build commands required (deferred T10 ok)".into(),
        ));
    }
    if builds.windows.deferred.as_deref() != Some("T9") {
        return Err(BootstrapError::Manifest(
            "windows build must declare deferred = \"T9\"".into(),
        ));
    }
    if builds.macos.deferred.as_deref() != Some("T10") {
        return Err(BootstrapError::Manifest(
            "macos build must declare deferred = \"T10\"".into(),
        ));
    }
    Ok(())
}

/// Full offline bootstrap check (no network).
pub fn check_bootstrap(root: &Path) -> Result<(), BootstrapError> {
    let versions = load_versions(root)?;
    if versions.schema.version != 1 {
        return Err(BootstrapError::Manifest(format!(
            "unsupported schema version {}",
            versions.schema.version
        )));
    }
    validate_source_pin(&versions.sdl3.source)?;
    validate_crate_pins(&versions.sdl3.crates.sdl3, &versions.sdl3.crates.sdl3_sys)?;
    if !versions
        .sdl3
        .crates
        .sdl3_sys_full
        .starts_with(&versions.sdl3.crates.sdl3_sys)
    {
        return Err(BootstrapError::Check(
            "sdl3_sys_full must start with sdl3-sys version".into(),
        ));
    }
    if !versions
        .sdl3
        .crates
        .sdl3_sys_full
        .contains(&versions.sdl3.source.release)
    {
        return Err(BootstrapError::Check(
            "sdl3_sys_full must embed SDL source release".into(),
        ));
    }
    validate_builds(&versions.sdl3.build)?;
    if versions.sdl3.cache.cache_root_env.trim().is_empty()
        || versions.sdl3.cache.cache_root_default.trim().is_empty()
    {
        return Err(BootstrapError::Manifest(
            "cache root config incomplete".into(),
        ));
    }
    validate_workspace_cargo_toml(
        root,
        &versions.sdl3.crates.sdl3,
        &versions.sdl3.crates.sdl3_sys,
    )?;
    validate_cargo_lock(
        root,
        &versions.sdl3.crates.sdl3,
        &versions.sdl3.crates.sdl3_sys,
    )?;
    // Repo must not vendor native SDL trees.
    let banned = [
        root.join("third_party/SDL"),
        root.join("third_party/SDL3"),
        root.join("third_party/sdl3-src"),
    ];
    for p in banned {
        if p.exists() {
            return Err(BootstrapError::Check(format!(
                "native SDL tree must stay outside git: {}",
                p.display()
            )));
        }
    }
    Ok(())
}

/// CLI entry.
pub fn run_bootstrap(check: bool) -> Result<(), BootstrapError> {
    let root = workspace_root_from_xtask_manifest();
    if check {
        check_bootstrap(&root)?;
        let v = load_versions(&root)?;
        println!(
            "bootstrap: ok (SDL {} / sdl3 {} / sdl3-sys {}; win/mac native deferred T9/T10)",
            v.sdl3.source.release, v.sdl3.crates.sdl3, v.sdl3.crates.sdl3_sys
        );
        return Ok(());
    }

    // Non-check path documents cache layout only in T6; full source fetch/build is optional
    // operator action (may use network once). Never invoked from Cargo build.rs.
    let v = load_versions(&root)?;
    validate_source_pin(&v.sdl3.source)?;
    validate_crate_pins(&v.sdl3.crates.sdl3, &v.sdl3.crates.sdl3_sys)?;
    let cache = resolve_cache_root(&v.sdl3.cache);
    let prefix = cache
        .join("sdl3")
        .join(&v.sdl3.source.release)
        .join(format!("prefix-{}", std::env::consts::OS));
    println!("bootstrap: pins ok");
    println!(
        "  source {} sha256={}",
        v.sdl3.source.release, v.sdl3.source.sha256
    );
    println!("  cache root {}", cache.display());
    println!("  expected prefix {}", prefix.display());
    println!("  linux cmds: {}", v.sdl3.build.linux.commands.len());
    println!("  windows/macOS shared builds deferred (T9/T10)");
    println!("  run with --check for offline verification");
    Ok(())
}

fn resolve_cache_root(cache: &Sdl3Cache) -> PathBuf {
    if let Ok(v) = std::env::var(&cache.cache_root_env)
        && !v.trim().is_empty()
    {
        return PathBuf::from(v);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(&cache.cache_root_default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unpinned_sdl() {
        assert!(validate_crate_pins("0.18", "0.6.7").is_err());
        assert!(validate_crate_pins("^0.18.4", "0.6.7").is_err());
        assert!(validate_crate_pins("0.18.4", "0.6").is_err());
        assert!(validate_crate_pins("0.18.4", "~0.6.7").is_err());
        assert!(validate_crate_pins("0.18.4", "0.6.7").is_ok());

        let bad_source = Sdl3Source {
            release: "3.4".into(),
            tarball: "x.tar.gz".into(),
            url: "https://example.com/x".into(),
            sha256: "abcd".into(),
        };
        assert!(validate_source_pin(&bad_source).is_err());

        let good_source = Sdl3Source {
            release: "3.4.12".into(),
            tarball: "SDL3-3.4.12.tar.gz".into(),
            url: "https://github.com/libsdl-org/SDL/releases/download/release-3.4.12/SDL3-3.4.12.tar.gz".into(),
            sha256: "f07b958a9ac5020fb7a44cadb957f658b2149c3c8abb4f63145fac9303249db7".into(),
        };
        assert!(validate_source_pin(&good_source).is_ok());
    }

    #[test]
    fn tracked_versions_pass_check() {
        let root = workspace_root_from_xtask_manifest();
        check_bootstrap(&root).expect("bootstrap check");
    }
}
