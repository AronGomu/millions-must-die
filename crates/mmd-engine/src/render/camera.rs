//! Clamped panning camera over the isometric projection.

use crate::render::instance::IsoView;
use crate::scenario::Cell;

/// Pan speed in cells per second, for both keyboard and edge pan.
///
/// 24 cells/s is three times the agent walk speed
/// (`sim::SPEED_CELLS_PER_SEC = 8.0`), so a player can outrun the unit they
/// just ordered — the property that makes a camera feel responsive rather than
/// the number itself.
pub const CAMERA_PAN_CELLS_PER_SEC: f32 = 24.0;

/// Distance from a view border, in screen pixels, inside which the pointer pans.
pub const EDGE_PAN_MARGIN_PX: f32 = 12.0;

/// Cell-space camera centre plus the projection it produces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// The cell-space point the view centre looks at.
    center: [f32; 2],
    /// Grid width in cells — the clamp bound.
    width: u32,
    /// Grid height in cells — the clamp bound.
    height: u32,
    /// Projection with everything but `origin`/`depth_bias` fixed for the scene.
    view: IsoView,
}

impl Camera {
    /// Build from a scene's geometry, starting centred on `start`.
    ///
    /// `start` is clamped like every later pan, so a scenario naming a centre
    /// off its own grid cannot open the game looking at nothing.
    pub fn new(
        width: u32,
        height: u32,
        cell_size_px: f32,
        view_size: [f32; 2],
        start: [f32; 2],
    ) -> Self {
        let view = IsoView::new(width, height, Cell { x: 0, y: 0 }, cell_size_px, view_size);
        let mut camera = Self {
            center: [0.0, 0.0],
            width,
            height,
            view,
        };
        camera.pan_cells(start[0], start[1]);
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

    /// Move the centre by a cell-space delta and clamp.
    ///
    /// Clamp is `0.0 ..= width as f32` and `0.0 ..= height as f32` — the closed
    /// cell-space rectangle of the grid, so the centre may sit exactly on the
    /// far edge and never outside it. A non-finite delta is ignored outright
    /// rather than poisoning the centre.
    pub fn pan_cells(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        self.center = [
            (self.center[0] + dx).clamp(0.0, self.width as f32),
            (self.center[1] + dy).clamp(0.0, self.height as f32),
        ];
    }

    /// Apply one tick of panning from a direction vector.
    ///
    /// `dir` components are expected in `-1.0..=1.0`; the step is
    /// `dir * CAMERA_PAN_CELLS_PER_SEC * dt`. A diagonal is **not** normalised:
    /// holding two keys pans faster on the diagonal, which is what every RTS
    /// this one is modelled on does.
    pub fn pan_tick(&mut self, dir: [f32; 2], dt: f32) {
        let step = CAMERA_PAN_CELLS_PER_SEC * dt;
        self.pan_cells(dir[0] * step, dir[1] * step);
    }

    /// Centre the camera on a cell (clamped). Used by "jump to base".
    pub fn look_at_cell(&mut self, cell: Cell) {
        let target = [cell.x as f32 + 0.5, cell.y as f32 + 0.5];
        self.center = [
            target[0].clamp(0.0, self.width as f32),
            target[1].clamp(0.0, self.height as f32),
        ];
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

/// A screen-space pan direction converted to the cell-space delta that moves
/// the view that way. Pure inverse of the projection's linear part.
pub fn screen_dir_to_cells(dir: [f32; 2], tile_w: f32, tile_h: f32) -> [f32; 2] {
    let u = if tile_w != 0.0 {
        dir[0] / (tile_w * 0.5)
    } else {
        0.0
    };
    let v = if tile_h != 0.0 {
        dir[1] / (tile_h * 0.5)
    } else {
        0.0
    };
    [(u + v) * 0.5, (v - u) * 0.5]
}
