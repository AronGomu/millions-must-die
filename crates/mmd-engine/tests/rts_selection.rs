//! T8 — selection: turning a screen-space pointer gesture into a set of
//! entity handles.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts::selection` and `testkit::RtsHarness`.

use mmd_engine::render::{Camera, IsoView};
use mmd_engine::rts::{
    BuildingKind, DRAG_MIN_PX, EntityId, EntityKind, OWNER_NEUTRAL, OWNER_PLAYER, Pick,
    RTS_SPRITE_SIZE_PX, ResourceKind, Selection, UnitKind, building_pick_contains,
    building_plot_contains, building_screen_rect, entity_pick_depth, footprint_contains,
    footprint_min, is_drag, normalise_rect, pick_at, sprite_screen_rect, unit_pick_contains,
};
use mmd_engine::scenario::Cell;
use mmd_engine::testkit::RtsHarness;

/// The view every pick/box test in this file is built against, unless stated
/// otherwise.
fn view() -> IsoView {
    Camera::new(320, 320, 4.0, [1920.0, 1080.0], [166.0, 172.0]).iso_view()
}

fn workers(h: &RtsHarness) -> Vec<EntityId> {
    let ids = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(ids.len(), 6, "the tracked scene seeds exactly six workers");
    ids
}

fn set_pos(h: &mut RtsHarness, id: EntityId, pos: [f32; 2]) {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world_mut().entities_mut().set_position(slot, pos);
}

// --- Selection ---------------------------------------------------------------

#[test]
fn selection_iterates_in_slot_order() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let mut sel = Selection::new();
    for &id in ids.iter().rev() {
        sel.insert(id);
    }
    assert_eq!(sel.ids(), ids.as_slice());
}

#[test]
fn insert_is_idempotent() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let mut sel = Selection::new();
    assert!(sel.insert(ids[0]));
    assert!(!sel.insert(ids[0]));
    assert_eq!(sel.len(), 1);
}

#[test]
fn toggle_round_trips() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let mut sel = Selection::new();
    assert!(sel.toggle(ids[0]));
    assert!(!sel.toggle(ids[0]));
    assert_eq!(sel.len(), 0);
}

#[test]
fn replace_deduplicates() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let mut sel = Selection::new();
    sel.replace(&[ids[0], ids[0], ids[1]]);
    assert_eq!(sel.len(), 2);
}

#[test]
fn retain_live_drops_a_despawned_id() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let mut sel = Selection::new();
    sel.replace(&ids[0..3]);
    assert!(h.world_mut().entities_mut().despawn(ids[1]));
    let dropped = sel.retain_live(h.world().entities());
    assert_eq!(dropped, 1);
    assert_eq!(sel.len(), 2);
}

#[test]
fn retain_live_drops_a_stale_generation() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let a = ids[0];
    let mut sel = Selection::new();
    sel.insert(a);

    assert!(h.world_mut().entities_mut().despawn(a));
    let b = h
        .world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [0.0, 0.0])
        .expect("respawn into the freed slot");
    assert_eq!(b.index, a.index, "the free list must reuse a's slot");
    assert_ne!(b.generation, a.generation);

    let dropped = sel.retain_live(h.world().entities());
    assert_eq!(dropped, 1);
    assert!(sel.is_empty());
    assert!(!sel.contains(b), "a stale handle must not resolve to b");
}

#[test]
fn primary_is_the_lowest_slot() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    // Slots 13, 11, 15 -> ids[2], ids[0], ids[4].
    let mut sel = Selection::new();
    sel.insert(ids[2]);
    sel.insert(ids[0]);
    sel.insert(ids[4]);
    assert_eq!(sel.primary(), Some(ids[0]));
}

// --- normalise_rect / is_drag -------------------------------------------------

#[test]
fn normalise_rect_handles_every_drag_direction() {
    let want = ([10.0f32, 20.0], [50.0f32, 80.0]);
    let corners = [
        ([10.0, 20.0], [50.0, 80.0]),
        ([50.0, 80.0], [10.0, 20.0]),
        ([10.0, 80.0], [50.0, 20.0]),
        ([50.0, 20.0], [10.0, 80.0]),
    ];
    for (a, b) in corners {
        assert_eq!(normalise_rect(a, b), want, "a={a:?} b={b:?}");
    }
}

