//! Aspect-fit logical canvas: exact 16:9 content rect inside any drawable,
//! and the pointer inverse that matches it exactly.
//!
//! World/UI stay authored at the fixed [`super::VIEW_WIDTH`] ×
//! [`super::VIEW_HEIGHT`] logical canvas regardless of window shape. The
//! destination on screen is the largest centred rectangle that is an exact
//! `16k × 9k` multiple — never a stretch, never a crop — and the live mouse
//! path inverts precisely that same rectangle so a click lands on the pixel
//! it looks like it lands on.

use super::renderer::{VIEW_HEIGHT, VIEW_WIDTH};

/// An axis-aligned pixel rectangle, non-negative extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectU32 {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A window's current shape: its logical size (SDL "window units", the space
/// mouse events arrive in), the drawable's actual pixel size (may differ
/// under HiDPI), and the exact-16:9 content rect inside the drawable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayViewport {
    pub window_units: [u32; 2],
    pub drawable_px: [u32; 2],
    pub content_px: RectU32,
}

/// A window-unit pointer mapped into the fixed logical canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MappedPointer {
    /// Clamped to `[0, VIEW_WIDTH] x [0, VIEW_HEIGHT]`.
    pub logical: [f32; 2],
    /// Whether the *unclamped* drawable point fell inside `content_px`
    /// (half-open bounds) before clamping. `false` for a point in the bars.
    pub inside_content: bool,
}

/// Largest centred rectangle inside `drawable_px` that is an exact `16k × 9k`
/// multiple, `k` an integer floor of `min(w/16, h/9)`. `None` when the
/// drawable cannot fit even `k = 1` (16×9).
pub fn aspect_fit_16_9(drawable_px: [u32; 2]) -> Option<RectU32> {
    let k = (drawable_px[0] / 16).min(drawable_px[1] / 9);
    if k == 0 {
        return None;
    }
    let w = 16 * k;
    let h = 9 * k;
    Some(RectU32 {
        x: (drawable_px[0] - w) / 2,
        y: (drawable_px[1] - h) / 2,
        w,
        h,
    })
}

impl DisplayViewport {
    /// Build from a window's logical size and its drawable's pixel size.
    /// `None` when the drawable cannot fit an exact 16:9 rect at all
    /// (mirrors [`aspect_fit_16_9`]).
    pub fn new(window_units: [u32; 2], drawable_px: [u32; 2]) -> Option<Self> {
        let content_px = aspect_fit_16_9(drawable_px)?;
        Some(Self {
            window_units,
            drawable_px,
            content_px,
        })
    }

    /// Map a window-unit pointer (the space SDL mouse events arrive in) to
    /// the fixed logical canvas.
    ///
    /// Window units to drawable pixels uses a per-axis ratio (HiDPI: ratio
    /// greater than 1); the drawable point is tested against `content_px`
    /// half-open, then clamped closed into `content_px`, then rescaled
    /// linearly onto the fixed logical canvas. This is the exact inverse of
    /// the render transform: `present_blit` scales the fixed logical
    /// offscreen into `content_px` with the same linear map, run forward.
    pub fn map_pointer(self, window_point: [f32; 2]) -> MappedPointer {
        let ratio_x = if self.window_units[0] > 0 {
            self.drawable_px[0] as f32 / self.window_units[0] as f32
        } else {
            1.0
        };
        let ratio_y = if self.window_units[1] > 0 {
            self.drawable_px[1] as f32 / self.window_units[1] as f32
        } else {
            1.0
        };
        let drawable = [window_point[0] * ratio_x, window_point[1] * ratio_y];

        let content = self.content_px;
        let x1 = content.x as f32;
        let y1 = content.y as f32;
        let x2 = (content.x + content.w) as f32;
        let y2 = (content.y + content.h) as f32;
        let inside_content =
            drawable[0] >= x1 && drawable[0] < x2 && drawable[1] >= y1 && drawable[1] < y2;

        let clamped_x = drawable[0].clamp(x1, x2);
        let clamped_y = drawable[1].clamp(y1, y2);

        let logical = if content.w > 0 && content.h > 0 {
            [
                (clamped_x - x1) / content.w as f32 * VIEW_WIDTH as f32,
                (clamped_y - y1) / content.h as f32 * VIEW_HEIGHT as f32,
            ]
        } else {
            [0.0, 0.0]
        };

        MappedPointer {
            logical,
            inside_content,
        }
    }
}
