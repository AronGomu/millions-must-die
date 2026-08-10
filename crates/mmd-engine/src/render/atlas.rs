//! Load tracked zombie atlases (4 × 128×256 premul RGBA) and the phase-1
//! placeholder families (4 RTS sheets + 1 UI font sheet) that fill texture
//! slots 4..=8.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::RenderError;

/// Generated atlas manifest schema version.
const ATLAS_MANIFEST_VERSION: u32 = 2;
/// Phase-0 atlas count.
pub const ATLAS_COUNT: usize = 4;
/// Default display-quad edge px.
pub const SPRITE_SIZE_PX: u32 = 30;
/// Source-resolution edge px for one atlas frame.
pub const FRAME_SIZE_PX: u32 = 32;
/// Frames across X.
pub const FRAMES_X: u32 = 4;
/// Dirs down Y.
pub const FRAMES_Y: u32 = 8;
/// Atlas width px (4 frames × 32).
pub const ATLAS_WIDTH_PX: u32 = FRAMES_X * FRAME_SIZE_PX;
/// Atlas height px (8 dirs × 32).
pub const ATLAS_HEIGHT_PX: u32 = FRAMES_Y * FRAME_SIZE_PX;

// Texture slots 0..=3: the phase-0 zombie skins. Unchanged.
/// Slot of the RTS worker sheet.
pub const SLOT_RTS_WORKER: u32 = 4;
/// Slot of the RTS soldier sheet.
pub const SLOT_RTS_SOLDIER: u32 = 5;
/// Slot of the RTS building/resource-node table sheet.
pub const SLOT_RTS_BUILDINGS: u32 = 6;
/// Slot of the RTS prop sheet (selection ring, placement tiles, icons, panel).
pub const SLOT_RTS_PROPS: u32 = 7;
/// Slot of the UI bitmap font.
pub const SLOT_UI_FONT: u32 = 8;
/// Total bindable texture slots. `DrawGroup::atlas_id` must be below this.
pub const ATLAS_SLOT_COUNT: usize = 9;

/// The four RTS sheets, in slot order starting at [`SLOT_RTS_WORKER`].
pub const RTS_FILES: [&str; 4] = ["worker.png", "soldier.png", "buildings.png", "props.png"];
/// The UI family's one sheet.
const UI_FILES: [&str; 1] = ["font.png"];

/// Placeholder-family manifest schema version (written by `xtask atlases`).
const PLACEHOLDER_MANIFEST_VERSION: u32 = 1;
/// Glyph cell edge px in the UI font sheet.
const GLYPH_W_PX: u32 = 8;
const GLYPH_H_PX: u32 = 8;
/// Glyph grid of the UI font sheet: ASCII 32..=127 over 16 × 6 cells.
const FONT_COLS: u32 = 16;
const FONT_ROWS: u32 = 6;

#[derive(Debug, Deserialize)]
struct AtlasManifest {
    version: u32,
    atlases: Vec<AtlasManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct AtlasManifestEntry {
    id: u32,
    file: String,
    sha256: String,
}

/// Decoded atlas RGBA8 bytes + size.
#[derive(Clone, Debug)]
pub struct AtlasRgba {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl AtlasRgba {
    /// Sample one pixel (x,y).
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

/// UV rect for `(dir, frame)` in normalized atlas space.
pub fn frame_uv_rect(dir: u32, frame: u32) -> [f32; 4] {
    let u0 = frame as f32 / FRAMES_X as f32;
    let v0 = dir as f32 / FRAMES_Y as f32;
    let u1 = (frame + 1) as f32 / FRAMES_X as f32;
    let v1 = (dir + 1) as f32 / FRAMES_Y as f32;
    [u0, v0, u1, v1]
}

/// Default generated atlas dir from workspace root.
pub fn default_atlas_dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("assets/sprites/generated")
}

/// Directory of the RTS placeholder family, relative to the workspace root.
pub fn rts_atlas_dir(workspace_root: &Path) -> PathBuf {
    default_atlas_dir(workspace_root).join("rts")
}

/// Directory of the UI family, relative to the workspace root.
pub fn ui_atlas_dir(workspace_root: &Path) -> PathBuf {
    default_atlas_dir(workspace_root).join("ui")
}

/// Load all four tracked atlases from `dir`.
pub fn load_atlases(dir: &Path) -> Result<[AtlasRgba; ATLAS_COUNT], RenderError> {
    let manifest_path = dir.join("manifest.json");
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|e| RenderError::Io(format!("read {}: {e}", manifest_path.display())))?;
    let manifest: AtlasManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| RenderError::Atlas(format!("manifest parse: {e}")))?;
    if manifest.version != ATLAS_MANIFEST_VERSION || manifest.atlases.len() != ATLAS_COUNT {
        return Err(RenderError::Atlas(format!(
            "manifest layout: version={} atlas_count={}",
            manifest.version,
            manifest.atlases.len()
        )));
    }

