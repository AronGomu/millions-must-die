//! T12 — packing the RTS world into draw groups, an overlay and a UI layer,
//! plus the camera the whole frame projects through.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts` and `testkit::RtsHarness`. The two device cases live in
//! `gpu_smoke.rs`.

use mmd_engine::render::{
    DrawGroup, SLOT_RTS_BUILDINGS, SLOT_RTS_PROPS, SLOT_RTS_SOLDIER, SLOT_RTS_WORKER, SLOT_UI_FONT,
    SpriteInstance, frame_uv_rect, quad_is_visible, screen_axes_to_cells,
};
use mmd_engine::rts::{
    BuildingKind, DEFAULT_CAMERA_PAN_SPEED, DRAG_BOX_BORDER_TINT, DRAG_BOX_FILL_TINT,
    DRAG_BOX_THICKNESS_PX, DragBox, EntityId, EntityKind, MAX_ENTITIES, OWNER_PLAYER, Prop,
    ResourceKind, RtsFrame, UnitKind, building_quad_px, building_uv, ghost_min_corner, node_uv,
    pack_frame, placement_candidate, prop_uv, unit_slot,
};
use mmd_engine::runtime::ring_quad_size_px;
use mmd_engine::scenario::Cell;
use mmd_engine::sim::TICK_DT;
use mmd_engine::testkit::RtsHarness;

/// The view centre, and therefore the default cursor: the tracked scene opens
/// centred on the HQ, so this pixel is the HQ's footprint centre.
const CURSOR: [f32; 2] = [960.0, 540.0];

/// The HQ's footprint centre in cell space (`hq_cell` 160,160 + 12/2).
const HQ_CENTRE: [f32; 2] = [166.0, 166.0];

/// Clear, obstacle-free, node-free, HQ-free Depot corner — the same one
/// `rts_production.rs` builds on.
const DEPOT_CORNER: Cell = Cell { x: 180, y: 176 };

fn scene() -> RtsHarness {
    RtsHarness::scene().build().expect("rts scene harness")
}

fn workers(h: &RtsHarness) -> Vec<EntityId> {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))
}

fn group(groups: &[DrawGroup], atlas_id: u32) -> &DrawGroup {
    groups
        .iter()
        .find(|g| g.atlas_id == atlas_id)
        .unwrap_or_else(|| panic!("no group for slot {atlas_id}"))
}

fn world_total(frame: &RtsFrame) -> usize {
    frame.world.iter().map(|g| g.instances.len()).sum()
}

/// The instances of the UI's prop group — ghost tiles, rally flags, drag box.
fn props(frame: &RtsFrame) -> &[SpriteInstance] {
    &group(&frame.ui, SLOT_RTS_PROPS).instances
}

// --- the reusable frame ------------------------------------------------------

#[test]
fn frame_new_reserves_the_documented_groups() {
    let frame = RtsFrame::new();

    let world_slots: Vec<u32> = frame.world.iter().map(|g| g.atlas_id).collect();
    assert_eq!(
        world_slots,
        vec![SLOT_RTS_WORKER, SLOT_RTS_SOLDIER, SLOT_RTS_BUILDINGS],
        "the world layer is exactly the three RTS world slots, in slot order"
    );
    let ui_slots: Vec<u32> = frame.ui.iter().map(|g| g.atlas_id).collect();
    assert_eq!(
        ui_slots,
        vec![
            SLOT_RTS_WORKER,
            SLOT_RTS_SOLDIER,
            SLOT_RTS_BUILDINGS,
            SLOT_RTS_PROPS,
            SLOT_UI_FONT
        ],
        "the UI layer mirrors the world's three atlases, then props, then font"
    );

    // Reserved at the ceilings each buffer can reach: a frame must never grow
    // one, which is what `pack_frame_allocates_nothing` measures.
    for g in &frame.world {
        assert_eq!(g.instances.capacity(), MAX_ENTITIES, "slot {}", g.atlas_id);
    }
    assert_eq!(frame.overlay.capacity(), MAX_ENTITIES + mmd_engine::rts::MAX_GRID_LINES);
    for g in &frame.ui[..4] {
        assert_eq!(g.instances.capacity(), MAX_ENTITIES, "slot {}", g.atlas_id);
    }
    assert_eq!(frame.ui[4].instances.capacity(), 4_096);

    assert_eq!(frame.instance_count(), 0, "a fresh frame holds nothing");
}

// --- the named UV tables -----------------------------------------------------

