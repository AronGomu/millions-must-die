//! T8 — selection: turning a screen-space pointer gesture into a set of
//! entity handles.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts::selection` and `testkit::RtsHarness`.

use mmd_engine::render::{Camera, IsoView};
use mmd_engine::rts::{
    BuildingKind, DRAG_MIN_PX, EntityId, EntityKind, OWNER_PLAYER, Pick, ResourceKind, Selection,
    UnitKind, footprint_contains, footprint_min, is_drag, normalise_rect, pick_at,
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
    let target = ids[0]; // slot 11, at (162.5, 178.5)
    let screen = view().project(162.5, 178.5);

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
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("start hq");
    let screen = view().project(166.0, 166.0);
    assert_eq!(pick_at(h.world(), &view(), screen), Pick::Building(hq));
}

#[test]
fn a_worker_standing_on_the_hq_wins_the_click() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let target = ids[0];
    set_pos(&mut h, target, [166.0, 166.0]);
    let screen = view().project(166.0, 166.0);
    assert_eq!(pick_at(h.world(), &view(), screen), Pick::Unit(target));
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
    assert_eq!(pick_at(h.world(), &view(), screen), Pick::Nothing);
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
    let screen_a = view().project(162.5, 178.5);
    let screen_b = view().project(163.5, 178.5);

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
    h.world_mut().selection_mut().replace(&[a, b]);

    let screen_a = view().project(162.5, 178.5);
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

/// Screen extent of the six spawn-cell workers (162..=167, 178).
fn six_worker_box() -> ([f32; 2], [f32; 2]) {
    (view().project(162.5, 178.5), view().project(167.5, 178.5))
}

#[test]
fn box_selects_every_own_unit_inside() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
    let (a, b) = six_worker_box();

    let n = h.world_mut().box_select_into_selection(&view(), a, b);
    assert_eq!(n, 6);
    assert_eq!(h.world().selection().ids(), ids.as_slice());
}

#[test]
fn box_excludes_units_outside() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let ids = workers(&h);
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
