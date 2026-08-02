//! Load tracked placeholder atlases (4 × 12×24 premul RGBA).

use std::fs;
use std::path::{Path, PathBuf};

use super::RenderError;

/// Phase-0 atlas count.
pub const ATLAS_COUNT: usize = 4;
/// Sprite edge px.
pub const SPRITE_SIZE_PX: u32 = 3;
/// Atlas width px (4 frames × 3).
pub const ATLAS_WIDTH_PX: u32 = 12;
/// Atlas height px (8 dirs × 3).
pub const ATLAS_HEIGHT_PX: u32 = 24;
/// Frames across X.
pub const FRAMES_X: u32 = 4;
/// Dirs down Y.
pub const FRAMES_Y: u32 = 8;

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

/// Load all four tracked atlases from `dir`.
pub fn load_atlases(dir: &Path) -> Result<[AtlasRgba; ATLAS_COUNT], RenderError> {
    let mut out: Vec<AtlasRgba> = Vec::with_capacity(ATLAS_COUNT);
    for id in 0..ATLAS_COUNT as u32 {
        let path = dir.join(format!("atlas_{id}.png"));
        let bytes = fs::read(&path)
            .map_err(|e| RenderError::Io(format!("read {}: {e}", path.display())))?;
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

/// CPU reference pixel matching xtask generator (`dir=0,frame=0` center = lx=1,ly=1).
pub fn expected_sprite_center_pixel(atlas_id: u32) -> [u8; 4] {
    sprite_pixel(atlas_id, 0, 0, 1, 1)
}

/// Mirror of xtask `sprite_pixel` for probe expectations.
pub fn sprite_pixel(atlas_id: u32, dir: u32, frame: u32, lx: u32, ly: u32) -> [u8; 4] {
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