#[test]
fn is_drag_needs_four_pixels() {
    assert!(!is_drag([0.0, 0.0], [3.9, 0.0]));
    assert!(!is_drag([0.0, 0.0], [0.0, 3.9]));
    assert!(is_drag([0.0, 0.0], [DRAG_MIN_PX, 0.0]));
    assert!(is_drag([0.0, 0.0], [0.0, DRAG_MIN_PX]));
}

// --- pick_at -------------------------------------------------------------------

#[test]
fn clicking_a_worker_selects_it() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let target = ids[0];
    // Isolated well clear of the other five workers' default spawn row: the
    // 48x48 sprite rect reaches several cells past a unit's own body radius,
    // so a click meant for one worker in a tightly packed row can otherwise
    // resolve to whichever overlapping worker renders frontmost.
    set_pos(&mut h, target, [50.0, 50.0]);
    let screen = view().project(50.0, 50.0);

    let pick = h.world_mut().click_select(&view(), screen);
    assert_eq!(pick, Pick::Unit(target));
    assert_eq!(h.world().selection().ids(), &[target]);
}

#[test]
fn clicking_between_two_workers_picks_the_nearer() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    // `a` sits at the lower slot but farther from the click; `b` is the
    // higher slot but nearer. A "first hit in slot order" bug would pick
    // `a`; only a true nearest-search picks `b`.
    let (a, b) = (ids[0], ids[1]);
    set_pos(&mut h, a, [48.0, 50.0]);
    set_pos(&mut h, b, [52.0, 50.0]);

    let screen = view().project(51.6, 50.0);
    let pick = pick_at(h.world(), &view(), screen);
    assert_eq!(pick, Pick::Unit(b), "the nearer worker must win the click");
}

#[test]
fn the_pick_radius_is_the_body_radius() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let target = ids[0];
    set_pos(&mut h, target, [50.0, 50.0]);
    let r = h.world().scenario().collision_radius_cells();
    assert_eq!(r, 3.0, "the tracked scene's body radius");

    let hit_screen = view().project(50.0 + r, 50.0);
    assert_eq!(pick_at(h.world(), &view(), hit_screen), Pick::Unit(target));

    let miss_screen = view().project(50.0 + r + 0.1, 50.0);
    assert_eq!(pick_at(h.world(), &view(), miss_screen), Pick::Nothing);
}

#[test]
fn clicking_the_hq_selects_it_when_no_unit_is_near() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    // Radius-aware initial spawn (T3) can scatter a worker close enough to
    // the HQ's ground point that its own (12-cell) sprite quad reaches back
    // over it; park every worker well clear first so this case is actually
    // testing "no unit is near".
    for id in workers(&h) {
        set_pos(&mut h, id, [40.5, 40.5]);
    }
    let hq = h.world().start_hq().expect("start hq");
    let screen = view().project(166.0, 166.0);
    assert_eq!(pick_at(h.world(), &view(), screen), Pick::Building(hq));
}

#[test]
fn equal_depth_ties_go_to_the_lower_entity_slot() {
    // The HQ is entity slot 0 (spawned first); a worker parked at exactly the
    // HQ's ground point ties its depth exactly. The GPU's `GREATER` depth
    // test never lets a later-packed, equal-depth instance overwrite an
    // earlier one, so at an exact tie the earlier-packed (lower-slot) entity
    // — here, the HQ — is what is actually on top of the pixel.
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    // Park every other worker well clear — T3's radius-aware spawn can
    // otherwise leave one close enough to the HQ's ground point to outrank
    // both tied entities on depth alone, which would test nothing about the
    // tie-break this case exists to pin.
    for &id in &ids[1..] {
        set_pos(&mut h, id, [40.5, 40.5]);
    }
    let target = ids[0];
    let hq = h.world().start_hq().expect("start hq");
    set_pos(&mut h, target, [166.0, 166.0]);
    let screen = view().project(166.0, 166.0);
    assert_eq!(pick_at(h.world(), &view(), screen), Pick::Building(hq));
}

