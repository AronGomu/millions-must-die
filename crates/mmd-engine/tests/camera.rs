//! T4 — panning camera + screen<->cell unprojection.
//!
//! Pure math, no GPU: every case here is a headless CPU check of
//! `iso_unproject`, `IsoView::{unproject, cell_at, with_center_cell}` and the
//! new `render::camera` module.

use mmd_engine::render::{
    CAMERA_PAN_CELLS_PER_SEC, Camera, EDGE_PAN_MARGIN_PX, IsoView, edge_pan_dir, iso_project,
    iso_unproject, screen_dir_to_cells,
};
use mmd_engine::scenario::Cell;

const VIEW: [f32; 2] = [1920.0, 1080.0];

#[test]
fn unproject_inverts_project_exactly() {
    let view = IsoView::new(320, 320, Cell { x: 160, y: 160 }, 4.0, VIEW);
    let mut count = 0;
    for i in 0..20 {
        for j in 0..20 {
            let (cx, cy) = (i as f32 * 16.0, j as f32 * 16.0);
            let s = iso_project(cx, cy, view.tile_w, view.tile_h, view.origin);
            let back = iso_unproject(s[0], s[1], view.tile_w, view.tile_h, view.origin);
            assert!(
                (back[0] - cx).abs() < 1e-3 && (back[1] - cy).abs() < 1e-3,
                "cell ({cx},{cy}) round-tripped to {back:?}"
            );
            count += 1;
        }
    }
    assert_eq!(count, 400);
}

#[test]
fn project_inverts_unproject_exactly() {
    let view = IsoView::new(320, 320, Cell { x: 160, y: 160 }, 4.0, VIEW);
    let mut count = 0;
    for i in 0..20 {
        for j in 0..20 {
            let (sx, sy) = (i as f32 * 96.0, j as f32 * 54.0);
            let c = iso_unproject(sx, sy, view.tile_w, view.tile_h, view.origin);
            let back = iso_project(c[0], c[1], view.tile_w, view.tile_h, view.origin);
            assert!(
                (back[0] - sx).abs() < 1e-3 && (back[1] - sy).abs() < 1e-3,
                "screen ({sx},{sy}) round-tripped to {back:?}"
            );
            count += 1;
        }
    }
    assert_eq!(count, 400);
}

#[test]
fn unproject_of_a_degenerate_tile_is_not_finite() {
    let c = iso_unproject(10.0, 10.0, 0.0, 0.0, [0.0, 0.0]);
    assert!(c[0].is_nan(), "x component: {c:?}");
    assert!(c[1].is_nan(), "y component: {c:?}");
}

#[test]
fn cell_at_returns_the_cell_a_sprite_was_packed_from() {
    let view = IsoView::new(320, 320, Cell { x: 160, y: 160 }, 4.0, VIEW);
    for cell in [
        Cell { x: 0, y: 0 },
        Cell { x: 1, y: 0 },
        Cell { x: 0, y: 1 },
        Cell { x: 159, y: 160 },
        Cell { x: 319, y: 319 },
    ] {
        let centre = view.project(cell.x as f32 + 0.5, cell.y as f32 + 0.5);
        let got = view.cell_at(centre[0], centre[1], 320, 320);
        assert_eq!(got, Some(cell), "cell {cell:?} projected to {centre:?}");
    }
}

#[test]
fn cell_at_rejects_offscreen_and_off_grid() {
    let view = IsoView::new(320, 320, Cell { x: 160, y: 160 }, 4.0, VIEW);

    // Far above the diamond's top vertex: both unprojected components go
    // deeply negative.
    let top = view.project(0.0, 0.0);
    assert_eq!(view.cell_at(top[0], top[1] - 10_000.0, 320, 320), None);

    // Directly left of the origin far enough that the unprojected `x` alone
    // goes negative.
    assert_eq!(
        view.cell_at(view.origin[0] - 10_000.0, view.origin[1], 320, 320),
        None
    );
}

