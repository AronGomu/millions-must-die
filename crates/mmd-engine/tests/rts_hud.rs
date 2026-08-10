//! T13 — the on-screen HUD: pure layout, appended to an already-packed frame.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts::hud`. The two device cases live in `gpu_smoke.rs`.

use mmd_engine::render::{
    FONT_FIRST_CHAR, FONT_LAST_CHAR, GLYPH_TRACKING_PX, GLYPH_W_PX, SpriteInstance, glyph_uv_rect,
};
use mmd_engine::rts::{
    BUILD_MENU, BuildingKind, EntityId, EntityKind, NUM_BUF, OWNER_PLAYER, PANEL_LINE_PX,
    PANEL_TEXT_SCALE, PRODUCTION_QUEUE_CAP, Prop, ResourceKind, RtsFrame, SELECTION_RECT,
    TEXT_TINT, TEXT_TINT_BLOCKED, TEXT_TINT_HOTKEY, TOP_BAR_RECT, TOP_TEXT_SCALE, UnitKind,
    fmt_ratio, fmt_u32, kind_label, pack_frame, pack_hud, prop_uv,
};
use mmd_engine::scenario::Cell;
use mmd_engine::testkit::RtsHarness;

use mmd_engine::rts::{BOTTOM_PANEL_RECT, BUILD_MENU_RECT, PRODUCTION_RECT};

/// The view centre — matches `rts_pack.rs`'s `CURSOR`.
const CURSOR: [f32; 2] = [960.0, 540.0];

fn scene() -> RtsHarness {
    RtsHarness::scene().build().expect("rts scene harness")
}

fn workers(h: &RtsHarness) -> Vec<EntityId> {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))
}

/// The font group's instances — the only place `pack_hud`'s text lands.
fn glyphs(frame: &RtsFrame) -> &[SpriteInstance] {
    &frame.ui[1].instances
}

/// The prop group's instances — panels and icons.
fn props(frame: &RtsFrame) -> &[SpriteInstance] {
    &frame.ui[0].instances
}

/// Walk the font group's instances from `pos` in advance-width steps and map
/// each `uv_rect` back through `glyph_uv_rect` to a byte, so a test asserts
/// **the string the HUD drew**, not a pixel it hoped for. A step with no
/// matching instance reads back as `' '` — the same gap a real space leaves.
///
/// `max_len` bounds the walk so a read never wanders past its own block into
/// a neighbouring one at the same `y`; every call site below passes the
/// widest value that still stays inside the block being asserted on.
fn text_at(frame: &RtsFrame, pos: [f32; 2], scale: f32, max_len: usize) -> String {
    let advance = (GLYPH_W_PX + GLYPH_TRACKING_PX) * scale;
    let all = glyphs(frame);
    let mut out = String::new();
    let mut cx = pos[0];
    for _ in 0..max_len {
        match all.iter().find(|inst| inst.pos == [cx, pos[1]]) {
            Some(inst) => out.push(byte_for_uv(inst.uv_rect) as char),
            None => out.push(' '),
        }
        cx += advance;
    }
    out.trim_end().to_string()
}

/// The tint of the glyph instance whose top-left is exactly `pos`, if any.
fn tint_at(frame: &RtsFrame, pos: [f32; 2]) -> Option<[f32; 4]> {
    glyphs(frame)
        .iter()
        .find(|inst| inst.pos == pos)
        .map(|inst| inst.tint)
}

fn byte_for_uv(uv: [f32; 4]) -> u8 {
    for b in FONT_FIRST_CHAR..=FONT_LAST_CHAR {
        if glyph_uv_rect(b) == uv {
            return b;
        }
    }
    panic!("uv_rect {uv:?} matches no real glyph cell");
}

/// Advance of one panel-scale glyph — used to skip the build menu's leading
/// pending marker when a test wants the row's own text.
const PANEL_ADVANCE: f32 = GLYPH_W_PX * PANEL_TEXT_SCALE; // tracking is 0.0

// --- fmt_u32 / fmt_ratio / kind_label ----------------------------------------

#[test]
fn fmt_u32_covers_zero_and_the_max() {
    let mut buf = [0u8; NUM_BUF];
    assert_eq!(fmt_u32(&mut buf, 0), "0");
    assert_eq!(fmt_u32(&mut buf, 1), "1");
    assert_eq!(fmt_u32(&mut buf, 4_294_967_295), "4294967295");
}

#[test]
fn fmt_ratio_joins_with_a_slash() {
    let mut buf = [0u8; NUM_BUF];
    assert_eq!(fmt_ratio(&mut buf, 6, 10), "6/10");
}

