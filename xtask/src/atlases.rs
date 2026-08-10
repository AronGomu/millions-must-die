//! Deterministic zombie sprite atlas generator.
//!
//! Layout (fixed): 8 direction rows × 4 animation frame columns.
//! Each frame is a 32×32 premultiplied-alpha sprite derived from pinned CC0 source art.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::digest::sha256_hex;

/// Manifest schema version.
pub const MANIFEST_VERSION: u32 = 2;

/// Atlas count locked by phase-0 contract.
pub const ATLAS_COUNT: u32 = 4;
/// Cardinal + diagonal directions.
pub const DIRECTION_COUNT: u32 = 8;
/// Animation frames per direction.
pub const FRAME_COUNT: u32 = 4;
/// Source-resolution edge length for each generated atlas frame.
pub const FRAME_SIZE_PX: u32 = 32;

/// Frames across atlas X axis (frame index).
pub const FRAMES_X: u32 = FRAME_COUNT;
/// Frames down atlas Y axis (direction index).
pub const FRAMES_Y: u32 = DIRECTION_COUNT;

/// Total frames packed per atlas.
pub const FRAMES_PER_ATLAS: u32 = DIRECTION_COUNT * FRAME_COUNT;

/// Atlas pixel width.
pub const ATLAS_WIDTH_PX: u32 = FRAMES_X * FRAME_SIZE_PX;
/// Atlas pixel height.
pub const ATLAS_HEIGHT_PX: u32 = FRAMES_Y * FRAME_SIZE_PX;

const GENERATOR_ID: &str = "mmd-zombie-cc0-v1";
const SOURCE_FILE: &str = "assets/sprites/source/stoner-games-zombie-strip12.png";
const SOURCE_SHA256: &str = "5207803a33b04bf45cfcb80308f340262128731b6cd51a5e569833ecdc7379f3";
const SOURCE_LICENSE: &str = "CC0-1.0";
const SOURCE_FRAME_SIZE_PX: u32 = 128;
const SOURCE_FRAME_COUNT: u32 = 12;
const SOURCE_CROP_X_PX: u32 = 24;
const SOURCE_CROP_WIDTH_PX: u32 = 80;
const SOURCE_WIDTH_PX: u32 = SOURCE_FRAME_SIZE_PX * SOURCE_FRAME_COUNT;
const SOURCE_HEIGHT_PX: u32 = SOURCE_FRAME_SIZE_PX;

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
    pub frame_size_px: u32,
    pub order: String,
}