#[test]
fn frontmost_rendered_entity_wins() {
    // Two workers whose pick regions both cover the click point (adjacent
    // cells, well within the union of sprite rect and body circle) but at
    // different depths: the one with the strictly greater ground point —
    // the one rendered frontmost — must win, regardless of entity slot.
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let (back, front) = (ids[0], ids[1]);
    set_pos(&mut h, back, [100.0, 100.0]);
    set_pos(&mut h, front, [100.0, 102.0]);

    let screen = view().project(100.0, 102.0);
    assert_eq!(
        pick_at(h.world(), &view(), screen),
        Pick::Unit(front),
        "the strictly-greater-depth unit must win even though it is the higher slot"
    );
}

#[test]
fn clicking_a_node_selects_it() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    // The scenario's first crystal node cell, (140, 150) -> centre (140.5, 150.5).
    let slot = h.world().entities().slot(node).expect("live node");
    assert_eq!(h.world().entities().position(slot), [140.5, 150.5]);

    let screen = view().project(140.5, 150.5);
    assert_eq!(pick_at(h.world(), &view(), screen), Pick::Node(node));
}

#[test]
fn clicking_a_node_one_cell_off_misses_it() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let screen = view().project(141.5, 150.5);
    // One cell east of the node's centre is [948, 448]: past the bottom edge
    // of the node's sprite rect (y 446), so no exact shape covers it. Before
    // the building sprite quad became pickable this was `Pick::Nothing`; the
    // point lies inside the HQ's quad ([936, 432, 1032, 528]), so the fallback
    // tier now answers with the HQ. What this test pins is unchanged: one cell
    // off does not pick the node.
    let hq = h.world().start_hq().expect("hq");
    assert_eq!(pick_at(h.world(), &view(), screen), Pick::Building(hq));
}

#[test]
fn every_resource_quad_corner_is_pickable() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    // Spawned well clear of every scenario entity: this test is about corner
    // geometry, not about the tracked scene's node layout, and a corner near
    // a real neighbouring node would ambiguously hit either.
    let ground = [50.0, 250.0];
    let node = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Node(ResourceKind::Crystal),
            OWNER_NEUTRAL,
            ground,
        )
        .expect("spawn an isolated node");
    let rect = sprite_screen_rect(&view(), ground);
    let (x0, y0, x1, y1) = (rect[0], rect[1], rect[2], rect[3]);

    // Four points just inside each corner: all pick the node.
    for (x, y) in [
        (x0 + 1.0, y0 + 1.0),
        (x1 - 1.0, y0 + 1.0),
        (x0 + 1.0, y1 - 1.0),
        (x1 - 1.0, y1 - 1.0),
    ] {
        assert_eq!(
            pick_at(h.world(), &view(), [x, y]),
            Pick::Node(node),
            "inside corner ({x}, {y}) must hit the node"
        );
    }

    // Four points just outside each corner: all miss.
    for (x, y) in [
        (x0 - 1.0, y0 - 1.0),
        (x1 + 1.0, y0 - 1.0),
        (x0 - 1.0, y1 + 1.0),
        (x1 + 1.0, y1 + 1.0),
    ] {
        assert_eq!(
            pick_at(h.world(), &view(), [x, y]),
            Pick::Nothing,
            "outside corner ({x}, {y}) must miss"
        );
    }
}

#[test]
fn unit_pick_is_sprite_rect_union_body_circle() {
    let v = view();
    let ground = [50.0, 50.0];
    let radius = 3.0;
    let rect = sprite_screen_rect(&v, ground);

    // Rect-only: inside the sprite rect (bottom edge, far right), outside the
    // body circle.
    let rect_only = [rect[0] + 20.0, rect[3]];
    assert!(unit_pick_contains(&v, ground, radius, rect_only));

    // Circle-only: outside the rect (below the ground line), inside the
    // 3-cell body circle.
    let below_ground = v.project(ground[0] + 1.5, ground[1] + 1.5);
    assert!(
        below_ground[1] > rect[3],
        "must actually fall outside the rect"
    );
    assert!(unit_pick_contains(&v, ground, radius, below_ground));

    // Neither: far outside both.
    let neither = [rect[0] - 500.0, rect[1] - 500.0];
    assert!(!unit_pick_contains(&v, ground, radius, neither));
}