#[test]
fn kind_label_covers_every_kind() {
    let kinds = [
        EntityKind::Unit(UnitKind::Worker),
        EntityKind::Unit(UnitKind::Soldier),
        EntityKind::Building(BuildingKind::Hq),
        EntityKind::Building(BuildingKind::Depot),
        EntityKind::Building(BuildingKind::Barracks),
        EntityKind::Node(ResourceKind::Crystal),
        EntityKind::Node(ResourceKind::Gas),
    ];
    let mut labels: Vec<&str> = kinds.iter().map(|&k| kind_label(k)).collect();
    for l in &labels {
        assert!(!l.is_empty());
        assert_eq!(*l, l.to_uppercase());
    }
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), 7, "every kind must have a distinct label");
}

// --- append, don't clear; only the two UI groups -----------------------------

#[test]
fn hud_appends_and_does_not_clear() {
    let mut h = scene();
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let before: Vec<SpriteInstance> = props(&frame).to_vec();
    assert!(!before.is_empty(), "the ghost must have packed something");

    pack_hud(h.world(), &mut frame);
    let after = props(&frame);
    assert!(
        after.len() > before.len(),
        "pack_hud must append, not replace"
    );
    assert_eq!(
        &after[..before.len()],
        &before[..],
        "pack_frame's own instances must survive pack_hud"
    );
}

#[test]
fn hud_uses_only_the_two_ui_groups() {
    let mut h = scene();
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let world_before: Vec<Vec<SpriteInstance>> =
        frame.world.iter().map(|g| g.instances.clone()).collect();
    let overlay_before = frame.overlay.clone();

    pack_hud(h.world_mut(), &mut frame);

    assert_eq!(frame.ui.len(), 2, "the UI layer stays exactly two groups");
    let world_after: Vec<Vec<SpriteInstance>> =
        frame.world.iter().map(|g| g.instances.clone()).collect();
    assert_eq!(
        world_before, world_after,
        "the world layer must be untouched"
    );
    assert_eq!(
        frame.overlay, overlay_before,
        "the overlay must be untouched"
    );
}

#[test]
fn the_panels_are_drawn_before_the_text() {
    let mut h = scene();
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    pack_hud(h.world_mut(), &mut frame);

    let panel_uv = prop_uv(Prop::PanelFill);
    let panel_count = props(&frame)
        .iter()
        .filter(|i| i.uv_rect == panel_uv)
        .count();
    assert_eq!(panel_count, 2, "top bar + bottom panel, exactly two fills");

    // Every glyph really is in ui[1], which the renderer draws after ui[0].
    assert!(!glyphs(&frame).is_empty());
    assert_eq!(frame.ui[1].instances.len(), glyphs(&frame).len());
}

// --- the top bar --------------------------------------------------------------

#[test]
fn the_top_bar_shows_the_stock() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    let text_y = TOP_BAR_RECT[1] + (TOP_BAR_RECT[3] - 8.0 * TOP_TEXT_SCALE) * 0.5;
    assert_eq!(
        text_at(&frame, [56.0, text_y], TOP_TEXT_SCALE, 10),
        "300",
        "the tracked scene opens with 300 crystal"
    );
    assert_eq!(
        text_at(&frame, [320.0, text_y], TOP_TEXT_SCALE, 10),
        "100",
        "the tracked scene opens with 100 gas"
    );
}

#[test]
fn the_top_bar_shows_supply_as_a_ratio() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    let text_y = TOP_BAR_RECT[1] + (TOP_BAR_RECT[3] - 8.0 * TOP_TEXT_SCALE) * 0.5;
    assert_eq!(
        text_at(&frame, [584.0, text_y], TOP_TEXT_SCALE, 10),
        "6/10",
        "six spawn workers, a ten-supply HQ"
    );
}

#[test]
fn supply_turns_red_at_the_cap() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    // Used starts at 6/10; four more Workers close the gap without a tick.
    for _ in 0..4 {
        assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    }
    assert_eq!(h.world().supply().free(), 0);

    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    let text_y = TOP_BAR_RECT[1] + (TOP_BAR_RECT[3] - 8.0 * TOP_TEXT_SCALE) * 0.5;
    assert_eq!(
        tint_at(&frame, [584.0, text_y]),
        Some(TEXT_TINT_BLOCKED),
        "the supply ratio's first glyph must carry the blocked tint at the cap"
    );
}

#[test]
fn supply_is_normal_below_the_cap() {
    let h = scene();
    assert!(h.world().supply().free() > 0);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    let text_y = TOP_BAR_RECT[1] + (TOP_BAR_RECT[3] - 8.0 * TOP_TEXT_SCALE) * 0.5;
    assert_eq!(tint_at(&frame, [584.0, text_y]), Some(TEXT_TINT));
}

// --- the selection block -------------------------------------------------------