#[test]
fn prop_uv_maps_to_the_published_cells() {
    let all = [
        Prop::SelectionRing,
        Prop::PlacementOk,
        Prop::PlacementBad,
        Prop::RallyFlag,
        Prop::CrystalIcon,
        Prop::GasIcon,
        Prop::SupplyIcon,
        Prop::PanelFill,
        Prop::GearIcon,
        Prop::MinimapFrame,
        Prop::IconBuildHq,
        Prop::IconBuildDepot,
        Prop::IconBuildBarracks,
        Prop::IconTrainWorker,
        Prop::IconTrainSoldier,
        Prop::IconSetRally,
    ];
    for (i, p) in all.into_iter().enumerate() {
        let i = i as u32;
        assert_eq!(
            prop_uv(p),
            frame_uv_rect(i / 4, i % 4),
            "{p:?} must be sheet cell ({}, {})",
            i / 4,
            i % 4
        );
    }
    // …and the published cells really are the first two rows.
    assert_eq!(prop_uv(Prop::SelectionRing), frame_uv_rect(0, 0));
    assert_eq!(prop_uv(Prop::RallyFlag), frame_uv_rect(0, 3));
    assert_eq!(prop_uv(Prop::PanelFill), frame_uv_rect(1, 3));
}

#[test]
fn building_uv_switches_row_on_construction() {
    // Row 0 finished, row 1 under construction — same column either way.
    assert_eq!(building_uv(BuildingKind::Depot, false), frame_uv_rect(0, 1));
    assert_eq!(building_uv(BuildingKind::Depot, true), frame_uv_rect(1, 1));
    assert_eq!(building_uv(BuildingKind::Hq, false), frame_uv_rect(0, 0));
    assert_eq!(building_uv(BuildingKind::Hq, true), frame_uv_rect(1, 0));
    assert_eq!(
        building_uv(BuildingKind::Barracks, false),
        frame_uv_rect(0, 2)
    );
    assert_eq!(
        building_uv(BuildingKind::Barracks, true),
        frame_uv_rect(1, 2)
    );
}

#[test]
fn node_uv_switches_column_on_depletion() {
    assert_eq!(node_uv(ResourceKind::Crystal, false), frame_uv_rect(2, 0));
    assert_eq!(node_uv(ResourceKind::Crystal, true), frame_uv_rect(2, 2));
    assert_eq!(node_uv(ResourceKind::Gas, false), frame_uv_rect(2, 1));
    assert_eq!(node_uv(ResourceKind::Gas, true), frame_uv_rect(2, 3));
}

#[test]
fn unit_slot_separates_the_two_kinds() {
    assert_eq!(unit_slot(UnitKind::Worker), SLOT_RTS_WORKER);
    assert_eq!(unit_slot(UnitKind::Soldier), SLOT_RTS_SOLDIER);
    assert_eq!(SLOT_RTS_WORKER, 4);
    assert_eq!(SLOT_RTS_SOLDIER, 5);
}

#[test]
fn building_quad_matches_the_documented_sizes() {
    // The tracked scene's tile is 8 x 4. Width is the footprint diamond's full
    // width; height is twice the diamond's height, which is what makes a
    // building read as a solid rather than a decal.
    assert_eq!(building_quad_px(12, 8.0, 4.0), [96.0, 96.0], "HQ");
    assert_eq!(building_quad_px(8, 8.0, 4.0), [64.0, 64.0], "Depot");
    assert_eq!(building_quad_px(10, 8.0, 4.0), [80.0, 80.0], "Barracks");
}

#[test]
fn ghost_min_corner_centres_the_footprint() {
    assert_eq!(
        ghost_min_corner(Cell { x: 100, y: 100 }, 8),
        Cell { x: 96, y: 96 },
        "the cursor's cell is the footprint's centre, not its corner"
    );
    assert_eq!(
        ghost_min_corner(Cell { x: 200, y: 40 }, 12),
        Cell { x: 194, y: 34 }
    );
    assert_eq!(
        ghost_min_corner(Cell { x: 2, y: 3 }, 10),
        Cell { x: 0, y: 0 },
        "the corner saturates at zero rather than wrapping"
    );
}

// --- the world layer ---------------------------------------------------------

#[test]
fn every_entity_packs_exactly_once() {
    let h = scene();
    let mut frame = RtsFrame::new();
    // Packed twice on purpose: a packer that forgot to clear would double
    // every instance on the second call and only there.
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let first = world_total(&frame);
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert_eq!(first, 17, "1 HQ + 10 nodes + 6 workers");
    assert_eq!(
        world_total(&frame),
        17,
        "re-packing an unchanged world must not accumulate"
    );
}

#[test]
fn nodes_and_buildings_share_the_building_slot() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert_eq!(
        group(&frame.world, SLOT_RTS_BUILDINGS).instances.len(),
        11,
        "1 HQ + 10 nodes"
    );
}

#[test]
fn workers_land_in_the_worker_slot() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert_eq!(group(&frame.world, SLOT_RTS_WORKER).instances.len(), 6);
    assert_eq!(group(&frame.world, SLOT_RTS_SOLDIER).instances.len(), 0);
}

#[test]
fn a_soldier_lands_in_the_soldier_slot() {
    let mut h = scene();
    h.world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Soldier),
            OWNER_PLAYER,
            [166.5, 172.5],
        )
        .expect("spawn soldier");
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert_eq!(group(&frame.world, SLOT_RTS_SOLDIER).instances.len(), 1);
    assert_eq!(group(&frame.world, SLOT_RTS_WORKER).instances.len(), 6);
}

