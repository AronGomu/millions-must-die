//! Deterministic placeholder sprite atlas generator.
//!
//! Layout (fixed): 8 direction rows × 4 animation frame columns.
//! Each frame is a 3×3 premultiplied-alpha sprite.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Manifest schema version.
pub const MANIFEST_VERSION: u32 = 1;

/// Atlas count locked by phase-0 contract.
pub const ATLAS_COUNT: u32 = 4;
/// Cardinal + diagonal directions.
pub const DIRECTION_COUNT: u32 = 8;
/// Animation frames per direction.
pub const FRAME_COUNT: u32 = 4;
/// Sprite edge length in pixels.
pub const SPRITE_SIZE_PX: u32 = 3;

/// Frames across atlas X axis (frame index).
pub const FRAMES_X: u32 = FRAME_COUNT;
/// Frames down atlas Y axis (direction index).
pub const FRAMES_Y: u32 = DIRECTION_COUNT;

/// Total frames packed per atlas.
pub const FRAMES_PER_ATLAS: u32 = DIRECTION_COUNT * FRAME_COUNT;

/// Atlas pixel width.
pub const ATLAS_WIDTH_PX: u32 = FRAMES_X * SPRITE_SIZE_PX;
/// Atlas pixel height.
pub const ATLAS_HEIGHT_PX: u32 = FRAMES_Y * SPRITE_SIZE_PX;

const GENERATOR_ID: &str = "mmd-placeholder-v1";

/// Atlas generation / check failures.
#[derive(Debug, Error)]
pub enum AtlasError {
    #[error("io error: {0}")]
    Io(String),
    #[error("png encode error: {0}")]
    Encode(String),
    #[error("png decode error: {0}")]
    Decode(String),
    #[error("manifest error: {0}")]
    Manifest(String),
    #[error("hash mismatch for {file}: expected {expected}, got {actual}")]
    HashMismatch {
        file: String,
        expected: String,
        actual: String,
    },
    #[error("layout mismatch: {0}")]
    Layout(String),
    #[error("pixel not premultiplied at ({x},{y}) in {file}: rgba=({r},{g},{b},{a})")]
    NotPremultiplied {
        file: String,
        x: u32,
        y: u32,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    },
    #[error("check failed: {0}")]
    Check(String),
}

/// One tracked atlas entry in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasEntry {
    pub id: u32,
    pub file: String,
    pub sha256: String,
}

/// Fixed atlas pack layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasLayout {
    pub frames_x: u32,
    pub frames_y: u32,
    pub sprite_size_px: u32,
    pub order: String,
}

/// Tracked atlas set manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasManifest {
    pub version: u32,
    pub generator: String,
    pub atlas_count: u32,
    pub direction_count: u32,
    pub frame_count: u32,
    pub frames_per_atlas: u32,
    pub layout: AtlasLayout,
    pub atlases: Vec<AtlasEntry>,
}

/// Resolve default generated atlas directory from workspace root.
pub fn default_output_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("assets/sprites/generated")
}

/// Workspace root = parent of xtask package dir.
pub fn workspace_root_from_xtask_manifest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate lives under workspace root")
        .to_path_buf()
}

/// Generate four atlases + manifest under `out_dir`.
pub fn generate_atlases(out_dir: &Path) -> Result<AtlasManifest, AtlasError> {
    fs::create_dir_all(out_dir).map_err(|e| AtlasError::Io(e.to_string()))?;

    let mut entries = Vec::with_capacity(ATLAS_COUNT as usize);
    for id in 0..ATLAS_COUNT {
        let file_name = format!("atlas_{id}.png");
        let path = out_dir.join(&file_name);
        let png = encode_atlas_png(id)?;
        fs::write(&path, &png).map_err(|e| AtlasError::Io(e.to_string()))?;
        let digest = sha256_hex(&png);
        entries.push(AtlasEntry {
            id,
            file: file_name,
            sha256: digest,
        });
    }

    let manifest = AtlasManifest {
        version: MANIFEST_VERSION,
        generator: GENERATOR_ID.to_string(),
        atlas_count: ATLAS_COUNT,
        direction_count: DIRECTION_COUNT,
        frame_count: FRAME_COUNT,
        frames_per_atlas: FRAMES_PER_ATLAS,
        layout: AtlasLayout {
            frames_x: FRAMES_X,
            frames_y: FRAMES_Y,
            sprite_size_px: SPRITE_SIZE_PX,
            order: "dir_rows_frame_cols".to_string(),
        },
        atlases: entries,
    };

    write_manifest(out_dir, &manifest)?;
    Ok(manifest)
}