#[test]
fn entity_pick_depth_matches_the_render_ground_y() {
    let v = view();
    let nearer = entity_pick_depth(&v, [100.0, 100.0]);
    let farther = entity_pick_depth(&v, [100.0, 110.0]);
    assert!(
        farther > nearer,
        "a larger cx+cy ground point must sort deeper (frontmost)"
    );
}

#[test]
fn sprite_screen_rect_is_forty_eight_pixels_square() {
    let rect = sprite_screen_rect(&view(), [100.0, 100.0]);
    assert_eq!(rect[2] - rect[0], RTS_SPRITE_SIZE_PX[0]);
    assert_eq!(rect[3] - rect[1], RTS_SPRITE_SIZE_PX[1]);
}

#[test]
fn clicking_empty_ground_clears_the_selection() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    h.world_mut().selection_mut().replace(&ids[0..3]);
    assert_eq!(h.world().selection().len(), 3);

    let screen = view().project(10.5, 10.5);
    let pick = h.world_mut().click_select(&view(), screen);
    assert_eq!(pick, Pick::Nothing);
    assert!(h.world().selection().is_empty());
}

// --- shift click -----------------------------------------------------------

#[test]
fn shift_click_adds() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let (a, b) = (ids[0], ids[1]);
    // Isolated apart: the default spawn row packs six workers one cell
    // apart, well inside the union of sprite rect and body circle, which
    // would make each click resolve to whichever overlapping worker is
    // frontmost rather than the one the click names.
    set_pos(&mut h, a, [50.0, 50.0]);
    set_pos(&mut h, b, [80.0, 80.0]);
    let screen_a = view().project(50.0, 50.0);
    let screen_b = view().project(80.0, 80.0);

    h.world_mut().click_select(&view(), screen_a);
    h.world_mut().shift_click_select(&view(), screen_b);

    let sel = h.world().selection();
    assert!(sel.contains(a) && sel.contains(b));
    assert_eq!(sel.len(), 2);
}

#[test]
fn shift_click_on_a_selected_unit_removes_it() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let (a, b) = (ids[0], ids[1]);
    set_pos(&mut h, a, [50.0, 50.0]);
    set_pos(&mut h, b, [80.0, 80.0]);
    h.world_mut().selection_mut().replace(&[a, b]);

    let screen_a = view().project(50.0, 50.0);
    h.world_mut().shift_click_select(&view(), screen_a);

    let sel = h.world().selection();
    assert_eq!(sel.ids(), &[b]);
}

#[test]
fn shift_click_on_nothing_keeps_the_selection() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    h.world_mut().selection_mut().replace(&ids[0..3]);

    let screen = view().project(10.5, 10.5);
    let pick = h.world_mut().shift_click_select(&view(), screen);
    assert_eq!(pick, Pick::Nothing);
    assert_eq!(h.world().selection().len(), 3);
}

// --- box_select ----------------------------------------------------------------

/// Screen extent of a row of six cells (162..=167, 178). Radius-aware initial
/// spawn (T3) scatters the tracked scene's own six workers well outside this
/// box, so every test below explicitly re-parks them into it first — the box
/// itself, not the scenario's default spawn layout, is what these cases mean
/// to exercise.
fn six_worker_box() -> ([f32; 2], [f32; 2]) {
    (view().project(162.5, 178.5), view().project(167.5, 178.5))
}

/// Re-park `ids` one cell apart along `(162..=167, 178)`, matching
/// [`six_worker_box`]. Ignores body-radius overlap deliberately: box select
/// only reads ground points, never clearance, so a tight test row is a valid
/// fixture even though six live 3-cell-radius bodies could never really stand
/// there at once.
fn park_workers_in_a_row(h: &mut RtsHarness, ids: &[EntityId]) {
    for (i, &id) in ids.iter().enumerate() {
        set_pos(h, id, [162.5 + i as f32, 178.5]);
    }
}