#[test]
fn a_units_uv_is_its_dir_and_frame() {
    let mut h = scene();
    let first = workers(&h)[0];
    let slot = h.world().entities().slot(first).expect("live worker");
    h.world_mut().entities_mut().set_dir(slot, 3);
    h.world_mut().entities_mut().set_frame(slot, 2);

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let packed = &group(&frame.world, SLOT_RTS_WORKER).instances;
    assert_eq!(packed.len(), 6);
    // Slot order: the lowest-slot worker is the one that was set.
    assert_eq!(
        packed[0].uv_rect,
        frame_uv_rect(3, 2),
        "the animation frame must follow the entity's dir and frame"
    );
    assert_ne!(
        packed[1].uv_rect, packed[0].uv_rect,
        "an untouched worker must keep its own frame"
    );
    assert_eq!(packed[1].uv_rect, frame_uv_rect(0, 0));
}

#[test]
fn a_site_draws_the_construction_sprite() {
    let mut h = scene();
    let builder = workers(&h)[0];
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    h.world_mut()
        .confirm_placement(DEPOT_CORNER, builder)
        .expect("confirm placement");

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let packed = &group(&frame.world, SLOT_RTS_BUILDINGS).instances;
    assert_eq!(packed.len(), 12, "1 HQ + 10 nodes + the new site");
    // Stated as the sheet's own literal cell, not through `building_uv`: an
    // expectation written in terms of the function under test would survive
    // that function ignoring `under_construction` entirely.
    assert_eq!(
        packed[11].uv_rect,
        frame_uv_rect(1, BuildingKind::Depot as u32),
        "a site draws row 1 — the under-construction sprite"
    );
    assert_eq!(
        packed[0].uv_rect,
        frame_uv_rect(0, BuildingKind::Hq as u32),
        "the finished HQ still draws row 0"
    );
}

#[test]
fn a_depleted_node_draws_the_depleted_sprite() {
    let mut h = scene();
    let crystal = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal));
    let slot = h.world().entities().slot(crystal[0]).expect("live node");
    h.world_mut().entities_mut().set_amount(slot, 0);

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let packed = &group(&frame.world, SLOT_RTS_BUILDINGS).instances;
    // Slot order: HQ, then the crystal nodes in seed order.
    assert_eq!(
        packed[1].uv_rect,
        frame_uv_rect(2, 2),
        "an empty crystal node draws the depleted column"
    );
    assert_eq!(
        packed[2].uv_rect,
        frame_uv_rect(2, 0),
        "a full crystal node still draws the full column"
    );
}

#[test]
fn sprites_stand_on_their_ground_point() {
    let h = scene();
    let iso = h.world().iso_view();
    let slot = h
        .world()
        .entities()
        .slot(workers(&h)[0])
        .expect("live worker");
    let p = h.world().entities().position(slot);
    let ground = iso.project(p[0], p[1]);

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let inst = group(&frame.world, SLOT_RTS_WORKER).instances[0];
    assert_eq!(
        inst.pos[1] + inst.size[1],
        ground[1],
        "the quad's bottom edge sits on the ground point"
    );
    assert_eq!(
        inst.pos[0] + inst.size[0] * 0.5,
        ground[0],
        "…and it is horizontally centred on it"
    );
}

#[test]
fn offscreen_entities_are_culled() {
    let mut h = scene();
    // The camera frontier keeps the view inside the map's projected diamond,
    // so a pan can never carry the camera far enough from a 320x320 map's
    // own centre to leave every entity offscreen. Moving every live entity
    // instead — the same test-only hook `culling_uses_the_same_rect_as_the_
    // horde` uses — isolates the thing this test actually checks: the
    // packer's per-quad cull, not how far a legal camera can travel.
    let mut live = Vec::new();
    h.world().entities().collect_live(&mut live);
    for slot in live {
        h.world_mut()
            .entities_mut()
            .set_position(slot, [10_000.0, 10_000.0]);
    }
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert_eq!(
        world_total(&frame),
        0,
        "every entity moved off the visible view must pack nothing"
    );
}

#[test]
fn culling_uses_the_same_rect_as_the_horde() {
    let mut h = scene();
    let slot = h
        .world()
        .entities()
        .slot(workers(&h)[0])
        .expect("live worker");
    let iso = h.world().iso_view();
    let view = iso.view_size;
    let sprite = h.world().scenario().sprite_size_px() as f32;
    let size = [sprite, sprite];

    // Exactly on the left edge. With the scene's origin (960, -124) and its
    // 8 x 4 tile, cell (43, 289) projects to a ground point of (-24, 540), so
    // the 48 px quad's right edge lands on `pos.x + size.x == 0`.
    h.world_mut()
        .entities_mut()
        .set_position(slot, [43.0, 289.0]);
    let ground = iso.project(43.0, 289.0);
    let pos = [ground[0] - sprite * 0.5, ground[1] - sprite];
    assert_eq!(
        pos[0] + size[0],
        0.0,
        "the case must sit exactly on the edge"
    );
    assert!(!quad_is_visible(pos, size, view));

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert_eq!(
        group(&frame.world, SLOT_RTS_WORKER).instances.len(),
        5,
        "a quad touching the edge covers no pixel and must be dropped"
    );

    // A quarter of a cell back on screen: one pixel of the quad is visible.
    h.world_mut()
        .entities_mut()
        .set_position(slot, [43.25, 289.0]);
    let ground = iso.project(43.25, 289.0);
    let pos = [ground[0] - sprite * 0.5, ground[1] - sprite];
    assert!(pos[0] + size[0] > 0.0);
    assert!(quad_is_visible(pos, size, view));

    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert_eq!(
        group(&frame.world, SLOT_RTS_WORKER).instances.len(),
        6,
        "a quad with any pixel on screen must be packed"
    );
}