    let mut out: Vec<AtlasRgba> = Vec::with_capacity(ATLAS_COUNT);
    for id in 0..ATLAS_COUNT as u32 {
        let expected_file = format!("atlas_{id}.png");
        let entry = manifest
            .atlases
            .get(id as usize)
            .ok_or_else(|| RenderError::Atlas(format!("manifest missing atlas {id}")))?;
        if entry.id != id || entry.file != expected_file {
            return Err(RenderError::Atlas(format!(
                "manifest atlas {id}: id={} file={}",
                entry.id, entry.file
            )));
        }
        let path = dir.join(&entry.file);
        let bytes = fs::read(&path)
            .map_err(|e| RenderError::Io(format!("read {}: {e}", path.display())))?;
        let actual_sha256 = sha256_hex(&bytes);
        if actual_sha256 != entry.sha256 {
            return Err(RenderError::Atlas(format!(
                "{} hash mismatch: expected {}, got {actual_sha256}",
                entry.file, entry.sha256
            )));
        }
        let (width, height, rgba) = decode_rgba8_png(&bytes)?;
        if width != ATLAS_WIDTH_PX || height != ATLAS_HEIGHT_PX {
            return Err(RenderError::Atlas(format!(
                "atlas_{id} size {width}x{height}, want {ATLAS_WIDTH_PX}x{ATLAS_HEIGHT_PX}"
            )));
        }
        if rgba.len() != (width * height * 4) as usize {
            return Err(RenderError::Atlas(format!(
                "atlas_{id} rgba len {}",
                rgba.len()
            )));
        }
        out.push(AtlasRgba {
            id,
            width,
            height,
            rgba,
        });
    }
    out.try_into()
        .map_err(|_| RenderError::Atlas("atlas count".into()))
}

/// One placeholder family's `manifest.json`, as written by
/// `xtask atlases` (`xtask/src/placeholder_art.rs`).
///
/// The `generator` string is deliberately not deserialised: the geometry block
/// below plus the per-file sha256 already pin every byte this loader consumes,
/// and an unread field would only be a lint to silence.
#[derive(Debug, Deserialize)]
struct PlaceholderManifest {
    version: u32,
    frame_width_px: u32,
    frame_height_px: u32,
    cols: u32,
    rows: u32,
    images: Vec<PlaceholderImage>,
}

#[derive(Debug, Deserialize)]
struct PlaceholderImage {
    id: u32,
    file: String,
    sha256: String,
}