/// Write canonical pretty JSON manifest.
pub fn write_manifest(out_dir: &Path, manifest: &AtlasManifest) -> Result<(), AtlasError> {
    let path = out_dir.join("manifest.json");
    let mut body =
        serde_json::to_string_pretty(manifest).map_err(|e| AtlasError::Manifest(e.to_string()))?;
    body.push('\n');
    fs::write(path, body).map_err(|e| AtlasError::Io(e.to_string()))
}

/// Load manifest from disk.
pub fn load_manifest(out_dir: &Path) -> Result<AtlasManifest, AtlasError> {
    let path = out_dir.join("manifest.json");
    let raw = fs::read_to_string(&path).map_err(|e| AtlasError::Io(e.to_string()))?;
    serde_json::from_str(&raw).map_err(|e| AtlasError::Manifest(e.to_string()))
}

/// Regenerate into memory-backed checks against on-disk tracked assets.
pub fn check_atlases(out_dir: &Path) -> Result<(), AtlasError> {
    let on_disk = load_manifest(out_dir)?;
    validate_manifest_layout(&on_disk)?;

    let tmp = tempfile_dir()?;
    let fresh = generate_atlases(&tmp)?;
    if fresh != on_disk {
        return Err(AtlasError::Check(
            "regenerated manifest differs from tracked manifest.json".into(),
        ));
    }

    for entry in &on_disk.atlases {
        let tracked =
            fs::read(out_dir.join(&entry.file)).map_err(|e| AtlasError::Io(e.to_string()))?;
        let expected = sha256_hex(&tracked);
        if expected != entry.sha256 {
            return Err(AtlasError::HashMismatch {
                file: entry.file.clone(),
                expected: entry.sha256.clone(),
                actual: expected,
            });
        }
        let regenerated =
            fs::read(tmp.join(&entry.file)).map_err(|e| AtlasError::Io(e.to_string()))?;
        if tracked != regenerated {
            return Err(AtlasError::Check(format!(
                "png bytes drift for {}",
                entry.file
            )));
        }
        assert_premultiplied(&entry.file, &tracked)?;
    }

    Ok(())
}

fn tempfile_dir() -> Result<PathBuf, AtlasError> {
    let dir = std::env::temp_dir().join(format!(
        "mmd-atlases-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    fs::create_dir_all(&dir).map_err(|e| AtlasError::Io(e.to_string()))?;
    Ok(dir)
}

/// Validate locked layout fields on a manifest.
pub fn validate_manifest_layout(manifest: &AtlasManifest) -> Result<(), AtlasError> {
    if manifest.version != MANIFEST_VERSION {
        return Err(AtlasError::Layout(format!(
            "version {} != {MANIFEST_VERSION}",
            manifest.version
        )));
    }
    if manifest.atlas_count != ATLAS_COUNT {
        return Err(AtlasError::Layout(format!(
            "atlas_count {} != {ATLAS_COUNT}",
            manifest.atlas_count
        )));
    }
    if manifest.direction_count != DIRECTION_COUNT {
        return Err(AtlasError::Layout(format!(
            "direction_count {} != {DIRECTION_COUNT}",
            manifest.direction_count
        )));
    }
    if manifest.frame_count != FRAME_COUNT {
        return Err(AtlasError::Layout(format!(
            "frame_count {} != {FRAME_COUNT}",
            manifest.frame_count
        )));
    }
    if manifest.frames_per_atlas != FRAMES_PER_ATLAS {
        return Err(AtlasError::Layout(format!(
            "frames_per_atlas {} != {FRAMES_PER_ATLAS}",
            manifest.frames_per_atlas
        )));
    }
    if manifest.atlases.len() != ATLAS_COUNT as usize {
        return Err(AtlasError::Layout(format!(
            "atlases len {} != {ATLAS_COUNT}",
            manifest.atlases.len()
        )));
    }
    if manifest.layout.frames_x != FRAMES_X
        || manifest.layout.frames_y != FRAMES_Y
        || manifest.layout.sprite_size_px != SPRITE_SIZE_PX
    {
        return Err(AtlasError::Layout("frame grid constants drifted".into()));
    }
    if manifest.layout.order != "dir_rows_frame_cols" {
        return Err(AtlasError::Layout(format!(
            "unexpected order {}",
            manifest.layout.order
        )));
    }
    Ok(())
}

/// Encode one atlas PNG (deterministic bytes).
pub fn encode_atlas_png(atlas_id: u32) -> Result<Vec<u8>, AtlasError> {
    let pixels = render_atlas_rgba(atlas_id);
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, ATLAS_WIDTH_PX, ATLAS_HEIGHT_PX);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fastest);
        encoder.set_filter(png::Filter::NoFilter);
        let mut writer = encoder
            .write_header()
            .map_err(|e| AtlasError::Encode(e.to_string()))?;
        writer
            .write_image_data(&pixels)
            .map_err(|e| AtlasError::Encode(e.to_string()))?;
    }
    Ok(out)
}