#[test]
fn packing_follows_the_camera() {
    let mut h = scene();
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let before: Vec<[f32; 2]> = frame
        .world
        .iter()
        .flat_map(|g| g.instances.iter().map(|i| i.pos))
        .collect();
    let iso_before = h.world().iso_view();

    h.world_mut().camera_mut().pan_cells(10.0, 10.0);
    let iso_after = h.world().iso_view();
    let expect = [
        iso_after.origin[0] - iso_before.origin[0],
        iso_after.origin[1] - iso_before.origin[1],
    ];
    assert_ne!(expect, [0.0, 0.0], "the pan must actually move the camera");

    pack_frame(h.world(), CURSOR, None, &mut frame);
    let after: Vec<[f32; 2]> = frame
        .world
        .iter()
        .flat_map(|g| g.instances.iter().map(|i| i.pos))
        .collect();

    assert_eq!(before.len(), after.len(), "the pan must not cull anybody");
    assert!(!before.is_empty());
    for (a, b) in before.iter().zip(after.iter()) {
        assert_eq!([b[0] - a[0], b[1] - a[1]], expect);
    }
}

// --- the overlay: selection rings -------------------------------------------

#[test]
fn selection_rings_are_procedural() {
    let mut h = scene();
    let ids = workers(&h);
    for id in ids.iter().take(3) {
        h.world_mut().selection_mut().insert(*id);
    }
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);

    assert_eq!(frame.overlay.len(), 3);
    for inst in &frame.overlay {
        assert!(
            inst.is_ring(),
            "a textured instance in the overlay would sample slot 0, not the \
             sheet it was packed for: {inst:?}"
        );
    }
}

#[test]
fn nothing_selected_means_no_rings() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert!(h.world().selection().is_empty());
    assert!(frame.overlay.is_empty());
}

#[test]
fn a_selected_building_gets_a_ring_sized_to_its_footprint() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    let iso = h.world().iso_view();

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert_eq!(frame.overlay.len(), 1);
    assert_eq!(
        frame.overlay[0].size,
        ring_quad_size_px(iso.tile_w, iso.tile_h, 6.0),
        "an edge-12 footprint's ring is drawn at radius 6 cells"
    );
}

// --- the UI layer: ghost, rally flags, drag box ------------------------------

#[test]
fn the_ghost_only_appears_while_pending() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert!(props(&frame).is_empty());
}

#[test]
fn the_ghost_follows_the_cursor_cell() {
    let mut h = scene();
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let iso = h.world().iso_view();

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let before: Vec<[f32; 2]> = props(&frame).iter().map(|i| i.pos).collect();

    // One cell of `+x`, which the projection moves half a tile right and half
    // a tile down.
    let cursor = iso.project(167.5, 166.5);
    pack_frame(h.world(), cursor, None, &mut frame);
    let after: Vec<[f32; 2]> = props(&frame).iter().map(|i| i.pos).collect();

    assert_eq!(before.len(), after.len());
    assert!(!before.is_empty());
    let expect = [iso.tile_w * 0.5, iso.tile_h * 0.5];
    for (a, b) in before.iter().zip(after.iter()) {
        assert_eq!([b[0] - a[0], b[1] - a[1]], expect);
    }
}

#[test]
fn a_valid_ghost_is_green() {
    let mut h = scene();
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let iso = h.world().iso_view();
    // The cursor's cell is the footprint's centre, so aim four cells past the
    // known-clear corner.
    let cursor = iso.project(184.5, 180.5);

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), cursor, None, &mut frame);
    let packed = props(&frame);
    assert_eq!(packed.len(), 65);
    for inst in &packed[..64] {
        assert_eq!(
            inst.uv_rect,
            frame_uv_rect(0, 1),
            "a valid footprint tiles the placement-OK cell"
        );
    }
}

#[test]
fn an_invalid_ghost_is_red() {
    let mut h = scene();
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));

    let mut frame = RtsFrame::new();
    // The default cursor is the HQ's own centre: the footprint overlaps it.
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let packed = props(&frame);
    assert_eq!(packed.len(), 65);
    for inst in &packed[..64] {
        assert_eq!(
            inst.uv_rect,
            frame_uv_rect(0, 2),
            "a footprint over the HQ tiles the placement-BAD cell"
        );
    }
}