#[test]
fn box_selects_every_own_unit_inside() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    park_workers_in_a_row(&mut h, &ids);
    let (a, b) = six_worker_box();

    let n = h.world_mut().box_select_into_selection(&view(), a, b);
    assert_eq!(n, 6);
    assert_eq!(h.world().selection().ids(), ids.as_slice());
}

#[test]
fn box_excludes_units_outside() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    park_workers_in_a_row(&mut h, &ids);
    let a = view().project(162.5, 178.5);
    let b = view().project(164.5, 178.5);

    let n = h.world_mut().box_select_into_selection(&view(), a, b);
    assert_eq!(n, 3);
    assert_eq!(h.world().selection().ids(), &ids[0..3]);
}

/// Bounding box, in screen space, of a footprint centred at `center` with
/// edge `edge` cells.
fn footprint_screen_box(view: &IsoView, center: [f32; 2], edge: u32) -> ([f32; 2], [f32; 2]) {
    let half = edge as f32 * 0.5;
    let corners = [
        view.project(center[0] - half, center[1] - half),
        view.project(center[0] + half, center[1] - half),
        view.project(center[0] - half, center[1] + half),
        view.project(center[0] + half, center[1] + half),
    ];
    let mut min = corners[0];
    let mut max = corners[0];
    for c in corners {
        min = [min[0].min(c[0]), min[1].min(c[1])];
        max = [max[0].max(c[0]), max[1].max(c[1])];
    }
    (min, max)
}

#[test]
fn box_never_selects_buildings() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let (a, b) = footprint_screen_box(&view(), [166.0, 166.0], BuildingKind::Hq.footprint_cells());

    let n = h.world_mut().box_select_into_selection(&view(), a, b);
    assert_eq!(n, 0);
    assert!(h.world().selection().is_empty());
}

#[test]
fn box_never_selects_nodes() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    // Crystal node at (140, 150), a single-cell footprint centred at (140.5, 150.5).
    let (a, b) = footprint_screen_box(&view(), [140.5, 150.5], 1);

    let n = h.world_mut().box_select_into_selection(&view(), a, b);
    assert_eq!(n, 0);
}

#[test]
fn box_ignores_a_degenerate_rectangle() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    h.world_mut().selection_mut().replace(&ids[0..2]);

    let p = view().project(162.5, 178.5);
    let n = h.world_mut().box_select_into_selection(&view(), p, p);
    assert_eq!(n, 0);
    assert!(h.world().selection().is_empty());
}

#[test]
fn box_works_dragged_in_any_direction() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    park_workers_in_a_row(&mut h, &ids);
    let (a, b) = six_worker_box();
    let corners = [
        (a, b),
        (b, a),
        ([a[0], b[1]], [b[0], a[1]]),
        ([b[0], a[1]], [a[0], b[1]]),
    ];
    for (p1, p2) in corners {
        let n = h.world_mut().box_select_into_selection(&view(), p1, p2);
        assert_eq!(n, 6, "drag {p1:?} -> {p2:?}");
        assert_eq!(h.world().selection().ids(), ids.as_slice());
    }
}

#[test]
fn box_respects_the_camera() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let mut camera = Camera::new(320, 320, 4.0, [1920.0, 1080.0], [166.0, 172.0]);
    let old_view = camera.iso_view();
    let (a, b) = (
        old_view.project(162.5, 178.5),
        old_view.project(167.5, 178.5),
    );

    camera.pan_cells(40.0, 0.0);
    let new_view = camera.iso_view();

    let n = h.world_mut().box_select_into_selection(&new_view, a, b);
    assert_eq!(n, 0, "the old screen rect must miss the panned units");
}

// --- footprint_contains / footprint_min -----------------------------------------

#[test]
fn footprint_contains_matches_the_seeded_hq() {
    let center = [166.0, 166.0];
    assert!(footprint_contains(center, 12, Cell { x: 160, y: 160 }));
    assert!(footprint_contains(center, 12, Cell { x: 171, y: 171 }));
    assert!(!footprint_contains(center, 12, Cell { x: 159, y: 160 }));
    assert!(!footprint_contains(center, 12, Cell { x: 172, y: 171 }));
}

