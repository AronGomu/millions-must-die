//! Clamped panning camera over the isometric projection, bounded by a
//! projected map frontier — distinct from the scenario's own cell extent.

use crate::render::instance::{IsoView, iso_project, iso_unproject};
use crate::scenario::Cell;

/// Distance from a view border, in screen pixels, inside which the pointer pans.
pub const EDGE_PAN_MARGIN_PX: f32 = 12.0;

/// One tick's pan intent: two independent screen-space directions, each with
/// its own speed.
///
/// Keyboard and pointer-edge panning stay apart rather than merging into one
/// direction: a player can hold an arrow key *and* park the pointer on an
/// edge at once, and [`crate::rts_settings`]'s `CameraSettings` persists a
/// speed for each source separately.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraPanIntent {
    /// Held arrow-key direction, components in `-1.0..=1.0`.
    pub keyboard_dir: [f32; 2],
    /// Pointer-edge direction, components in `-1.0..=1.0`.
    pub edge_dir: [f32; 2],
    /// Keyboard pan speed, in cells/second.
    pub keyboard_speed: f32,
    /// Edge pan speed, in cells/second.
    pub edge_speed: f32,
}

/// Projected-space bounds the camera *centre* is clamped into.
///
/// Distinct from the scenario's cell extent: the playable map is a grid of
/// cells, but the camera area is that grid's projected diamond, shrunk by
/// half the logical view so the screen never shows past the map's edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFrontier {
    /// Legal range of the candidate centre's projected `x`, origin `[0, 0]`.
    pub x: [f32; 2],
    /// Legal range of the candidate centre's projected `y`, origin `[0, 0]`.
    pub y: [f32; 2],
}

impl CameraFrontier {
    /// Derive from a `width` x `height` cell grid's projected diamond and a
    /// logical `view_size`.
    ///
    /// The diamond's own bounds, projected with origin `[0, 0]`:
    /// `x = [-height * tile_w/2, width * tile_w/2]`,
    /// `y = [0, (width + height) * tile_h/2]`. Each axis is then inset by
    /// half the corresponding `view_size` component, since a centre nearer
    /// the diamond's edge than half a screen would draw dead space beyond
    /// it. An axis narrower than the view it would need to fill (a map
    /// smaller than one screen along that axis) has no legal inset range;
    /// [`shrink`] collapses it to the un-inset diamond's midpoint instead of
    /// an inverted `[min, max]` that would reject every candidate.
    pub fn new(width: u32, height: u32, tile_w: f32, tile_h: f32, view_size: [f32; 2]) -> Self {
        let (w, h) = (width as f32, height as f32);
        let aabb_x = [-h * tile_w * 0.5, w * tile_w * 0.5];
        let aabb_y = [0.0, (w + h) * tile_h * 0.5];
        Self {
            x: shrink(aabb_x, view_size[0] * 0.5),
            y: shrink(aabb_y, view_size[1] * 0.5),
        }
    }
}

/// `aabb` inset by `inset` on both ends, or the AABB's own midpoint —
/// repeated — when the inset would invert the range.
fn shrink(aabb: [f32; 2], inset: f32) -> [f32; 2] {
    let (min, max) = (aabb[0] + inset, aabb[1] - inset);
    if min > max {
        let mid = (aabb[0] + aabb[1]) * 0.5;
        [mid, mid]
    } else {
        [min, max]
    }
}

/// Cell-space camera centre plus the projection it produces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The cell-space point the view centre looks at.
    center: [f32; 2],
    /// The projected-space clamp — [`CameraFrontier`].
    frontier: CameraFrontier,
    /// Projection with everything but `origin`/`depth_bias` fixed for the scene.
    view: IsoView,
}

impl Camera {
    /// Build from a scene's geometry, starting centred on `start`.
    ///
    /// `start` is clamped like every later pan, so a scenario naming a centre
    /// off its own frontier cannot open the game looking at dead space.
    pub fn new(
        width: u32,
        height: u32,
        cell_size_px: f32,
        view_size: [f32; 2],
        start: [f32; 2],
    ) -> Self {
        let view = IsoView::new(width, height, Cell { x: 0, y: 0 }, cell_size_px, view_size);
        let frontier = CameraFrontier::new(width, height, view.tile_w, view.tile_h, view_size);
        let mut camera = Self {
            center: [0.0, 0.0],
            frontier,
            view,
        };
        camera.look_at_point(start);
        camera
    }