#[test]
fn an_empty_selection_says_zero() {
    let h = scene();
    assert!(h.world().selection().is_empty());
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1]],
            PANEL_TEXT_SCALE,
            20
        ),
        "SELECTED 0"
    );
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1] + PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            20
        ),
        "",
        "an empty selection draws no line 1"
    );
}

#[test]
fn a_selected_worker_names_itself() {
    let mut h = scene();
    let w = workers(&h)[0];
    h.world_mut().selection_mut().insert(w);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1] + PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            20
        ),
        "WORKER"
    );
}

#[test]
fn a_carrying_worker_reports_its_cargo() {
    let mut h = scene();
    let w = workers(&h)[0];
    let slot = h.world().entities().slot(w).expect("live worker");
    h.world_mut()
        .entities_mut()
        .set_carry(slot, Some((ResourceKind::Gas, 8)));
    h.world_mut().selection_mut().insert(w);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1] + 2.0 * PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        "CARRYING GAS 8"
    );
}

#[test]
fn an_empty_handed_worker_says_so() {
    let mut h = scene();
    let w = workers(&h)[0];
    assert!(
        h.world()
            .entities()
            .carry(h.world().entities().slot(w).unwrap())
            .is_none()
    );
    h.world_mut().selection_mut().insert(w);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1] + 2.0 * PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        "CARRYING NOTHING"
    );
}

#[test]
fn a_selected_soldier_is_idle() {
    let mut h = scene();
    let soldier = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Soldier),
            OWNER_PLAYER,
            [166.5, 172.5],
        )
        .expect("spawn soldier");
    h.world_mut().selection_mut().insert(soldier);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1] + 2.0 * PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        "IDLE"
    );
}

#[test]
fn a_selected_site_reports_its_percentage() {
    let mut h = scene();
    let site = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Building(BuildingKind::Depot),
            OWNER_PLAYER,
            [180.0, 176.0],
        )
        .expect("spawn depot site");
    let slot = h.world().entities().slot(site).expect("live site");
    h.world_mut().entities_mut().set_progress(slot, 90, 180);
    h.world_mut().selection_mut().insert(site);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1] + 2.0 * PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        "BUILDING 50%"
    );
}

#[test]
fn a_finished_building_says_ready() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1] + 2.0 * PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        "READY"
    );
}

#[test]
fn a_selected_node_reports_its_remainder() {
    let mut h = scene();
    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    h.world_mut().selection_mut().insert(node);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1] + 2.0 * PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        "REMAINING 1500"
    );
}

#[test]
fn a_rally_point_is_shown() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().set_rally(hq, Some(Cell { x: 200, y: 210 })));
    h.world_mut().selection_mut().insert(hq);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1] + 3.0 * PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        "RALLY 200,210"
    );
}

#[test]
fn no_rally_means_no_line_three() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world().rally(hq).is_none());
    h.world_mut().selection_mut().insert(hq);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1] + 3.0 * PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        ""
    );
}

// --- the production block ------------------------------------------------------

fn inside_rect(pos: [f32; 2], rect: [f32; 4]) -> bool {
    pos[0] >= rect[0]
        && pos[0] < rect[0] + rect[2]
        && pos[1] >= rect[1]
        && pos[1] < rect[1] + rect[3]
}

#[test]
fn the_production_block_is_absent_without_a_queue() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    assert!(h.world().production_queue(hq).expect("hq queue").is_empty());
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert!(
        glyphs(&frame)
            .iter()
            .all(|g| !inside_rect(g.pos, PRODUCTION_RECT)),
        "no queue must mean no production block glyphs"
    );
}

#[test]
fn the_production_block_reports_the_head() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    h.step_exact(150);

    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert_eq!(
        text_at(
            &frame,
            [PRODUCTION_RECT[0], PRODUCTION_RECT[1]],
            PANEL_TEXT_SCALE,
            25
        ),
        "PRODUCING WORKER"
    );
    assert_eq!(
        text_at(
            &frame,
            [PRODUCTION_RECT[0], PRODUCTION_RECT[1] + PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            10
        ),
        "50%"
    );
    assert_eq!(
        text_at(
            &frame,
            [PRODUCTION_RECT[0], PRODUCTION_RECT[1] + 2.0 * PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            15
        ),
        format!("QUEUE 1/{PRODUCTION_QUEUE_CAP}")
    );
}

// --- the build menu --------------------------------------------------------------

#[test]
fn the_build_menu_lists_three_rows() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    for i in 0..3 {
        let y = BUILD_MENU_RECT[1] + i as f32 * PANEL_LINE_PX;
        let s = text_at(
            &frame,
            [BUILD_MENU_RECT[0] + PANEL_ADVANCE, y],
            PANEL_TEXT_SCALE,
            25,
        );
        assert!(!s.is_empty(), "row {i} must draw something");
    }
}