#[test]
fn iso_view_new_is_unchanged() {
    let iso = IsoView::new(480, 270, Cell { x: 240, y: 135 }, 4.0, [1920.0, 1080.0]);
    assert_eq!(iso.tile_w, 8.0);
    assert_eq!(iso.tile_h, 4.0);
    assert_eq!(iso.origin, [540.0, -212.0]);
    assert_eq!(iso.map_height_px, 1500.0);
    assert!((iso.depth_scale - 1.0 / 1500.0).abs() < 1e-9);
    assert!((iso.depth_bias - 212.0 / 1500.0).abs() < 1e-9);
    assert_eq!(iso.view_size, [1920.0, 1080.0]);
}

#[test]
fn with_center_cell_matches_new_for_the_destination_centre() {
    let dest = Cell { x: 240, y: 135 };
    let direct = IsoView::new(480, 270, dest, 4.0, VIEW);
    let via_center = IsoView::new(480, 270, dest, 4.0, VIEW)
        .with_center_cell([dest.x as f32 + 0.5, dest.y as f32 + 0.5]);
    assert!((direct.origin[0] - via_center.origin[0]).abs() < 1e-6);
    assert!((direct.origin[1] - via_center.origin[1]).abs() < 1e-6);
    assert!((direct.depth_bias - via_center.depth_bias).abs() < 1e-6);
}

#[test]
fn depth_key_is_camera_independent() {
    let base = IsoView::new(480, 270, Cell { x: 240, y: 135 }, 4.0, VIEW);
    let views = [
        base.with_center_cell([0.5, 0.5]),
        base.with_center_cell([300.0, 50.0]),
        base.with_center_cell([100.0, 200.0]),
    ];
    let (cx, cy) = (50.0f32, 60.0f32);
    let keys: Vec<f32> = views
        .iter()
        .map(|v| v.depth(v.project(cx, cy)[1]))
        .collect();
    for k in &keys[1..] {
        assert!(
            (k - keys[0]).abs() < 1e-6,
            "depth keys diverge across cameras: {keys:?}"
        );
    }
}

#[test]
fn panning_moves_the_origin_the_other_way() {
    let mut camera = Camera::new(480, 270, 4.0, VIEW, [240.0, 135.0]);
    let before = camera.iso_view().origin;
    camera.pan_cells(2.0, 0.0);
    let after = camera.iso_view().origin;
    assert!((after[0] - (before[0] - 2.0 * camera.iso_view().tile_w * 0.5)).abs() < 1e-3);
}

#[test]
fn pan_is_clamped_to_the_grid() {
    let mut camera = Camera::new(320, 320, 4.0, VIEW, [160.0, 160.0]);
    camera.pan_cells(-10_000.0, -10_000.0);
    assert_eq!(camera.center(), [0.0, 0.0]);
    camera.pan_cells(10_000.0, 10_000.0);
    assert_eq!(camera.center(), [320.0, 320.0]);
}

#[test]
fn start_center_is_clamped() {
    let camera = Camera::new(64, 64, 4.0, VIEW, [999.0, -999.0]);
    assert_eq!(camera.center(), [64.0, 0.0]);
}

#[test]
fn a_non_finite_pan_is_ignored() {
    let mut camera = Camera::new(480, 270, 4.0, VIEW, [240.0, 135.0]);
    let before = camera.center();
    camera.pan_cells(f32::NAN, 1.0);
    assert_eq!(camera.center(), before);
}

#[test]
fn pan_tick_uses_the_documented_speed() {
    let mut camera = Camera::new(480, 270, 4.0, VIEW, [240.0, 135.0]);
    let before = camera.center();
    camera.pan_tick([1.0, 0.0], 1.0 / 60.0);
    assert!((camera.center()[0] - (before[0] + CAMERA_PAN_CELLS_PER_SEC / 60.0)).abs() < 1e-6);
    assert_eq!(camera.center()[1], before[1]);
}

