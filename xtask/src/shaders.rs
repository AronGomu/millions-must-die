//! Offline shader artifact checks.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::digest::{DigestError, verify_file_sha256};

const REQUIRED_FORMATS: [&str; 3] = ["spirv", "dxil", "metallib"];

/// Shader check failures.
#[derive(Debug, Error)]
pub enum ShaderError {
    #[error("io error: {0}")]
    Io(String),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("digest error: {0}")]
    Digest(#[from] DigestError),
    #[error("check failed: {0}")]
    Check(String),
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ShaderManifest {
    version: u32,
    canonical: String,
    canonical_sha256: String,
    generator: String,
    entry_points: EntryPoints,
    resources: Resources,
    formats: BTreeMapFormats,
}

use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
struct EntryPoints {
    vertex: String,
    fragment: String,
}

#[derive(Debug, Deserialize)]
struct Resources {
    uniform_buffers: u32,
    samplers: u32,
    sampled_textures: u32,
    storage_buffers: u32,
    storage_textures: u32,
}

#[derive(Debug, Deserialize)]
struct FormatSlot {
    #[allow(dead_code)]
    status: String,
    #[serde(default)]
    deferred: Option<String>,
    files: Vec<ArtifactFile>,
}

#[derive(Debug, Deserialize)]
struct ArtifactFile {
    #[allow(dead_code)]
    stage: String,
    file: String,
    sha256: String,
}

type BTreeMapFormats = BTreeMap<String, FormatSlot>;

/// Workspace root = parent of xtask package dir.
pub fn workspace_root_from_xtask_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate lives under workspace root")
        .to_path_buf()
}

fn generated_dir(root: &Path) -> PathBuf {
    root.join("shaders/generated")
}

fn load_manifest(dir: &Path) -> Result<ShaderManifest, ShaderError> {
    let path = dir.join("manifest.json");
    let raw = fs::read_to_string(&path).map_err(|e| ShaderError::Io(e.to_string()))?;
    // Keep unknown fields tolerant via Value round-trip only if needed.
    serde_json::from_str(&raw).map_err(|e| ShaderError::Manifest(e.to_string()))
}

/// Ensure manifest lists every backend format slot.
fn manifest_has_all_shader_formats(manifest: &ShaderManifest) -> Result<(), ShaderError> {
    for fmt in REQUIRED_FORMATS {
        if !manifest.formats.contains_key(fmt) {
            return Err(ShaderError::Check(format!(
                "shader manifest missing format slot {fmt}"
            )));
        }
        let slot = &manifest.formats[fmt];
        if slot.files.is_empty() {
            return Err(ShaderError::Check(format!(
                "shader format {fmt} has no files"
            )));
        }
    }
    Ok(())
}

/// Verify canonical HLSL + every tracked blob hash.
fn artifact_hashes_match(root: &Path, manifest: &ShaderManifest) -> Result<(), ShaderError> {
    let canonical = root.join(&manifest.canonical);
    verify_file_sha256(&canonical, &manifest.canonical_sha256)?;
    let out_dir = generated_dir(root);
    for (fmt, slot) in &manifest.formats {
        for file in &slot.files {
            let path = out_dir.join(&file.file);
            if !path.is_file() {
                return Err(ShaderError::Check(format!(
                    "missing {fmt} artifact {}",
                    file.file
                )));
            }
            verify_file_sha256(&path, &file.sha256)?;
        }
    }
    Ok(())
}

fn validate_resources(resources: &Resources) -> Result<(), ShaderError> {
    if resources.uniform_buffers != 1
        || resources.samplers != 1
        || resources.sampled_textures != 1
        || resources.storage_buffers != 0
        || resources.storage_textures != 0
    {
        return Err(ShaderError::Check(format!(
            "unexpected resource contract: ubo={} samp={} tex={} ssbo={} stor_tex={}",
            resources.uniform_buffers,
            resources.samplers,
            resources.sampled_textures,
            resources.storage_buffers,
            resources.storage_textures
        )));
    }
    Ok(())
}

fn validate_deferred_placeholders(
    out_dir: &Path,
    manifest: &ShaderManifest,
) -> Result<(), ShaderError> {
    let dxil = manifest
        .formats
        .get("dxil")
        .ok_or_else(|| ShaderError::Check("dxil missing".into()))?;
    let metal = manifest
        .formats
        .get("metallib")
        .ok_or_else(|| ShaderError::Check("metallib missing".into()))?;

    // Real DXIL starts with DXBC magic; placeholders remain deferred=T9 until native regen.
    let dxil_real = dxil_blobs_look_native(out_dir, dxil)?;
    if dxil_real {
        if dxil.deferred.is_some() || dxil.status != "tracked" {
            return Err(ShaderError::Check(
                "native DXIL present: set status=tracked and drop deferred".into(),
            ));
        }
    } else if dxil.deferred.as_deref() != Some("T9") {
        return Err(ShaderError::Check(
            "dxil slot must declare deferred = T9 until Windows native regen".into(),
        ));
    }

    // Real metallib starts with MTLB magic; placeholders remain deferred=T10 until native regen.
    let metal_real = metallib_blobs_look_native(out_dir, metal)?;
    if metal_real {
        if metal.deferred.is_some() || metal.status != "tracked" {
            return Err(ShaderError::Check(
                "native metallib present: set status=tracked and drop deferred".into(),
            ));
        }
    } else if metal.deferred.as_deref() != Some("T10") {
        return Err(ShaderError::Check(
            "metallib slot must declare deferred = T10 until macOS native regen".into(),
        ));
    }
    Ok(())
}