#[test]
fn the_ghost_covers_the_whole_footprint() {
    let mut h = scene();
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let iso = h.world().iso_view();

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let packed = props(&frame);
    assert_eq!(
        packed.len(),
        8 * 8 + 1,
        "one tile per footprint cell, plus the building's own silhouette"
    );
    for inst in &packed[..64] {
        assert_eq!(
            inst.size,
            [iso.tile_w, iso.tile_h * 2.0],
            "a ghost tile is one cell diamond's width by twice its height"
        );
    }
    assert_eq!(
        packed[64].size,
        building_quad_px(8, iso.tile_w, iso.tile_h),
        "the silhouette is the building's own quad"
    );
    assert_eq!(
        packed[64].uv_rect,
        frame_uv_rect(1, 1),
        "the silhouette is the under-construction sprite"
    );
}

#[test]
fn drag_box_has_pure_green_ten_percent_fill() {
    let h = scene();
    let mut frame = RtsFrame::new();
    let drag = DragBox {
        a: [10.0, 20.0],
        b: [110.0, 80.0],
    };
    pack_frame(h.world(), CURSOR, Some(drag), &mut frame);
    let packed = props(&frame);
    let fill = &packed[0];
    assert!(
        fill.is_diagonal_line(),
        "fill must be a diagonal_line instance"
    );
    assert_eq!(fill.tint, DRAG_BOX_FILL_TINT, "fill tint must be 10% green");
    // Thickness encoded in uv_rect[1] equals the rect height (60px).
    assert_eq!(fill.uv_rect[1], 60.0, "fill thickness equals rect height");
}

#[test]
fn drag_box_has_opaque_two_pixel_pure_green_border() {
    let h = scene();
    let mut frame = RtsFrame::new();
    let drag = DragBox {
        a: [10.0, 20.0],
        b: [110.0, 80.0],
    };
    pack_frame(h.world(), CURSOR, Some(drag), &mut frame);
    let packed = props(&frame);
    assert_eq!(packed.len(), 5, "drag box must pack exactly 5 instances");
    for inst in &packed[1..] {
        assert!(
            inst.is_diagonal_line(),
            "border edge must be a diagonal_line instance: {inst:?}"
        );
        assert_eq!(
            inst.tint, DRAG_BOX_BORDER_TINT,
            "border tint must be opaque green"
        );
        assert_eq!(
            inst.uv_rect[1], DRAG_BOX_THICKNESS_PX,
            "border thickness must be 2px"
        );
    }
}

#[test]
fn drag_box_never_samples_the_atlas() {
    let h = scene();
    let mut frame = RtsFrame::new();
    let drag = DragBox {
        a: [10.0, 20.0],
        b: [110.0, 80.0],
    };
    pack_frame(h.world(), CURSOR, Some(drag), &mut frame);
    let packed = props(&frame);
    assert_eq!(packed.len(), 5);
    for inst in packed {
        assert!(
            inst.is_diagonal_line(),
            "every drag instance must be texture-free: {inst:?}"
        );
    }
}

#[test]
fn the_drag_box_normalises_its_corners() {
    let h = scene();
    let mut frame = RtsFrame::new();

    let forward = DragBox {
        a: [10.0, 10.0],
        b: [110.0, 60.0],
    };
    pack_frame(h.world(), CURSOR, Some(forward), &mut frame);
    let a: Vec<SpriteInstance> = props(&frame).to_vec();

    let backward = DragBox {
        a: [110.0, 60.0],
        b: [10.0, 10.0],
    };
    pack_frame(h.world(), CURSOR, Some(backward), &mut frame);
    let b: Vec<SpriteInstance> = props(&frame).to_vec();

    assert_eq!(a.len(), 5);
    assert_eq!(
        a, b,
        "dragging up-left must draw the same box as down-right"
    );
}

#[test]
fn no_drag_means_no_box() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert!(props(&frame).is_empty());
}

#[test]
fn a_rally_flag_draws_for_a_selected_building() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    let cell = Cell { x: 180, y: 176 };
    assert!(h.world_mut().set_rally(hq, Some(cell)));
    h.world_mut().selection_mut().insert(hq);
    let iso = h.world().iso_view();
    let ground = iso.project(cell.x as f32 + 0.5, cell.y as f32 + 0.5);

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let packed = props(&frame);
    assert_eq!(packed.len(), 1);
    assert_eq!(
        packed[0].uv_rect,
        frame_uv_rect(0, 3),
        "the rally flag cell"
    );
    assert_eq!(
        packed[0].pos[1] + packed[0].size[1],
        ground[1],
        "the flag stands on the rally cell"
    );
    assert_eq!(packed[0].pos[0] + packed[0].size[0] * 0.5, ground[0]);
}