/// Fill RGBA buffer for one atlas.
pub fn render_atlas_rgba(atlas_id: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (ATLAS_WIDTH_PX * ATLAS_HEIGHT_PX * 4) as usize];
    for dir in 0..DIRECTION_COUNT {
        for frame in 0..FRAME_COUNT {
            let origin_x = frame * SPRITE_SIZE_PX;
            let origin_y = dir * SPRITE_SIZE_PX;
            for ly in 0..SPRITE_SIZE_PX {
                for lx in 0..SPRITE_SIZE_PX {
                    let [r, g, b, a] = sprite_pixel(atlas_id, dir, frame, lx, ly);
                    let x = origin_x + lx;
                    let y = origin_y + ly;
                    let idx = ((y * ATLAS_WIDTH_PX + x) * 4) as usize;
                    pixels[idx] = r;
                    pixels[idx + 1] = g;
                    pixels[idx + 2] = b;
                    pixels[idx + 3] = a;
                }
            }
        }
    }
    pixels
}

/// Deterministic premultiplied placeholder pixel.
///
/// RGB always ≤ A. Pattern encodes atlas/dir/frame/local coords so frames differ.
pub fn sprite_pixel(atlas_id: u32, dir: u32, frame: u32, lx: u32, ly: u32) -> [u8; 4] {
    // Edge fade keeps some translucent coverage for blend tests.
    let edge =
        u32::from(lx == 0 || ly == 0 || lx == SPRITE_SIZE_PX - 1 || ly == SPRITE_SIZE_PX - 1);
    let a = 180u32 + frame * 12 + (1 - edge) * 30;
    let a = a.min(255) as u8;

    let base_r = 40 + atlas_id * 40;
    let base_g = 30 + dir * 18;
    let base_b = 50 + frame * 28 + lx * 10 + ly * 7;

    let r = scale_premul(base_r, a);
    let g = scale_premul(base_g, a);
    let b = scale_premul(base_b, a);
    [r, g, b, a]
}

fn scale_premul(channel: u32, a: u8) -> u8 {
    let c = channel.min(255);
    ((c * u32::from(a)) / 255).min(u32::from(a)) as u8
}

/// Decode PNG and assert every pixel is premultiplied (RGB ≤ A).
pub fn assert_premultiplied(file: &str, png_bytes: &[u8]) -> Result<(), AtlasError> {
    let decoder = png::Decoder::new(Cursor::new(png_bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| AtlasError::Decode(e.to_string()))?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| AtlasError::Decode(e.to_string()))?;
    let bytes = &buf[..info.buffer_size()];
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(AtlasError::Decode(format!(
            "{file}: expected 8-bit RGBA, got {:?} {:?}",
            info.color_type, info.bit_depth
        )));
    }
    let w = info.width;
    for (i, px) in bytes.chunks_exact(4).enumerate() {
        let r = px[0];
        let g = px[1];
        let b = px[2];
        let a = px[3];
        if r > a || g > a || b > a {
            let x = (i as u32) % w;
            let y = (i as u32) / w;
            return Err(AtlasError::NotPremultiplied {
                file: file.to_string(),
                x,
                y,
                r,
                g,
                b,
                a,
            });
        }
    }
    Ok(())
}

