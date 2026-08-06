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
    /// Display-quad size in pixels (independent from atlas frame resolution).
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

/// CPU mirror of the vertex stage's world→clip transform in
/// `shaders/sprite.hlsl` (`VSMain`).
///
/// Pixel space is y-down with the origin at the view's top-left; clip space is
/// y-up over `[-1, 1]`. Given a unit-quad `corner` in `[-0.5, 0.5]` and one
/// instance's `pos`/`size`, this returns the `SV_Position` the shader emits.
///
/// This mirror exists so a headless test can state the expected clip
/// coordinate for a known world corner. It is *not* self-validating: the
/// binding back to the real shader is the GPU raster probe in
/// `tests/render_correctness.rs`, which renders one sprite and requires its
/// footprint to land exactly where this function predicts.
///
/// A zero component in `view_size` has no meaningful projection and yields a
/// non-finite coordinate; callers pass the live [`FrameUniforms::view_size`],
/// which the renderer fixes at the offscreen resolution.
pub fn world_to_clip(
    pos: [f32; 2],
    size: [f32; 2],
    corner: [f32; 2],
    view_size: [f32; 2],
) -> [f32; 4] {
    let world = [
        pos[0] + (corner[0] + 0.5) * size[0],
        pos[1] + (corner[1] + 0.5) * size[1],
    ];
    let ndc_x = (world[0] / view_size[0]) * 2.0 - 1.0;
    let ndc_y = (world[1] / view_size[1]) * 2.0 - 1.0;
    [ndc_x, -ndc_y, 0.0, 1.0]
}

/// Fixed-function viewport map the GPU applies after the vertex stage:
/// clip `xy` → pixel `xy` in the y-down render target.
///
/// Paired with [`world_to_clip`] this round-trips to the original world pixel,
/// which is exactly the claim the GPU raster probe checks.
pub fn clip_to_pixel(clip: [f32; 4], view_size: [f32; 2]) -> [f32; 2] {
    [
        (clip[0] * 0.5 + 0.5) * view_size[0],
        (0.5 - clip[1] * 0.5) * view_size[1],
    ]
}

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
