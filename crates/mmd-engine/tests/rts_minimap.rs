//! T12 — minimap projection: raw isometric fit, inverse, camera polygon.
//!
//! Pure logic, no GPU, no clock. HUD hit testing lives in `rts_hud.rs`
//! (`hit_*` tests); this file only proves the projection math.

use mmd_engine::rts::{HudLayout, MinimapProjection, minimap_projection};
use mmd_engine::testkit::RtsHarness;

fn scene() -> RtsHarness {
    RtsHarness::scene().build().expect("rts scene harness")
}

/// `rts_prototype_v1.ron` is a square 320x320 map, and `HudLayout::MINIMAP_MAP`
/// is exactly `2:1` (`352x176`), matching the raw isometric bounding box's own
/// `2:1` ratio for a square map — so this projection has no letterbox offset.
fn projection() -> MinimapProjection {
    MinimapProjection::new(
        [0.0, 0.0, 320.0, 320.0],
        HudLayout::MINIMAP_MAP[2] as u32,
        HudLayout::MINIMAP_MAP[3] as u32,
    )
}

fn assert_round_trips(p: MinimapProjection, cell: [f32; 2]) {
    let local = p.map_to_minimap(cell);
    let back = p.minimap_to_map(local).unwrap_or_else(|| {
        panic!("cell {cell:?} -> local {local:?} rejected as outside the diamond")
    });
    assert!(
        (back[0] - cell[0]).abs() <= 0.01 && (back[1] - cell[1]).abs() <= 0.01,
        "cell {cell:?} round-tripped to {back:?} via local {local:?}"
    );
}

#[test]
fn minimap_projection_round_trips_map_corners() {
    let p = projection();
    for cell in [
        [0.0, 0.0],
        [320.0, 0.0],
        [320.0, 320.0],
        [0.0, 320.0],
        [160.0, 160.0],
    ] {
        assert_round_trips(p, cell);
    }
}

#[test]
fn minimap_projection_matches_the_helper_built_from_a_world() {
    let h = scene();
    let p = minimap_projection(h.world());
    assert_eq!(
        p,
        projection(),
        "the world-derived projection must agree with a manual one"
    );
}

#[test]
fn outside_diamond_is_rejected() {
    let p = projection();
    // The minimap pixel box's own top-left corner: inside the rectangular
    // pixel box, outside the map's projected diamond.
    assert_eq!(p.minimap_to_map([0.0, 0.0]), None);
    // The bottom-right corner is the same story, mirrored.
    assert_eq!(
        p.minimap_to_map([HudLayout::MINIMAP_MAP[2], HudLayout::MINIMAP_MAP[3]]),
        None
    );
}

#[test]
fn camera_polygon_projects_four_view_corners() {
    let h = scene();
    let p = minimap_projection(h.world());
    let view = h.world().iso_view();

    let expected: Vec<[f32; 2]> = [[0.0, 0.0], [1920.0, 0.0], [1920.0, 1080.0], [0.0, 1080.0]]
        .into_iter()
        .map(|c| {
            let world_pt = view.unproject(c[0], c[1]);
            p.map_to_minimap(world_pt)
        })
        .collect();

    let got = p.camera_polygon(&view);
    assert_eq!(got.len(), 4);
    for (i, e) in expected.iter().enumerate() {
        assert_eq!(
            got[i], *e,
            "corner {i} did not match a direct unproject+project"
        );
    }
}