#[test]
fn the_build_menu_shows_costs() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    let rows = [
        (0, "[Q] HQ 400C"),
        (1, "[W] DEPOT 100C"),
        (2, "[E] BARRACKS 150C 25G"),
    ];
    for (i, want) in rows {
        let y = BUILD_MENU_RECT[1] + i as f32 * PANEL_LINE_PX;
        let got = text_at(
            &frame,
            [BUILD_MENU_RECT[0] + PANEL_ADVANCE, y],
            PANEL_TEXT_SCALE,
            25,
        );
        assert_eq!(got, want, "row {i}");
    }
}

#[test]
fn an_unaffordable_row_is_red() {
    let h = scene();
    assert_eq!(
        h.world().resources().crystal,
        300,
        "HQ costs 400, must be short"
    );
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    let hq_row_y = BUILD_MENU_RECT[1];
    let depot_row_y = BUILD_MENU_RECT[1] + PANEL_LINE_PX;
    // The "[" glyph, one advance past the prefix.
    assert_eq!(
        tint_at(&frame, [BUILD_MENU_RECT[0] + PANEL_ADVANCE, hq_row_y]),
        Some(TEXT_TINT_BLOCKED)
    );
    assert_eq!(
        tint_at(&frame, [BUILD_MENU_RECT[0] + PANEL_ADVANCE, depot_row_y]),
        Some(TEXT_TINT)
    );
}

#[test]
fn the_hotkey_letter_is_tinted_separately() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    for (i, (_, _)) in BUILD_MENU.into_iter().enumerate() {
        let y = BUILD_MENU_RECT[1] + i as f32 * PANEL_LINE_PX;
        // prefix, "[", hotkey — two advances past the row start.
        let pos = [BUILD_MENU_RECT[0] + 2.0 * PANEL_ADVANCE, y];
        assert_eq!(tint_at(&frame, pos), Some(TEXT_TINT_HOTKEY), "row {i}");
    }
}

#[test]
fn the_pending_row_is_marked() {
    let mut h = scene();
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    for (i, (_, kind)) in BUILD_MENU.into_iter().enumerate() {
        let y = BUILD_MENU_RECT[1] + i as f32 * PANEL_LINE_PX;
        let marker = text_at(&frame, [BUILD_MENU_RECT[0], y], PANEL_TEXT_SCALE, 1);
        if kind == BuildingKind::Depot {
            assert_eq!(marker, ">", "row {i} (the pending kind)");
        } else {
            assert_eq!(marker, "", "row {i} must read back as a blank prefix");
        }
    }
}

// --- resilience and purity -----------------------------------------------------

#[test]
fn the_hud_never_panics_on_a_stale_primary() {
    let mut h = scene();
    let w = workers(&h)[0];
    h.world_mut().selection_mut().insert(w);
    assert!(h.world_mut().entities_mut().despawn(w));

    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame); // must not panic
    assert_eq!(
        text_at(
            &frame,
            [SELECTION_RECT[0], SELECTION_RECT[1]],
            PANEL_TEXT_SCALE,
            20
        ),
        "SELECTED 1",
        "the stale id is still counted; only reading its dead columns is skipped"
    );
}

#[test]
fn pack_hud_does_not_mutate_the_world() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    assert!(h.world_mut().set_rally(hq, Some(Cell { x: 180, y: 176 })));

    let before = h.state_hash();
    let mut frame = RtsFrame::new();
    for _ in 0..100 {
        pack_frame(h.world(), CURSOR, None, &mut frame);
        pack_hud(h.world(), &mut frame);
    }
    assert_eq!(
        h.state_hash(),
        before,
        "packing the HUD is a read of world state"
    );
}

#[test]
fn bottom_panel_rect_is_covered_by_the_selection_and_build_blocks() {
    // Cheap sanity pin on the constants themselves, not on pack_hud: every
    // block referenced by the design lives inside the bottom panel, and the
    // top bar is a distinct region above it — a layout regression here would
    // otherwise only be caught by eyeballing a screenshot.
    assert!(inside_rect(
        [SELECTION_RECT[0], SELECTION_RECT[1]],
        BOTTOM_PANEL_RECT
    ));
    assert!(inside_rect(
        [PRODUCTION_RECT[0], PRODUCTION_RECT[1]],
        BOTTOM_PANEL_RECT
    ));
    assert!(inside_rect(
        [BUILD_MENU_RECT[0], BUILD_MENU_RECT[1]],
        BOTTOM_PANEL_RECT
    ));
    const { assert!(TOP_BAR_RECT[1] + TOP_BAR_RECT[3] <= BOTTOM_PANEL_RECT[1]) };
}