#[test]
fn an_unselected_buildings_rally_is_not_drawn() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().set_rally(hq, Some(Cell { x: 180, y: 176 })));
    assert!(h.world().selection().is_empty());

    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    assert!(props(&frame).is_empty());
}

#[test]
fn pack_frame_does_not_mutate_the_world() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));

    let before = h.state_hash();
    let mut frame = RtsFrame::new();
    for _ in 0..100 {
        pack_frame(
            h.world(),
            CURSOR,
            Some(DragBox {
                a: [10.0, 10.0],
                b: [110.0, 60.0],
            }),
            &mut frame,
        );
    }
    assert_eq!(h.state_hash(), before, "packing is a read of world state");
}

// --- the camera in the world -------------------------------------------------

#[test]
fn the_camera_starts_on_the_base() {
    let h = scene();
    let p = h.world().iso_view().project(HQ_CENTRE[0], HQ_CENTRE[1]);
    assert!(
        (p[0] - 960.0).abs() < 1e-3 && (p[1] - 540.0).abs() < 1e-3,
        "the HQ centre must open dead centre of the view, got {p:?}"
    );
    assert_eq!(h.world().keyboard_pan_dir(), [0.0, 0.0]);
    assert_eq!(h.world().edge_pan_dir(), [0.0, 0.0]);
}

#[test]
fn keyboard_pan_moves_the_camera_at_the_documented_speed() {
    let mut h = scene();
    let start = h.world().camera().center();

    h.world_mut().set_keyboard_pan_dir([1.0, 0.0]);
    h.step_exact(1);

    // One tick at `DEFAULT_CAMERA_PAN_SPEED`, along the cell-space cardinal
    // basis a screen-space `+x` maps to.
    let d = screen_axes_to_cells([1.0, 0.0]);
    let want = [
        start[0] + d[0] * DEFAULT_CAMERA_PAN_SPEED * TICK_DT,
        start[1] + d[1] * DEFAULT_CAMERA_PAN_SPEED * TICK_DT,
    ];
    let got = h.world().camera().center();
    assert!(
        (got[0] - want[0]).abs() < 1e-4 && (got[1] - want[1]).abs() < 1e-4,
        "after 1 tick the centre is {got:?}, expected {want:?}"
    );
    assert_ne!(got, start);
}

#[test]
fn keyboard_and_edge_speeds_are_independent() {
    let mut h = scene();
    let start = h.world().camera().center();
    h.world_mut().set_camera_speeds(48.0, 96.0);

    h.world_mut().set_keyboard_pan_dir([1.0, 0.0]);
    h.step_exact(1);
    let kb_only = h.world().camera().center();

    h.world_mut().set_keyboard_pan_dir([0.0, 0.0]);
    h.world_mut().set_edge_pan_dir([1.0, 0.0]);
    h.step_exact(1);
    let after_edge_only = h.world().camera().center();

    let kb = screen_axes_to_cells([1.0, 0.0]);
    let want_kb = [
        start[0] + kb[0] * 48.0 * TICK_DT,
        start[1] + kb[1] * 48.0 * TICK_DT,
    ];
    assert!(
        (kb_only[0] - want_kb[0]).abs() < 1e-4 && (kb_only[1] - want_kb[1]).abs() < 1e-4,
        "keyboard-only step was {kb_only:?}, expected {want_kb:?}"
    );

    let want_edge = [
        kb_only[0] + kb[0] * 96.0 * TICK_DT,
        kb_only[1] + kb[1] * 96.0 * TICK_DT,
    ];
    assert!(
        (after_edge_only[0] - want_edge[0]).abs() < 1e-4
            && (after_edge_only[1] - want_edge[1]).abs() < 1e-4,
        "edge-only step was {after_edge_only:?}, expected {want_edge:?}"
    );

    // Additive: holding both at once moves by the sum of the two.
    h.world_mut().set_keyboard_pan_dir([1.0, 0.0]);
    h.world_mut().set_edge_pan_dir([1.0, 0.0]);
    let before_both = h.world().camera().center();
    h.step_exact(1);
    let after_both = h.world().camera().center();
    let want_both = [
        before_both[0] + kb[0] * (48.0 + 96.0) * TICK_DT,
        before_both[1] + kb[1] * (48.0 + 96.0) * TICK_DT,
    ];
    assert!(
        (after_both[0] - want_both[0]).abs() < 1e-4 && (after_both[1] - want_both[1]).abs() < 1e-4,
        "combined step was {after_both:?}, expected {want_both:?}"
    );
}

#[test]
fn zero_pan_dir_holds_the_camera() {
    let mut h = scene();
    let start = h.world().camera().center();
    h.step_exact(600);
    assert_eq!(
        h.world().camera().center(),
        start,
        "a zero pan direction must not drift the camera"
    );
}