/// Tracked atlas set manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasManifest {
    pub version: u32,
    pub generator: String,
    pub source_file: String,
    pub source_sha256: String,
    pub source_license: String,
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
        source_file: SOURCE_FILE.to_string(),
        source_sha256: SOURCE_SHA256.to_string(),
        source_license: SOURCE_LICENSE.to_string(),
        atlas_count: ATLAS_COUNT,
        direction_count: DIRECTION_COUNT,
        frame_count: FRAME_COUNT,
        frames_per_atlas: FRAMES_PER_ATLAS,
        layout: AtlasLayout {
            frames_x: FRAMES_X,
            frames_y: FRAMES_Y,
            frame_size_px: FRAME_SIZE_PX,
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
    if manifest.generator != GENERATOR_ID
        || manifest.source_file != SOURCE_FILE
        || manifest.source_sha256 != SOURCE_SHA256
        || manifest.source_license != SOURCE_LICENSE
    {
        return Err(AtlasError::Layout(
            "generator source metadata drifted".into(),
        ));
    }
    if manifest.layout.frames_x != FRAMES_X
        || manifest.layout.frames_y != FRAMES_Y
        || manifest.layout.frame_size_px != FRAME_SIZE_PX
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

#[derive(Debug)]
struct SourceSheet {
    rgba: Vec<u8>,
}

fn source_path() -> PathBuf {
    workspace_root_from_xtask_manifest().join(SOURCE_FILE)
}

fn load_source_sheet() -> Result<SourceSheet, AtlasError> {
    let path = source_path();
    let png_bytes = fs::read(&path).map_err(|e| AtlasError::Io(e.to_string()))?;
    let actual = sha256_hex(&png_bytes);
    if actual != SOURCE_SHA256 {
        return Err(AtlasError::HashMismatch {
            file: SOURCE_FILE.to_string(),
            expected: SOURCE_SHA256.to_string(),
            actual,
        });
    }

    let decoder = png::Decoder::new(Cursor::new(png_bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| AtlasError::Decode(e.to_string()))?;
    let mut rgba = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader
        .next_frame(&mut rgba)
        .map_err(|e| AtlasError::Decode(e.to_string()))?;
    if info.width != SOURCE_WIDTH_PX
        || info.height != SOURCE_HEIGHT_PX
        || info.color_type != png::ColorType::Rgba
        || info.bit_depth != png::BitDepth::Eight
    {
        return Err(AtlasError::Decode(format!(
            "{SOURCE_FILE}: expected {SOURCE_WIDTH_PX}x{SOURCE_HEIGHT_PX} RGBA8, got {}x{} {:?} {:?}",
            info.width, info.height, info.color_type, info.bit_depth
        )));
    }
    rgba.truncate(info.buffer_size());
    Ok(SourceSheet { rgba })
}

/// Encode one atlas PNG (deterministic bytes).
pub fn encode_atlas_png(atlas_id: u32) -> Result<Vec<u8>, AtlasError> {
    if atlas_id >= ATLAS_COUNT {
        return Err(AtlasError::Layout(format!(
            "atlas id {atlas_id} >= {ATLAS_COUNT}"
        )));
    }
    let source = load_source_sheet()?;
    let pixels = render_atlas_rgba(atlas_id, &source);
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

/// Fill RGBA buffer for one atlas from pinned source art.
fn render_atlas_rgba(atlas_id: u32, source: &SourceSheet) -> Vec<u8> {
    let mut pixels = vec![0u8; (ATLAS_WIDTH_PX * ATLAS_HEIGHT_PX * 4) as usize];
    for dir in 0..DIRECTION_COUNT {
        for frame in 0..FRAME_COUNT {
            let origin_x = frame * FRAME_SIZE_PX;
            let origin_y = dir * FRAME_SIZE_PX;
            let source_frame = (frame * 3 + atlas_id * 3) % SOURCE_FRAME_COUNT;
            let flip_x = matches!(dir, 3..=5);
            for ly in 0..FRAME_SIZE_PX {
                for lx in 0..FRAME_SIZE_PX {
                    let sampled_x = if flip_x { FRAME_SIZE_PX - 1 - lx } else { lx };
                    let source_x = source_frame * SOURCE_FRAME_SIZE_PX
                        + SOURCE_CROP_X_PX
                        + sampled_x * SOURCE_CROP_WIDTH_PX / FRAME_SIZE_PX;
                    let source_y = ly * SOURCE_FRAME_SIZE_PX / FRAME_SIZE_PX;
                    let source_idx = ((source_y * SOURCE_WIDTH_PX + source_x) * 4) as usize;
                    let a = source.rgba[source_idx + 3];
                    let rgba = [
                        premultiply(source.rgba[source_idx], a),
                        premultiply(source.rgba[source_idx + 1], a),
                        premultiply(source.rgba[source_idx + 2], a),
                        a,
                    ];
                    let x = origin_x + lx;
                    let y = origin_y + ly;
                    let idx = ((y * ATLAS_WIDTH_PX + x) * 4) as usize;
                    pixels[idx..idx + 4].copy_from_slice(&rgba);
                }
            }
        }
    }
    pixels
}

fn premultiply(channel: u8, alpha: u8) -> u8 {
    ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8
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

/// CLI entry: write or check generated atlases (zombie + rts + ui families).
pub fn run_atlases(check: bool) -> Result<(), AtlasError> {
    let root = workspace_root_from_xtask_manifest();
    let out = default_output_dir(&root);
    let rts_out = crate::placeholder_art::rts_dir(&root);
    let ui_out = crate::placeholder_art::ui_dir(&root);
    if check {
        check_atlases(&out)?;
        crate::placeholder_art::check_placeholders(&rts_out, &ui_out)?;
        println!(
            "atlases: ok ({ATLAS_COUNT} zombie png + manifest, {} rts png + manifest, {} ui png + manifest)",
            crate::placeholder_art::RTS_FILES.len(),
            crate::placeholder_art::UI_FILES.len()
        );
    } else {
        let manifest = generate_atlases(&out)?;
        let (rts, ui) = crate::placeholder_art::generate_placeholders(&rts_out, &ui_out)?;
        println!(
            "atlases: wrote {} zombie + {} rts + {} ui png + manifests → {}",
            manifest.atlases.len(),
            rts.images.len(),
            ui.images.len(),
            out.parent().unwrap_or(&out).display()
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
        assert_eq!(manifest.layout.frame_size_px, 32);
        assert_eq!(ATLAS_WIDTH_PX, 128);
        assert_eq!(ATLAS_HEIGHT_PX, 256);

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
    fn source_is_pinned_rgba_strip() {
        let source = load_source_sheet().expect("source");
        assert_eq!(
            source.rgba.len(),
            (SOURCE_WIDTH_PX * SOURCE_HEIGHT_PX * 4) as usize
        );
        let bytes = fs::read(source_path()).expect("source png");
        assert_eq!(sha256_hex(&bytes), SOURCE_SHA256);
    }

    fn rgba_at(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    fn premultiplied_source_at(source: &SourceSheet, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * SOURCE_WIDTH_PX + x) * 4) as usize;
        let a = source.rgba[i + 3];
        [
            premultiply(source.rgba[i], a),
            premultiply(source.rgba[i + 1], a),
            premultiply(source.rgba[i + 2], a),
            a,
        ]
    }

    #[test]
    fn source_crop_phase_and_direction_mapping_are_locked() {
        let source = load_source_sheet().expect("source");
        let atlas_0 = render_atlas_rgba(0, &source);
        let atlas_1 = render_atlas_rgba(1, &source);
        let atlas_3 = render_atlas_rgba(3, &source);

        // Atlas 0, frame 0, local (16,16) maps to source frame 0 at (64,64).
        assert_eq!(
            rgba_at(&atlas_0, ATLAS_WIDTH_PX, 16, 16),
            premultiplied_source_at(&source, 64, 64)
        );
        // Atlas phase 1 starts at source frame 3.
        assert_eq!(
            rgba_at(&atlas_1, ATLAS_WIDTH_PX, 16, 16),
            premultiplied_source_at(&source, 3 * SOURCE_FRAME_SIZE_PX + 64, 64)
        );
        // Every west-facing row mirrors every frame; all other rows preserve orientation.
        for frame in 0..FRAME_COUNT {
            let frame_x = frame * FRAME_SIZE_PX;
            let east_is_asymmetric = (0..FRAME_SIZE_PX).any(|ly| {
                (0..FRAME_SIZE_PX).any(|lx| {
                    rgba_at(&atlas_0, ATLAS_WIDTH_PX, frame_x + lx, ly)
                        != rgba_at(
                            &atlas_0,
                            ATLAS_WIDTH_PX,
                            frame_x + FRAME_SIZE_PX - 1 - lx,
                            ly,
                        )
                })
            });
            assert!(
                east_is_asymmetric,
                "frame {frame} must detect orientation drift"
            );
            for dir in [3, 4, 5] {
                for ly in 0..FRAME_SIZE_PX {
                    for lx in 0..FRAME_SIZE_PX {
                        assert_eq!(
                            rgba_at(
                                &atlas_0,
                                ATLAS_WIDTH_PX,
                                frame_x + lx,
                                dir * FRAME_SIZE_PX + ly
                            ),
                            rgba_at(
                                &atlas_0,
                                ATLAS_WIDTH_PX,
                                frame_x + FRAME_SIZE_PX - 1 - lx,
                                ly
                            ),
                            "west dir={dir} frame={frame} local=({lx},{ly})"
                        );
                    }
                }
            }
            for dir in [1, 2, 6, 7] {
                for ly in 0..FRAME_SIZE_PX {
                    for lx in 0..FRAME_SIZE_PX {
                        assert_eq!(
                            rgba_at(
                                &atlas_0,
                                ATLAS_WIDTH_PX,
                                frame_x + lx,
                                dir * FRAME_SIZE_PX + ly
                            ),
                            rgba_at(&atlas_0, ATLAS_WIDTH_PX, frame_x + lx, ly),
                            "non-west dir={dir} frame={frame} local=({lx},{ly})"
                        );
                    }
                }
            }
        }
        // Last generated texel stays inside cropped source frame 6 for atlas 3/frame 3.
        assert_eq!(
            rgba_at(
                &atlas_3,
                ATLAS_WIDTH_PX,
                3 * FRAME_SIZE_PX + 31,
                7 * FRAME_SIZE_PX + 31
            ),
            premultiplied_source_at(
                &source,
                6 * SOURCE_FRAME_SIZE_PX + SOURCE_CROP_X_PX + 77,
                124
            )
        );
    }

    #[test]
    fn alpha_is_premultiplied() {
        let out = temp_out();
        let manifest = generate_atlases(&out).expect("generate");
        for entry in &manifest.atlases {
            let bytes = fs::read(out.join(&entry.file)).expect("png");
            assert_premultiplied(&entry.file, &bytes).expect("premultiplied");
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