#[test]
fn footprint_min_recovers_the_scenario_corner() {
    assert_eq!(footprint_min([166.0, 166.0], 12), Cell { x: 160, y: 160 });
}

// --- ticking + state hash -----------------------------------------------------

#[test]
fn selection_survives_a_tick() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    h.world_mut().selection_mut().replace(&ids[0..3]);

    h.step_exact(60);

    assert_eq!(h.world().selection().len(), 3);
}

#[test]
fn state_hash_sees_the_selection() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let before = h.state_hash();
    let screen = view().project(162.5, 178.5);
    let pick = h.world_mut().click_select(&view(), screen);
    assert!(matches!(pick, Pick::Unit(_)));
    assert_ne!(h.state_hash(), before);
}

// --- building pick: sprite rect union footprint --------------------------------

#[test]
fn all_building_sprite_corners_are_pickable() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    // The sprite quad is `pick_at`'s fallback tier, so any exact shape over a
    // probe point wins it — the scene's workers and its nearest crystal node
    // both reach into the HQ's quad. Move every other entity well clear so
    // this stays a test about the quad's geometry.
    let mut slots = Vec::new();
    h.world().entities().collect_live(&mut slots);
    for id in slots
        .into_iter()
        .filter_map(|slot| h.world().entities().id_at(slot))
        .filter(|&id| id != hq)
        .collect::<Vec<_>>()
    {
        set_pos(&mut h, id, [40.5, 40.5]);
    }
    let slot = h.world().entities().slot(hq).expect("live hq");
    let ground = h.world().entities().position(slot);
    let rect = building_screen_rect(&view(), ground, BuildingKind::Hq.footprint_cells());
    let (x0, y0, x1, y1) = (rect[0], rect[1], rect[2], rect[3]);

    for (x, y) in [
        (x0 + 1.0, y0 + 1.0),
        (x1 - 1.0, y0 + 1.0),
        (x0 + 1.0, y1 - 1.0),
        (x1 - 1.0, y1 - 1.0),
    ] {
        assert_eq!(
            pick_at(h.world(), &view(), [x, y]),
            Pick::Building(hq),
            "corner ({x}, {y}) inside sprite rect must pick the HQ"
        );
    }
}

#[test]
fn building_footprint_only_region_remains_pickable() {
    // A cell at the far corner of the HQ footprint projects below the sprite
    // rect bottom (the tower stands on the ground point, footprint tiles below
    // it). Clicking there must still select the building.
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    for id in workers(&h) {
        set_pos(&mut h, id, [40.5, 40.5]);
    }
    let hq = h.world().start_hq().expect("hq");
    let slot = h.world().entities().slot(hq).expect("live hq");
    let ground = h.world().entities().position(slot);
    let rect = building_screen_rect(&view(), ground, BuildingKind::Hq.footprint_cells());

    // Cell [171, 171] is the far corner of the footprint (min [160,160], edge 12).
    // Its screen centre projects below the sprite bottom.
    let screen = view().project(171.5, 171.5);
    assert!(
        screen[1] > rect[3],
        "the footprint corner must project below the sprite rect (sprite bottom {}, \
         footprint corner screen y {})",
        rect[3],
        screen[1]
    );
    assert_eq!(
        pick_at(h.world(), &view(), screen),
        Pick::Building(hq),
        "a footprint-only screen point must still pick the building"
    );
}

#[test]
fn outside_building_union_misses() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    for id in workers(&h) {
        set_pos(&mut h, id, [40.5, 40.5]);
    }
    let hq = h.world().start_hq().expect("hq");
    let slot = h.world().entities().slot(hq).expect("live hq");
    let ground = h.world().entities().position(slot);
    let rect = building_screen_rect(&view(), ground, BuildingKind::Hq.footprint_cells());

    // One cell past the far corner of the footprint — outside both sprite and footprint.
    let screen = view().project(172.5, 172.5);
    assert!(
        screen[1] > rect[3],
        "the point must be below the sprite rect"
    );
    assert_eq!(
        pick_at(h.world(), &view(), screen),
        Pick::Nothing,
        "a point outside both sprite and footprint must miss"
    );
}