/// DXIL container magic is ASCII `DXBC` (DirectX Bytecode container housing DXIL).
fn is_dxbc_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0..4] == *b"DXBC"
}

/// Apple Metal library magic is ASCII `MTLB`.
fn is_mtlb_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0..4] == *b"MTLB"
}

fn dxil_blobs_look_native(out_dir: &Path, slot: &FormatSlot) -> Result<bool, ShaderError> {
    let mut any = false;
    let mut all_native = true;
    for file in &slot.files {
        any = true;
        let path = out_dir.join(&file.file);
        let bytes = fs::read(&path).map_err(|e| ShaderError::Io(e.to_string()))?;
        if is_dxbc_magic(&bytes) {
            continue;
        }
        all_native = false;
        // Placeholder path must keep the MMD marker so accidental binary junk fails loud.
        if !bytes.starts_with(b"MMD_PLACEHOLDER_DXIL") {
            return Err(ShaderError::Check(format!(
                "{} is neither DXBC/DXIL nor MMD_PLACEHOLDER_DXIL",
                file.file
            )));
        }
    }
    Ok(any && all_native)
}

fn metallib_blobs_look_native(out_dir: &Path, slot: &FormatSlot) -> Result<bool, ShaderError> {
    let mut any = false;
    let mut all_native = true;
    for file in &slot.files {
        any = true;
        let path = out_dir.join(&file.file);
        let bytes = fs::read(&path).map_err(|e| ShaderError::Io(e.to_string()))?;
        if is_mtlb_magic(&bytes) {
            continue;
        }
        all_native = false;
        if !bytes.starts_with(b"MMD_PLACEHOLDER_METALLIB") {
            return Err(ShaderError::Check(format!(
                "{} is neither MTLB/metallib nor MMD_PLACEHOLDER_METALLIB",
                file.file
            )));
        }
    }
    Ok(any && all_native)
}

/// Full offline shader check.
pub fn check_shaders(root: &Path) -> Result<(), ShaderError> {
    let out_dir = generated_dir(root);
    let manifest = load_manifest(&out_dir)?;
    if manifest.version != 1 {
        return Err(ShaderError::Manifest(format!(
            "unsupported manifest version {}",
            manifest.version
        )));
    }
    if manifest.generator != "mmd-shader-pin-v1" {
        return Err(ShaderError::Manifest(format!(
            "unexpected generator {}",
            manifest.generator
        )));
    }
    if manifest.entry_points.vertex != "VSMain" || manifest.entry_points.fragment != "PSMain" {
        return Err(ShaderError::Check(
            "entry points must be VSMain/PSMain".into(),
        ));
    }
    validate_resources(&manifest.resources)?;
    manifest_has_all_shader_formats(&manifest)?;
    validate_deferred_placeholders(&out_dir, &manifest)?;
    artifact_hashes_match(root, &manifest)?;

    // SPIR-V magic word 0x07230203 little-endian
    for name in ["sprite.vert.spv", "sprite.frag.spv"] {
        let bytes = fs::read(out_dir.join(name)).map_err(|e| ShaderError::Io(e.to_string()))?;
        if bytes.len() < 4 || bytes[0..4] != [0x03, 0x02, 0x23, 0x07] {
            return Err(ShaderError::Check(format!("{name} missing SPIR-V magic")));
        }
    }
    Ok(())
}

/// Confirm workspace packages have no network-fetching build.rs.
pub fn build_has_no_network_fetch(root: &Path) -> Result<(), ShaderError> {
    let mut offenders = Vec::new();
    let build_rs_paths = [
        root.join("build.rs"),
        root.join("xtask/build.rs"),
        root.join("crates/mmd-engine/build.rs"),
        root.join("tools/mmd-lab/build.rs"),
    ];
    for path in build_rs_paths {
        if !path.exists() {
            continue;
        }
        let body = fs::read_to_string(&path).map_err(|e| ShaderError::Io(e.to_string()))?;
        let lower = body.to_ascii_lowercase();
        for needle in [
            "reqwest",
            "curl ",
            "wget ",
            "ureq",
            "http::",
            "https://",
            "std::net::",
            "tokio::net",
            "Command::new(\"curl\")",
            "Command::new(\"wget\")",
            "Command::new(\"git\")",
        ] {
            if lower.contains(&needle.to_ascii_lowercase()) {
                offenders.push(format!("{} contains {needle:?}", path.display()));
            }
        }
    }
    // Also scan package dirs for unexpected build.rs network usage under workspace members only.
    if !offenders.is_empty() {
        return Err(ShaderError::Check(offenders.join("; ")));
    }
    Ok(())
}

