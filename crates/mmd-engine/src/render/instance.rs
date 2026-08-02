//! Compact sprite instance record shared with `shaders/sprite.hlsl`.

use std::mem::size_of;

/// GPU instance stride for one sprite (slot 1, per-instance).
///
/// Layout must match VS TEXCOORD2..5 packing:
/// `pos.xy`, `size.xy`, `uv_rect.xyzw`, `tint.rgba`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteInstance {
    /// Top-left pixel position.
    pub pos: [f32; 2],
    /// Pixel size (phase-0 sprites are 3×3).
    pub size: [f32; 2],
    /// Atlas UV rectangle `(u0, v0, u1, v1)`.
    pub uv_rect: [f32; 4],
    /// Premultiplied RGBA tint.
    pub tint: [f32; 4],
}

impl SpriteInstance {
    /// Byte size of one instance record.
    pub const STRIDE: u32 = size_of::<Self>() as u32;

    /// White opaque premul tint.
    pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    /// Build instance for a single atlas frame.
    pub fn new(pos: [f32; 2], size: [f32; 2], uv_rect: [f32; 4], tint: [f32; 4]) -> Self {
        Self {
            pos,
            size,
            uv_rect,
            tint,
        }
    }
}

/// Unit-quad vertex (slot 0, per-vertex). Matches VS TEXCOORD0..1.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadVertex {
    /// Corner in `[-0.5, 0.5]`.
    pub corner: [f32; 2],
    /// Unit UV in `[0, 1]`.
    pub uv: [f32; 2],
}

impl QuadVertex {
    pub const STRIDE: u32 = size_of::<Self>() as u32;
}

/// Frame view uniform (space1 b0).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameUniforms {
    pub view_size: [f32; 2],
    pub _pad: [f32; 2],
}

/// Canonical unit quad (two triangles via index buffer).
pub const QUAD_VERTICES: [QuadVertex; 4] = [
    QuadVertex {
        corner: [-0.5, -0.5],
        uv: [0.0, 0.0],
    },
    QuadVertex {
        corner: [0.5, -0.5],
        uv: [1.0, 0.0],
    },
    QuadVertex {
        corner: [0.5, 0.5],
        uv: [1.0, 1.0],
    },
    QuadVertex {
        corner: [-0.5, 0.5],
        uv: [0.0, 1.0],
    },
];

/// Triangle list indices for [`QUAD_VERTICES`].
pub const QUAD_INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of};

    #[test]
    fn instance_layout_is_stable() {
        assert_eq!(size_of::<SpriteInstance>(), 48);
        assert_eq!(align_of::<SpriteInstance>(), 4);
        assert_eq!(offset_of!(SpriteInstance, pos), 0);
        assert_eq!(offset_of!(SpriteInstance, size), 8);
        assert_eq!(offset_of!(SpriteInstance, uv_rect), 16);
        assert_eq!(offset_of!(SpriteInstance, tint), 32);
        assert_eq!(SpriteInstance::STRIDE, 48);

        assert_eq!(size_of::<QuadVertex>(), 16);
        assert_eq!(offset_of!(QuadVertex, corner), 0);
        assert_eq!(offset_of!(QuadVertex, uv), 8);

        assert_eq!(size_of::<FrameUniforms>(), 16);
    }
}