/// SHA-256 hex digest of bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// CLI entry: write or check generated atlases.
pub fn run_atlases(check: bool) -> Result<(), AtlasError> {
    let root = workspace_root_from_xtask_manifest();
    let out = default_output_dir(&root);
    if check {
        check_atlases(&out)?;
        println!("atlases: ok ({ATLAS_COUNT} png + manifest)");
    } else {
        let manifest = generate_atlases(&out)?;
        println!(
            "atlases: wrote {} png + manifest → {}",
            manifest.atlases.len(),
            out.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn temp_out() -> PathBuf {
        tempfile::tempdir().expect("tempdir").keep()
    }

    #[test]
    fn generates_four_atlases() {
        let out = temp_out();
        let manifest = generate_atlases(&out).expect("generate");
        assert_eq!(manifest.atlas_count, 4);
        assert_eq!(manifest.atlases.len(), 4);
        for id in 0..4u32 {
            let path = out.join(format!("atlas_{id}.png"));
            assert!(path.is_file(), "missing {}", path.display());
            let bytes = fs::read(&path).expect("read png");
            assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "not png");
        }
        assert!(out.join("manifest.json").is_file());
    }

    #[test]
    fn layout_has_8x4_frames() {
        let out = temp_out();
        let manifest = generate_atlases(&out).expect("generate");
        validate_manifest_layout(&manifest).expect("layout");
        assert_eq!(manifest.direction_count, 8);
        assert_eq!(manifest.frame_count, 4);
        assert_eq!(manifest.frames_per_atlas, 32);
        assert_eq!(manifest.layout.frames_x, 4);
        assert_eq!(manifest.layout.frames_y, 8);
        assert_eq!(ATLAS_WIDTH_PX, 12);
        assert_eq!(ATLAS_HEIGHT_PX, 24);

        for entry in &manifest.atlases {
            let png = fs::read(out.join(&entry.file)).expect("png");
            let decoder = png::Decoder::new(Cursor::new(png));
            let reader = decoder.read_info().expect("info");
            let info = reader.info();
            assert_eq!(info.width, ATLAS_WIDTH_PX);
            assert_eq!(info.height, ATLAS_HEIGHT_PX);
        }
    }

    #[test]
    fn alpha_is_premultiplied() {
        let out = temp_out();
        let manifest = generate_atlases(&out).expect("generate");
        for entry in &manifest.atlases {
            let bytes = fs::read(out.join(&entry.file)).expect("png");
            assert_premultiplied(&entry.file, &bytes).expect("premultiplied");
        }
        // Direct pixel contract on generator output.
        for atlas in 0..ATLAS_COUNT {
            for dir in 0..DIRECTION_COUNT {
                for frame in 0..FRAME_COUNT {
                    for ly in 0..SPRITE_SIZE_PX {
                        for lx in 0..SPRITE_SIZE_PX {
                            let [r, g, b, a] = sprite_pixel(atlas, dir, frame, lx, ly);
                            assert!(r <= a && g <= a && b <= a, "rgba=({r},{g},{b},{a})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn hashes_match_manifest() {
        let out = temp_out();
        let manifest = generate_atlases(&out).expect("generate");
        for entry in &manifest.atlases {
            let bytes = fs::read(out.join(&entry.file)).expect("png");
            let actual = sha256_hex(&bytes);
            assert_eq!(actual, entry.sha256, "hash drift {}", entry.file);
        }
        // Second generation must be byte-identical.
        let out2 = temp_out();
        let manifest2 = generate_atlases(&out2).expect("regen");
        assert_eq!(manifest, manifest2);
        for entry in &manifest.atlases {
            let a = fs::read(out.join(&entry.file)).expect("a");
            let b = fs::read(out2.join(&entry.file)).expect("b");
            assert_eq!(a, b, "png nondeterministic {}", entry.file);
        }
    }

    #[test]
    fn clean_regeneration_is_stable() {
        let out = temp_out();
        generate_atlases(&out).expect("first");
        check_atlases(&out).expect("check against self");
        // Distinct atlases / frames produce distinct content.
        let mut digests = BTreeSet::new();
        for id in 0..ATLAS_COUNT {
            digests.insert(sha256_hex(&encode_atlas_png(id).expect("enc")));
        }
        assert_eq!(digests.len(), ATLAS_COUNT as usize);
    }
}
