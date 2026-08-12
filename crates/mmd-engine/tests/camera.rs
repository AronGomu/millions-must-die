//! T4/T9 — panning camera, screen<->cell unprojection, and the projected
//! camera frontier.
//!
//! Pure math, no GPU: every case here is a headless CPU check of
//! `iso_unproject`, `IsoView::{unproject, cell_at, with_center_cell}` and the
//! `render::camera` module.

use mmd_engine::render::{
    Camera, CameraFrontier, EDGE_PAN_MARGIN_PX, IsoView, edge_pan_dir, iso_project, iso_unproject,
    screen_axes_to_cells,
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

// --- T9: the projected camera frontier ---------------------------------

#[test]
fn frontier_shrinks_projected_map_by_view() {
    // 320x320, tile 8x4 (cell_size_px 4), 1920x1080 view.
    let frontier = CameraFrontier::new(320, 320, 8.0, 4.0, VIEW);
    assert_eq!(frontier.x, [-320.0, 320.0]);
    assert_eq!(frontier.y, [540.0, 740.0]);
}

#[test]
fn undersized_axis_collapses_to_midpoint() {
    // A map far smaller than the view along both axes: the diamond's own
    // extent minus half the view inverts on both, so each axis collapses to
    // the diamond's un-inset midpoint.
    let frontier = CameraFrontier::new(4, 4, 8.0, 4.0, VIEW);
    // aabb_x = [-16, 16], midpoint 0. aabb_y = [0, 16], midpoint 8.
    assert_eq!(frontier.x, [0.0, 0.0]);
    assert_eq!(frontier.y, [8.0, 8.0]);
}

#[test]
fn camera_cannot_cross_any_frontier_edge() {
    let mut camera = Camera::new(320, 320, 4.0, VIEW, [160.0, 160.0]);
    let frontier = camera.frontier();

    camera.pan_cells(-1_000_000.0, -1_000_000.0);
    let p = iso_project(camera.center()[0], camera.center()[1], 8.0, 4.0, [0.0, 0.0]);
    assert!(
        p[0] >= frontier.x[0] - 1e-3 && p[0] <= frontier.x[1] + 1e-3,
        "{p:?}"
    );
    assert!(
        p[1] >= frontier.y[0] - 1e-3 && p[1] <= frontier.y[1] + 1e-3,
        "{p:?}"
    );

    camera.pan_cells(2_000_000.0, 2_000_000.0);
    let p = iso_project(camera.center()[0], camera.center()[1], 8.0, 4.0, [0.0, 0.0]);
    assert!(
        p[0] >= frontier.x[0] - 1e-3 && p[0] <= frontier.x[1] + 1e-3,
        "{p:?}"
    );
    assert!(
        p[1] >= frontier.y[0] - 1e-3 && p[1] <= frontier.y[1] + 1e-3,
        "{p:?}"
    );

    camera.look_at_point([-9_999.0, 9_999.0]);
    let p = iso_project(camera.center()[0], camera.center()[1], 8.0, 4.0, [0.0, 0.0]);
    assert!(
        p[0] >= frontier.x[0] - 1e-3 && p[0] <= frontier.x[1] + 1e-3,
        "{p:?}"
    );
    assert!(
        p[1] >= frontier.y[0] - 1e-3 && p[1] <= frontier.y[1] + 1e-3,
        "{p:?}"
    );
}

#[test]
fn look_at_point_clamps_fractional_target() {
    let mut camera = Camera::new(320, 320, 4.0, VIEW, [160.0, 160.0]);
    let frontier = camera.frontier();
    camera.look_at_point([500.0, -500.0]);
    let p = iso_project(camera.center()[0], camera.center()[1], 8.0, 4.0, [0.0, 0.0]);
    assert!(
        p[0] >= frontier.x[0] - 1e-3 && p[0] <= frontier.x[1] + 1e-3,
        "{p:?}"
    );
    assert!(
        p[1] >= frontier.y[0] - 1e-3 && p[1] <= frontier.y[1] + 1e-3,
        "{p:?}"
    );
}

#[test]
fn a_non_finite_pan_is_ignored() {
    let mut camera = Camera::new(480, 270, 4.0, VIEW, [240.0, 135.0]);
    let before = camera.center();
    camera.pan_cells(f32::NAN, 1.0);
    assert_eq!(camera.center(), before);
}

#[test]
fn a_non_finite_look_at_point_is_ignored() {
    let mut camera = Camera::new(480, 270, 4.0, VIEW, [240.0, 135.0]);
    let before = camera.center();
    camera.look_at_point([f32::NAN, 1.0]);
    assert_eq!(camera.center(), before);
}

// --- screen <-> cell cardinal basis --------------------------------------

#[test]
fn screen_axes_map_to_cell_axes() {
    assert_eq!(screen_axes_to_cells([1.0, 0.0]), [1.0, -1.0]);
    assert_eq!(screen_axes_to_cells([-1.0, 0.0]), [-1.0, 1.0]);
    assert_eq!(screen_axes_to_cells([0.0, 1.0]), [1.0, 1.0]);
    assert_eq!(screen_axes_to_cells([0.0, -1.0]), [-1.0, -1.0]);
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

#[test]
fn look_at_cell_centres_that_cell() {
    // (240, 135) sits well inside this grid's frontier, so the clamp does
    // not fire and the cell centres exactly.
    let mut camera = Camera::new(480, 270, 4.0, VIEW, [0.0, 0.0]);
    camera.look_at_cell(Cell { x: 240, y: 135 });
    let projected = camera.iso_view().project(240.5, 135.5);
    assert!((projected[0] - 960.0).abs() < 1e-3);
    assert!((projected[1] - 540.0).abs() < 1e-3);
}