    /// The current projection. Packers and the renderer's depth uniform read this.
    pub fn iso_view(&self) -> IsoView {
        self.view.with_center_cell(self.center)
    }

    /// The cell-space centre.
    pub fn center(&self) -> [f32; 2] {
        self.center
    }

    /// The projected-space clamp this camera's centre is bounded by.
    pub fn frontier(&self) -> CameraFrontier {
        self.frontier
    }

    /// Project `candidate` with origin `[0, 0]`, clamp into the frontier,
    /// unproject back to a cell-space point.
    fn clamp_to_frontier(&self, candidate: [f32; 2]) -> [f32; 2] {
        let (tw, th) = (self.view.tile_w, self.view.tile_h);
        let p = iso_project(candidate[0], candidate[1], tw, th, [0.0, 0.0]);
        let cx = p[0].clamp(self.frontier.x[0], self.frontier.x[1]);
        let cy = p[1].clamp(self.frontier.y[0], self.frontier.y[1]);
        iso_unproject(cx, cy, tw, th, [0.0, 0.0])
    }

    /// Move the centre by a cell-space delta and clamp to the frontier.
    ///
    /// A non-finite delta is ignored outright rather than poisoning the
    /// centre.
    pub fn pan_cells(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        self.center = self.clamp_to_frontier([self.center[0] + dx, self.center[1] + dy]);
    }

    /// Centre the camera on a fractional map point (clamped to the
    /// frontier). Minimap-ready: the target need not be a cell centre.
    ///
    /// A non-finite point is ignored outright, matching [`Self::pan_cells`].
    pub fn look_at_point(&mut self, point: [f32; 2]) {
        if !point[0].is_finite() || !point[1].is_finite() {
            return;
        }
        self.center = self.clamp_to_frontier(point);
    }

    /// Centre the camera on a cell (clamped). Used by "jump to base".
    pub fn look_at_cell(&mut self, cell: Cell) {
        self.look_at_point([cell.x as f32 + 0.5, cell.y as f32 + 0.5]);
    }
}

/// Edge-pan direction for a pointer at `mouse` inside a `view` rect.
///
/// Returns one of `-1.0`, `0.0`, `1.0` per axis. `+x` means the world scrolls
/// so the camera centre moves toward larger `cell.x - cell.y` (screen right);
/// `+y` toward the bottom of the screen. A pointer outside the view produces
/// `[0.0, 0.0]` — a window that lost focus must not scroll forever.
pub fn edge_pan_dir(mouse: [f32; 2], view: [f32; 2]) -> [f32; 2] {
    if !mouse[0].is_finite()
        || !mouse[1].is_finite()
        || mouse[0] < 0.0
        || mouse[1] < 0.0
        || mouse[0] > view[0]
        || mouse[1] > view[1]
    {
        return [0.0, 0.0];
    }
    let x = if mouse[0] <= EDGE_PAN_MARGIN_PX {
        -1.0
    } else if mouse[0] >= view[0] - EDGE_PAN_MARGIN_PX {
        1.0
    } else {
        0.0
    };
    let y = if mouse[1] <= EDGE_PAN_MARGIN_PX {
        -1.0
    } else if mouse[1] >= view[1] - EDGE_PAN_MARGIN_PX {
        1.0
    } else {
        0.0
    };
    [x, y]
}

/// A screen-space cardinal axis pair converted to the cell-space cardinal
/// basis it maps to: `[sx + sy, sy - sx]`.
///
/// Tile-geometry independent by design — unlike the projection itself, a pan
/// *intent* is not a pixel offset, so keyboard/edge panning reads the same in
/// cell space regardless of the scene's tile aspect. A caller scales the
/// result by a speed and `dt` to get an actual step.
pub fn screen_axes_to_cells(dir: [f32; 2]) -> [f32; 2] {
    [dir[0] + dir[1], dir[1] - dir[0]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_collapses_an_inverted_range_to_the_midpoint() {
        assert_eq!(shrink([-10.0, 10.0], 20.0), [0.0, 0.0]);
        assert_eq!(shrink([-10.0, 30.0], 20.0), [10.0, 10.0]);
    }

    #[test]
    fn shrink_keeps_a_legal_range() {
        assert_eq!(shrink([-10.0, 10.0], 2.0), [-8.0, 8.0]);
    }
}