/// Load one hash-verified placeholder family from `dir`.
///
/// Mirrors [`load_atlases`]: the manifest's geometry must match what the caller
/// declares, every image must appear in `files` order under its own index, and
/// each PNG's sha256 must match its manifest entry *before* the bytes are
/// decoded. Image dimensions are derived from the manifest geometry rather than
/// asserted separately, so a sheet whose grid and size disagree cannot load.
fn load_placeholder_family(
    dir: &Path,
    files: &[&str],
    frame_w: u32,
    frame_h: u32,
    cols: u32,
    rows: u32,
) -> Result<Vec<AtlasRgba>, RenderError> {
    let manifest_path = dir.join("manifest.json");
    let manifest_bytes = fs::read(&manifest_path)
        .map_err(|e| RenderError::Io(format!("read {}: {e}", manifest_path.display())))?;
    let manifest: PlaceholderManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| RenderError::Atlas(format!("manifest parse: {e}")))?;
    if manifest.version != PLACEHOLDER_MANIFEST_VERSION
        || manifest.frame_width_px != frame_w
        || manifest.frame_height_px != frame_h
        || manifest.cols != cols
        || manifest.rows != rows
        || manifest.images.len() != files.len()
    {
        return Err(RenderError::Atlas(format!(
            "manifest layout in {}: version={} frame={}x{} grid={}x{} images={}",
            dir.display(),
            manifest.version,
            manifest.frame_width_px,
            manifest.frame_height_px,
            manifest.cols,
            manifest.rows,
            manifest.images.len()
        )));
    }

    let want_width = cols * frame_w;
    let want_height = rows * frame_h;
    let mut out: Vec<AtlasRgba> = Vec::with_capacity(files.len());
    for (id, expected_file) in files.iter().enumerate() {
        let id = id as u32;
        let entry = &manifest.images[id as usize];
        if entry.id != id || entry.file != *expected_file {
            return Err(RenderError::Atlas(format!(
                "manifest image {id}: id={} file={}",
                entry.id, entry.file
            )));
        }
        let path = dir.join(&entry.file);
        let bytes = fs::read(&path)
            .map_err(|e| RenderError::Io(format!("read {}: {e}", path.display())))?;
        let actual_sha256 = sha256_hex(&bytes);
        if actual_sha256 != entry.sha256 {
            return Err(RenderError::Atlas(format!(
                "{} hash mismatch: expected {}, got {actual_sha256}",
                entry.file, entry.sha256
            )));
        }
        let (width, height, rgba) = decode_rgba8_png(&bytes)?;
        if width != want_width || height != want_height {
            return Err(RenderError::Atlas(format!(
                "{} size {width}x{height}, want {want_width}x{want_height}",
                entry.file
            )));
        }
        if rgba.len() != (width * height * 4) as usize {
            return Err(RenderError::Atlas(format!(
                "{} rgba len {}",
                entry.file,
                rgba.len()
            )));
        }
        out.push(AtlasRgba {
            id,
            width,
            height,
            rgba,
        });
    }
    Ok(out)
}

/// Load the four RTS sheets, hash-verified against `rts/manifest.json`.
pub fn load_rts_atlases(dir: &Path) -> Result<[AtlasRgba; 4], RenderError> {
    load_placeholder_family(
        dir,
        &RTS_FILES,
        FRAME_SIZE_PX,
        FRAME_SIZE_PX,
        FRAMES_X,
        FRAMES_Y,
    )?
    .try_into()
    .map_err(|_| RenderError::Atlas("rts atlas count".into()))
}

/// Load the UI font sheet, hash-verified against `ui/manifest.json`.
pub fn load_ui_font(dir: &Path) -> Result<AtlasRgba, RenderError> {
    let mut family =
        load_placeholder_family(dir, &UI_FILES, GLYPH_W_PX, GLYPH_H_PX, FONT_COLS, FONT_ROWS)?;
    family
        .pop()
        .ok_or_else(|| RenderError::Atlas("ui font count".into()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn decode_rgba8_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), RenderError> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| RenderError::Atlas(format!("png header: {e}")))?;
    let mut buf = vec![
        0u8;
        reader.output_buffer_size().ok_or_else(|| {
            RenderError::Atlas("png output buffer size overflow".into())
        })?
    ];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| RenderError::Atlas(format!("png frame: {e}")))?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err(RenderError::Atlas(format!(
            "png format {:?}/{:?}",
            info.color_type, info.bit_depth
        )));
    }
    buf.truncate(info.buffer_size());
    Ok((info.width, info.height, buf))
}