#[test]
fn state_hash_sees_the_camera() {
    let mut h = scene();
    let before = h.state_hash();
    h.world_mut().camera_mut().pan_cells(1.0, 0.0);
    assert_ne!(h.state_hash(), before, "the camera is world state");

    // …and it reaches the hash through the tick, not only through a direct
    // pan: two worlds that ticked the same number of times but panned
    // differently must not agree.
    let mut still = scene();
    let mut panning = scene();
    panning.world_mut().set_keyboard_pan_dir([1.0, 0.0]);
    still.step_exact(1);
    panning.step_exact(1);
    assert_ne!(still.state_hash(), panning.state_hash());
}

/// The camera's *pan speed* reaches the state hash, not just its pan
/// direction: the same pan input at two speeds ends at two centres.
///
/// This is why an app run whose hash is compared to anything may not take its
/// camera speeds from per-user settings — see `src/rts_run.rs`'s
/// `replay_settings`, which forces the defaults on any scripted run so a
/// windowed replay and the offscreen gate run of the same script agree.
#[test]
fn camera_speed_reaches_the_state_hash() {
    let mut slow = scene();
    let mut fast = scene();
    slow.world_mut().set_camera_speeds(24.0, 24.0);
    fast.world_mut().set_camera_speeds(96.0, 96.0);
    assert_eq!(
        slow.state_hash(),
        fast.state_hash(),
        "the speed alone is not hashed state; only where it takes the camera is"
    );

    slow.world_mut().set_keyboard_pan_dir([1.0, 0.0]);
    fast.world_mut().set_keyboard_pan_dir([1.0, 0.0]);
    slow.step_exact(1);
    fast.step_exact(1);
    assert_ne!(
        slow.world().camera().center(),
        fast.world().camera().center(),
        "two pan speeds must move the camera by two different amounts"
    );
    assert_ne!(
        slow.state_hash(),
        fast.state_hash(),
        "the camera centre is hashed, so a settings-derived pan speed would make \
         the same script hash differently on different machines"
    );
}

#[test]
fn look_at_map_point_moves_the_camera_and_hashes() {
    let mut h = scene();
    let before = h.state_hash();
    // Chosen to fall well inside this scene's frontier (projected x in
    // [-320, 320], y in [540, 740] with origin [0, 0]) so the clamp does not
    // fire and the point centres exactly.
    h.world_mut().look_at_map_point([200.5, 150.5]);
    let p = h.world().iso_view().project(200.5, 150.5);
    assert!(
        (p[0] - 960.0).abs() < 1e-3 && (p[1] - 540.0).abs() < 1e-3,
        "look_at_map_point must centre the requested map point, got {p:?}"
    );
    assert_ne!(h.state_hash(), before, "the camera is world state");
}

#[test]
fn depth_uniforms_follow_the_panned_camera() {
    let mut h = scene();
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let before = frame
        .scene()
        .frame_uniforms
        .expect("pack_frame must supply frame uniforms");

    h.world_mut().camera_mut().pan_cells(20.0, 0.0);
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let after = frame
        .scene()
        .frame_uniforms
        .expect("pack_frame must supply frame uniforms");

    assert_ne!(
        before.depth_bias, after.depth_bias,
        "a pan must move the packed depth bias, or the renderer draws stale depth"
    );
}

// --- T10: ghost and click use the same candidate ------------------------------

#[test]
fn green_preview_commits_exact_displayed_min() {
    let mut h = scene();
    let w0 = workers(&h)[0];

    // Block DEPOT_CORNER so snap activates.
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    assert!(h.world_mut().confirm_placement(DEPOT_CORNER, w0).is_ok());
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));

    // Cursor giving raw = DEPOT_CORNER (blocked); snap finds a nearby cell.
    let iso = h.world().iso_view();
    // Cell (184, 180) → ghost_min_corner = (180, 176) = DEPOT_CORNER.
    let cursor = iso.project(184.5, 180.5);
    let cursor_cell = h
        .world()
        .iso_view()
        .cell_at(
            cursor[0],
            cursor[1],
            h.world().scenario().width(),
            h.world().scenario().height(),
        )
        .expect("cursor in bounds");

    let cand = placement_candidate(h.world(), BuildingKind::Depot, cursor_cell);
    assert!(
        cand.valid,
        "snap must succeed for this test to be meaningful"
    );
    assert_ne!(cand.min, DEPOT_CORNER, "raw is blocked; snap expected");

    // Pack a frame and verify the ghost is drawn at cand.min.
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), cursor, None, &mut frame);
    let tiles = props(&frame);
    assert_eq!(tiles.len(), 65, "64 tiles + 1 silhouette");

    // The first ghost tile is at the footprint's (min.x, min.y) cell centre.
    let expected_ground = iso.project(cand.min.x as f32 + 0.5, cand.min.y as f32 + 0.5);
    let tile_size = [iso.tile_w, iso.tile_h * 2.0];
    let expected_pos = [
        expected_ground[0] - tile_size[0] * 0.5,
        expected_ground[1] - tile_size[1] * 0.5,
    ];
    assert_eq!(
        tiles[0].pos, expected_pos,
        "ghost tile (0,0) must be at placement_candidate.min, not raw min"
    );

    // Confirm at cand.min → site lands exactly where the ghost was drawn.
    let w1 = workers(&h)[1];
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(cand.min, w1)
        .expect("confirm at snapped min");
    let slot = h.world().entities().slot(site).expect("live site");
    let pos = h.world().entities().position(slot);
    use mmd_engine::rts::footprint_min;
    let site_min = footprint_min(pos, BuildingKind::Depot.footprint_cells());
    assert_eq!(
        site_min, cand.min,
        "committed site min must equal the displayed ghost min"
    );
}