#[test]
fn diagonal_pan_is_not_normalised() {
    let mut cardinal = Camera::new(480, 270, 4.0, VIEW, [240.0, 135.0]);
    let mut diagonal = Camera::new(480, 270, 4.0, VIEW, [240.0, 135.0]);
    let dt = 1.0 / 60.0;
    cardinal.pan_tick([1.0, 0.0], dt);
    diagonal.pan_tick([1.0, 1.0], dt);

    let cardinal_step = cardinal.center()[0] - 240.0;
    let diagonal_step_x = diagonal.center()[0] - 240.0;
    let diagonal_step_y = diagonal.center()[1] - 135.0;
    assert!((diagonal_step_x - cardinal_step).abs() < 1e-6);
    assert!((diagonal_step_y - cardinal_step).abs() < 1e-6);
}

#[test]
fn edge_pan_fires_only_inside_the_margin() {
    let y = 540.0;
    let cases = [
        (0.0, -1.0),
        (11.9, -1.0),
        (EDGE_PAN_MARGIN_PX, -1.0),
        (12.1, 0.0),
        (1907.9, 0.0),
        (1920.0 - EDGE_PAN_MARGIN_PX, 1.0),
        (1920.0, 1.0),
    ];
    for (x, want_x) in cases {
        let got = edge_pan_dir([x, y], VIEW);
        assert_eq!(got[0], want_x, "x={x}");
    }
}

#[test]
fn edge_pan_outside_the_view_is_zero() {
    for mouse in [
        [-5.0, 540.0],
        [1925.0, 540.0],
        [960.0, -1.0],
        [f32::NAN, 0.0],
    ] {
        assert_eq!(edge_pan_dir(mouse, VIEW), [0.0, 0.0], "mouse={mouse:?}");
    }
}

#[test]
fn edge_pan_corner_pans_both_axes() {
    assert_eq!(edge_pan_dir([2.0, 2.0], VIEW), [-1.0, -1.0]);
}

// Both of the next two tests project the camera's *centre* with a fixed
// `[0.0, 0.0]` origin (exactly the pattern `IsoView::with_center_cell` uses
// internally) rather than through `camera.iso_view()`. The view re-centres
// on every pan, so the centre's projection through its own view is always
// the view's midpoint and can never show movement; the fixed-origin
// projection is what exposes which way the cell-space centre actually moved.
#[test]
fn screen_right_moves_the_camera_screen_right() {
    let mut camera = Camera::new(480, 270, 4.0, VIEW, [240.0, 135.0]);
    let tw = camera.iso_view().tile_w;
    let th = camera.iso_view().tile_h;
    let before = iso_project(camera.center()[0], camera.center()[1], tw, th, [0.0, 0.0]);
    let dir = screen_dir_to_cells([1.0, 0.0], tw, th);
    camera.pan_tick(dir, 1.0);
    let after = iso_project(camera.center()[0], camera.center()[1], tw, th, [0.0, 0.0]);
    assert!(after[0] > before[0], "before={before:?} after={after:?}");
}

#[test]
fn screen_down_moves_the_camera_screen_down() {
    let mut camera = Camera::new(480, 270, 4.0, VIEW, [240.0, 135.0]);
    let tw = camera.iso_view().tile_w;
    let th = camera.iso_view().tile_h;
    let before = iso_project(camera.center()[0], camera.center()[1], tw, th, [0.0, 0.0]);
    let dir = screen_dir_to_cells([0.0, 1.0], tw, th);
    camera.pan_tick(dir, 1.0);
    let after = iso_project(camera.center()[0], camera.center()[1], tw, th, [0.0, 0.0]);
    assert!(after[1] > before[1], "before={before:?} after={after:?}");
}

#[test]
fn look_at_cell_centres_that_cell() {
    let mut camera = Camera::new(480, 270, 4.0, VIEW, [240.0, 135.0]);
    camera.look_at_cell(Cell { x: 10, y: 200 });
    let projected = camera.iso_view().project(10.5, 200.5);
    assert!((projected[0] - 960.0).abs() < 1e-3);
    assert!((projected[1] - 540.0).abs() < 1e-3);
}