#[test]
fn building_union_preserves_owner_and_depth_rules() {
    // Only player-owned buildings are clickable; neutral ones must miss.
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    for id in workers(&h) {
        set_pos(&mut h, id, [40.5, 40.5]);
    }

    let neutral_building = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Building(BuildingKind::Depot),
            OWNER_NEUTRAL,
            [50.0, 50.0],
        )
        .expect("spawn neutral depot");
    let slot = h
        .world()
        .entities()
        .slot(neutral_building)
        .expect("live depot");
    let ground = h.world().entities().position(slot);
    // Click the centre of the sprite rect — must miss because not player-owned.
    let screen = view().project(ground[0], ground[1]);
    assert_eq!(
        pick_at(h.world(), &view(), screen),
        Pick::Nothing,
        "neutral-owned building must not be pickable"
    );
}

#[test]
fn building_pick_contains_sprite_and_footprint() {
    let v = view();
    let ground = [166.0, 166.0];
    let edge = BuildingKind::Hq.footprint_cells();
    let rect = building_screen_rect(&v, ground, edge);

    // Inside sprite rect.
    let centre = [(rect[0] + rect[2]) * 0.5, (rect[1] + rect[3]) * 0.5];
    assert!(building_pick_contains(&v, ground, edge, centre));

    // Below sprite but in footprint.
    let footprint_screen = v.project(171.5, 171.5);
    assert!(footprint_screen[1] > rect[3]);
    assert!(building_pick_contains(&v, ground, edge, footprint_screen));

    // Outside both.
    let outside = v.project(172.5, 172.5);
    assert!(!building_pick_contains(&v, ground, edge, outside));
}

#[test]
fn an_exact_shape_beats_a_building_sprite_quad() {
    // The rendered building quad is a square of mostly transparent air whose
    // ground point is deeper than everything drawn behind it. As a peer of the
    // exact shapes it swallowed every node and unit standing near a building,
    // so it is a fallback tier: consulted only when no exact shape matched.
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    // Well clear of every scenario entity, which all sit near the HQ.
    let ground = [50.0, 250.0];
    let edge = BuildingKind::Depot.footprint_cells();
    let depot = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Building(BuildingKind::Depot),
            OWNER_PLAYER,
            ground,
        )
        .expect("spawn an isolated depot");
    let rect = building_screen_rect(&view(), ground, edge);

    // Sprite-only air with nothing behind it: the fallback tier still picks
    // the building, which is what "the whole rendered building is clickable"
    // bought.
    let air = [(rect[0] + rect[2]) * 0.5, rect[1] + 2.0];
    assert!(
        !building_plot_contains(&view(), ground, edge, air),
        "the probe must be sprite-only, not on the plot"
    );
    assert_eq!(
        pick_at(h.world(), &view(), air),
        Pick::Building(depot),
        "a click on sprite-only area with nothing behind it still picks the building"
    );

    // A node north of the depot: outside the footprint, but its ground point
    // projects inside the depot's quad.
    let node_ground = [46.5, 242.5];
    let node = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Node(ResourceKind::Crystal),
            OWNER_NEUTRAL,
            node_ground,
        )
        .expect("spawn a node inside the depot's quad");
    let screen = view().project(node_ground[0], node_ground[1]);
    assert!(
        screen[0] >= rect[0]
            && screen[0] <= rect[2]
            && screen[1] >= rect[1]
            && screen[1] <= rect[3],
        "the node's ground point must fall inside the depot's sprite quad ({screen:?} vs {rect:?})"
    );
    assert!(
        !building_plot_contains(&view(), ground, edge, screen),
        "the node's ground point must be outside the depot's footprint"
    );
    assert!(
        entity_pick_depth(&view(), ground) > entity_pick_depth(&view(), node_ground),
        "the depot must be the deeper candidate, or this proves nothing about tiers"
    );
    assert_eq!(
        pick_at(h.world(), &view(), screen),
        Pick::Node(node),
        "an exact node quad beats a building's sprite quad however deep the building"
    );
}