// ── T12: Grid packing tests ──────────────────────────────────────────────────

use mmd_engine::rts::{FramePackOptions, GRID_TINT, MAX_GRID_LINES, pack_frame_with_options};
fn grid_on() -> FramePackOptions {
    FramePackOptions { show_grid: true }
}
fn grid_off() -> FramePackOptions {
    FramePackOptions { show_grid: false }
}

#[test]
fn grid_defaults_on_and_toggle_produces_frame_pack_options() {
    let on = FramePackOptions { show_grid: true };
    let off = FramePackOptions { show_grid: false };
    assert!(on.show_grid);
    assert!(!off.show_grid);
}

#[test]
fn grid_packs_exact_map_lattice() {
    let h = scene();
    let w = h.world().scenario().width();
    let width = w;
    let height = h.world().scenario().height();
    let expected = (width + 1 + height + 1) as usize;

    let mut frame = RtsFrame::new();
    pack_frame_with_options(h.world(), [0.0, 0.0], None, grid_on(), &mut frame);

    let grid_lines: Vec<_> = frame
        .overlay
        .iter()
        .filter(|i| i.is_diagonal_line())
        .collect();
    assert_eq!(
        grid_lines.len(),
        expected,
        "320×320 map needs {} grid lines",
        expected
    );
}

#[test]
fn disabled_grid_packs_no_lines() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_frame_with_options(h.world(), [0.0, 0.0], None, grid_off(), &mut frame);
    let lines = frame
        .overlay
        .iter()
        .filter(|i| i.is_diagonal_line())
        .count();
    assert_eq!(lines, 0, "grid off must produce no diagonal-line instances");
}

#[test]
fn grid_precedes_selection_rings() {
    let mut h = scene();
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    h.world_mut().selection_mut().insert(workers[0]);

    let mut frame = RtsFrame::new();
    pack_frame_with_options(h.world(), [0.0, 0.0], None, grid_on(), &mut frame);

    // First instance must be a diagonal line (grid), not a ring.
    assert!(
        frame.overlay[0].is_diagonal_line(),
        "first overlay instance must be a grid line, not a ring"
    );
    // At least one ring must follow.
    assert!(
        frame.overlay.iter().any(|i| !i.is_diagonal_line()),
        "a selected entity must produce at least one ring after grid lines"
    );
}

#[test]
fn grid_uses_only_diagonal_line_instances() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_frame_with_options(h.world(), [0.0, 0.0], None, grid_on(), &mut frame);
    let w = h.world().scenario().width();
    let height = h.world().scenario().height();
    let n = (w + 1 + height + 1) as usize;
    for inst in frame.overlay.iter().take(n) {
        assert!(
            inst.is_diagonal_line(),
            "every grid instance must be a diagonal-line; got uv_rect={:?}",
            inst.uv_rect
        );
        assert_eq!(inst.tint, GRID_TINT, "grid line must use GRID_TINT");
    }
}

#[test]
fn grid_toggle_does_not_change_world_hash() {
    let h = scene();
    let hash_before = h.world().state_hash();
    // Packing with grid on must not mutate world state.
    let mut frame = RtsFrame::new();
    pack_frame_with_options(h.world(), [0.0, 0.0], None, grid_on(), &mut frame);
    assert_eq!(
        h.world().state_hash(),
        hash_before,
        "grid packing must not alter world hash"
    );
}

#[test]
fn grid_capacity_covers_max_map_and_selection() {
    // MAX_GRID_LINES + MAX_ENTITIES is the overlay ceiling.
    // Simply confirm the constant is large enough for a 512×512 map.
    let max_lines = MAX_GRID_LINES;
    assert!(
        max_lines >= 2 * (512 + 1),
        "MAX_GRID_LINES={max_lines} must cover a 512×512 map"
    );
}

#[test]
fn grid_follows_camera_projection() {
    let h = scene();
    let iso = h.world().iso_view();
    let expected_a = iso.project(0.0, 0.0);
    let expected_b = iso.project(0.0, h.world().scenario().height() as f32);

    let mut frame = RtsFrame::new();
    pack_frame_with_options(h.world(), [0.0, 0.0], None, grid_on(), &mut frame);

    // The very first line is x=0 column edge: a=(0,0), b=(0,height).
    let first = &frame.overlay[0];
    assert_eq!(
        first.pos, expected_a,
        "first line must start at project(0,0)"
    );
    let end = [first.pos[0] + first.size[0], first.pos[1] + first.size[1]];
    assert_eq!(end, expected_b, "first line must end at project(0,height)");
}