/// CLI entry.
pub fn run_shaders(check: bool) -> Result<(), ShaderError> {
    let root = workspace_root_from_xtask_manifest();
    if check {
        check_shaders(&root)?;
        build_has_no_network_fetch(&root)?;
        println!(
            "shaders: ok (spirv+dxil+metallib; native DXIL/metallib regen still host-gated T9/T10)"
        );
        return Ok(());
    }
    // Tracked blobs always present; DXIL/metallib native regen is host-native (T9/T10).
    check_shaders(&root)?;
    println!("shaders: tracked artifacts already present; use --check in gates");
    println!("  DXIL native regen: Windows DXC (docs/platform/windows-bootstrap.md)");
    println!("  metallib native regen: macOS Metal tools (docs/platform/macos-bootstrap.md)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::sha256_file;
    use std::collections::BTreeSet;

    #[test]
    fn manifest_has_all_shader_formats_ok() {
        let root = workspace_root_from_xtask_manifest();
        let manifest = load_manifest(&generated_dir(&root)).expect("manifest");
        manifest_has_all_shader_formats(&manifest).expect("formats");
        let keys: BTreeSet<_> = manifest.formats.keys().map(|s| s.as_str()).collect();
        assert!(keys.contains("spirv"));
        assert!(keys.contains("dxil"));
        assert!(keys.contains("metallib"));
    }

    #[test]
    fn artifact_hashes_match_ok() {
        let root = workspace_root_from_xtask_manifest();
        let manifest = load_manifest(&generated_dir(&root)).expect("manifest");
        artifact_hashes_match(&root, &manifest).expect("hashes");
        // Stable re-hash of one blob.
        let spv = generated_dir(&root).join("sprite.vert.spv");
        let digest = sha256_file(&spv).expect("sha");
        assert_eq!(digest, manifest.formats["spirv"].files[0].sha256);
    }

    #[test]
    fn build_has_no_network_fetch_ok() {
        let root = workspace_root_from_xtask_manifest();
        build_has_no_network_fetch(&root).expect("no network build.rs");
    }

    #[test]
    fn missing_format_rejected() {
        let mut manifest =
            load_manifest(&generated_dir(&workspace_root_from_xtask_manifest())).expect("manifest");
        manifest.formats.remove("dxil");
        assert!(manifest_has_all_shader_formats(&manifest).is_err());
    }

    #[test]
    fn check_shaders_tracked_tree() {
        let root = workspace_root_from_xtask_manifest();
        check_shaders(&root).expect("check");
    }

    #[test]
    fn dxil_placeholder_not_native_magic() {
        let root = workspace_root_from_xtask_manifest();
        let dir = generated_dir(&root);
        let manifest = load_manifest(&dir).expect("manifest");
        let dxil = &manifest.formats["dxil"];
        // Until Windows native regen, placeholders must not look like DXBC.
        let native = dxil_blobs_look_native(&dir, dxil).expect("read");
        assert!(!native, "DXIL still placeholder until T9 Windows regen");
        assert_eq!(dxil.deferred.as_deref(), Some("T9"));
    }

    #[test]
    fn dxbc_magic_helper() {
        assert!(is_dxbc_magic(b"DXBC\0rest"));
        assert!(!is_dxbc_magic(b"MMD_PLACEHOLDER_DXIL"));
        assert!(!is_dxbc_magic(b"DXB"));
    }

    #[test]
    fn metallib_placeholder_not_native_magic() {
        let root = workspace_root_from_xtask_manifest();
        let dir = generated_dir(&root);
        let manifest = load_manifest(&dir).expect("manifest");
        let metal = &manifest.formats["metallib"];
        // Until macOS native regen, placeholders must not look like MTLB.
        let native = metallib_blobs_look_native(&dir, metal).expect("read");
        assert!(!native, "metallib still placeholder until T10 macOS regen");
        assert_eq!(metal.deferred.as_deref(), Some("T10"));
    }

    #[test]
    fn mtlb_magic_helper() {
        assert!(is_mtlb_magic(b"MTLB\0rest"));
        assert!(!is_mtlb_magic(b"MMD_PLACEHOLDER_METALLIB"));
        assert!(!is_mtlb_magic(b"MTL"));
    }
}
