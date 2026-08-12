//! T11 — the StarCraft-like control HUD: pure layout, appended to an already
//! packed frame.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts::hud`. The two device cases live in `gpu_smoke.rs`.

use mmd_engine::render::{
    FONT_FIRST_CHAR, FONT_LAST_CHAR, GLYPH_TRACKING_PX, GLYPH_W_PX, SLOT_RTS_BUILDINGS,
    SLOT_RTS_PROPS, SLOT_RTS_SOLDIER, SLOT_RTS_WORKER, SLOT_UI_FONT, SpriteInstance, frame_uv_rect,
    glyph_uv_rect,
};
use mmd_engine::rts::{
    BuildingKind, CommandId, DETAIL_TEXT_X, DETAIL_TEXT_Y, EntityId, EntityKind, GEAR_RECT,
    HudLayout, MULTI_ICON_COLS, MULTI_ICON_GAP_PX, MULTI_ICON_ORIGIN, MULTI_ICON_PX, NUM_BUF,
    OWNER_PLAYER, PANEL_LINE_PX, PANEL_TEXT_SCALE, PORTRAIT_POS, PORTRAIT_PX, Prop, ResourceKind,
    RtsFrame, TEXT_TINT, TEXT_TINT_BLOCKED, TOP_BAR_RECT, TOP_TEXT_SCALE, UnitKind, building_uv,
    command_slots, fmt_ratio, fmt_u32, kind_label, node_uv, pack_frame, pack_hud, prop_uv,
};
use mmd_engine::testkit::RtsHarness;

/// The view centre — matches `rts_pack.rs`'s `CURSOR`.
const CURSOR: [f32; 2] = [960.0, 540.0];

fn scene() -> RtsHarness {
    RtsHarness::scene().build().expect("rts scene harness")
}

fn workers(h: &RtsHarness) -> Vec<EntityId> {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))
}

fn group(frame: &RtsFrame, atlas_id: u32) -> &[SpriteInstance] {
    &frame
        .ui
        .iter()
        .find(|g| g.atlas_id == atlas_id)
        .unwrap_or_else(|| panic!("no ui group for slot {atlas_id}"))
        .instances
}

fn glyphs(frame: &RtsFrame) -> &[SpriteInstance] {
    group(frame, SLOT_UI_FONT)
}

fn props(frame: &RtsFrame) -> &[SpriteInstance] {
    group(frame, SLOT_RTS_PROPS)
}

/// Walk the font group's instances from `pos` in advance-width steps and map
/// each `uv_rect` back through `glyph_uv_rect` to a byte, so a test asserts
/// **the string the HUD drew**, not a pixel it hoped for. A step with no
/// matching instance reads back as `' '` — the same gap a real space leaves.
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

fn inside_rect(pos: [f32; 2], size: [f32; 2], rect: [f32; 4]) -> bool {
    pos[0] >= rect[0]
        && pos[0] + size[0] <= rect[0] + rect[2]
        && pos[1] >= rect[1]
        && pos[1] + size[1] <= rect[1] + rect[3]
}

fn disjoint(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] + a[2] <= b[0] || b[0] + b[2] <= a[0] || a[1] + a[3] <= b[1] || b[1] + b[3] <= a[1]
}

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

// --- the frame's five UI groups ----------------------------------------------

#[test]
fn rts_frame_ui_groups_are_texture_slots_4_through_8() {
    let frame = RtsFrame::new();
    let slots: Vec<u32> = frame.ui.iter().map(|g| g.atlas_id).collect();
    assert_eq!(
        slots,
        vec![
            SLOT_RTS_WORKER,
            SLOT_RTS_SOLDIER,
            SLOT_RTS_BUILDINGS,
            SLOT_RTS_PROPS,
            SLOT_UI_FONT,
        ],
        "worker, soldier, building, props, font — in texture-slot order 4..=8"
    );
}

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
fn hud_uses_only_the_five_ui_groups() {
    let mut h = scene();
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), CURSOR, None, &mut frame);
    let world_before: Vec<Vec<SpriteInstance>> =
        frame.world.iter().map(|g| g.instances.clone()).collect();
    let overlay_before = frame.overlay.clone();

    pack_hud(h.world_mut(), &mut frame);

    assert_eq!(frame.ui.len(), 5, "the UI layer stays exactly five groups");
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

// --- the top bar and gear -----------------------------------------------------

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

#[test]
fn the_gear_icon_appears_in_the_top_bar() {
    let h = scene();
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert!(
        props(&frame)
            .iter()
            .any(|i| i.pos == [GEAR_RECT[0], GEAR_RECT[1]]
                && i.size == [GEAR_RECT[2], GEAR_RECT[3]]
                && i.uv_rect == prop_uv(Prop::GearIcon)),
        "the gear icon must be drawn at its documented rect"
    );
    assert!(
        inside_rect(
            [GEAR_RECT[0], GEAR_RECT[1]],
            [GEAR_RECT[2], GEAR_RECT[3]],
            TOP_BAR_RECT
        ),
        "the gear sits inside the top bar"
    );
}

// --- the layout regions --------------------------------------------------------

#[test]
fn hud_regions_cover_bottom_without_overlap() {
    let minimap = HudLayout::MINIMAP_PANEL;
    let selection = HudLayout::SELECTION_PANEL;
    let command = HudLayout::COMMAND_PANEL;
    let bottom = HudLayout::BOTTOM_PANEL;

    for rect in [minimap, selection, command] {
        assert!(
            inside_rect([rect[0], rect[1]], [rect[2], rect[3]], bottom),
            "{rect:?} must sit inside the bottom panel"
        );
    }
    assert!(disjoint(minimap, selection), "minimap/selection overlap");
    assert!(disjoint(selection, command), "selection/command overlap");
    assert!(disjoint(minimap, command), "minimap/command overlap");

    assert!(inside_rect(
        [HudLayout::MINIMAP_MAP[0], HudLayout::MINIMAP_MAP[1]],
        [HudLayout::MINIMAP_MAP[2], HudLayout::MINIMAP_MAP[3]],
        minimap
    ));
    assert!(inside_rect(
        [HudLayout::COMMAND_GRID[0], HudLayout::COMMAND_GRID[1]],
        [HudLayout::COMMAND_GRID[2], HudLayout::COMMAND_GRID[3]],
        command
    ));
    const { assert!(TOP_BAR_RECT[1] + TOP_BAR_RECT[3] <= HudLayout::BOTTOM_PANEL[1]) };
}

// --- single selection: portrait + detail text --------------------------------

#[test]
fn single_selection_draws_portrait_and_full_details() {
    let mut h = scene();

    // Worker.
    let w = workers(&h)[0];
    h.world_mut().selection_mut().insert(w);
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert!(
        group(&frame, SLOT_RTS_WORKER)
            .iter()
            .any(|i| i.pos == PORTRAIT_POS
                && i.size == [PORTRAIT_PX, PORTRAIT_PX]
                && i.uv_rect == frame_uv_rect(6, 0)),
        "a worker's portrait must crop the worker sheet"
    );
    assert_eq!(
        text_at(&frame, [DETAIL_TEXT_X, DETAIL_TEXT_Y], PANEL_TEXT_SCALE, 20),
        "WORKER"
    );
    assert_eq!(
        text_at(
            &frame,
            [DETAIL_TEXT_X, DETAIL_TEXT_Y + PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        "CARRYING NOTHING"
    );
    h.world_mut().selection_mut().clear();

    // Soldier.
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
    assert!(
        group(&frame, SLOT_RTS_SOLDIER)
            .iter()
            .any(|i| i.pos == PORTRAIT_POS
                && i.size == [PORTRAIT_PX, PORTRAIT_PX]
                && i.uv_rect == frame_uv_rect(6, 0)),
        "a soldier's portrait must crop the soldier sheet"
    );
    assert_eq!(
        text_at(
            &frame,
            [DETAIL_TEXT_X, DETAIL_TEXT_Y + PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            10
        ),
        "IDLE"
    );
    h.world_mut().selection_mut().clear();

    // Building, finished, with a rally point.
    let hq = h.world().start_hq().expect("hq");
    assert!(
        h.world_mut()
            .set_rally(hq, Some(mmd_engine::scenario::Cell { x: 200, y: 210 }))
    );
    h.world_mut().selection_mut().insert(hq);
    let slot = h.world().entities().slot(hq).expect("live hq");
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert!(
        group(&frame, SLOT_RTS_BUILDINGS)
            .iter()
            .any(|i| i.pos == PORTRAIT_POS
                && i.size == [PORTRAIT_PX, PORTRAIT_PX]
                && i.uv_rect
                    == building_uv(
                        BuildingKind::Hq,
                        h.world().entities().progress_target(slot) > 0
                    )),
        "a building's portrait must crop the building sheet"
    );
    assert_eq!(
        text_at(
            &frame,
            [DETAIL_TEXT_X, DETAIL_TEXT_Y + PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            10
        ),
        "READY"
    );
    assert_eq!(
        text_at(
            &frame,
            [DETAIL_TEXT_X, DETAIL_TEXT_Y + 2.0 * PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        "RALLY 200,210"
    );
    h.world_mut().selection_mut().clear();

    // Resource node.
    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    h.world_mut().selection_mut().insert(node);
    let node_slot = h.world().entities().slot(node).expect("live node");
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);
    assert!(
        group(&frame, SLOT_RTS_BUILDINGS)
            .iter()
            .any(|i| i.pos == PORTRAIT_POS
                && i.size == [PORTRAIT_PX, PORTRAIT_PX]
                && i.uv_rect
                    == node_uv(
                        ResourceKind::Crystal,
                        h.world().entities().amount(node_slot) == 0
                    )),
        "a node's portrait must crop the buildings sheet's node row"
    );
    assert_eq!(
        text_at(
            &frame,
            [DETAIL_TEXT_X, DETAIL_TEXT_Y + PANEL_LINE_PX],
            PANEL_TEXT_SCALE,
            25
        ),
        "REMAINING 1500"
    );
}

// --- multi selection: sorted icon grid ----------------------------------------

#[test]
fn multi_selection_draws_first_24_sorted_icons() {
    let mut h = scene();
    let mut ids: Vec<EntityId> = Vec::new();
    for i in 0..30 {
        let x = 40.0 + (i as f32) * 2.0;
        let id = h
            .world_mut()
            .entities_mut()
            .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, [x, 40.0])
            .expect("spawn worker");
        ids.push(id);
    }
    // Insert shuffled: `Selection` keeps ascending order regardless.
    let mut shuffled = ids.clone();
    shuffled.reverse();
    for &id in &shuffled {
        h.world_mut().selection_mut().insert(id);
    }
    assert_eq!(h.world().selection().len(), 30);

    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame);

    let sorted_ids = h.world().selection().ids().to_vec();
    let first_24 = &sorted_ids[..24];
    let worker_icons = group(&frame, SLOT_RTS_WORKER);
    for (i, &id) in first_24.iter().enumerate() {
        let slot = h.world().entities().slot(id).expect("live worker");
        let _ = slot; // only the position/uv are asserted, both kind-independent here
        let row = i / MULTI_ICON_COLS;
        let col = i % MULTI_ICON_COLS;
        let pos = [
            MULTI_ICON_ORIGIN[0] + col as f32 * (MULTI_ICON_PX + MULTI_ICON_GAP_PX),
            MULTI_ICON_ORIGIN[1] + row as f32 * (MULTI_ICON_PX + MULTI_ICON_GAP_PX),
        ];
        assert!(
            worker_icons
                .iter()
                .any(|inst| inst.pos == pos && inst.size == [MULTI_ICON_PX, MULTI_ICON_PX]),
            "icon {i} missing at {pos:?}"
        );
    }
    assert_eq!(
        worker_icons.len(),
        24,
        "only the first 24 sorted ids draw an icon"
    );

    let overflow_x = MULTI_ICON_ORIGIN[0]
        + MULTI_ICON_COLS as f32 * MULTI_ICON_PX
        + (MULTI_ICON_COLS as f32 - 1.0) * MULTI_ICON_GAP_PX
        + 16.0;
    assert_eq!(
        text_at(
            &frame,
            [overflow_x, MULTI_ICON_ORIGIN[1]],
            PANEL_TEXT_SCALE,
            5
        ),
        "+6",
        "30 selected, 24 drawn, 6 overflow"
    );
}

#[test]
fn multi_selection_skips_stale_ids() {
    let mut h = scene();
    let ids = workers(&h);
    for &id in &ids[..3] {
        h.world_mut().selection_mut().insert(id);
    }
    assert!(h.world_mut().entities_mut().despawn(ids[1]));

    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame); // must not panic
    let worker_icons = group(&frame, SLOT_RTS_WORKER);
    assert_eq!(
        worker_icons.len(),
        2,
        "the despawned id must not draw an icon"
    );
}

// --- command_slots -------------------------------------------------------------

#[test]
fn worker_card_uses_stable_three_build_slots() {
    let mut h = scene();
    let ws = workers(&h);
    h.world_mut().selection_mut().insert(ws[0]);
    h.world_mut().selection_mut().insert(ws[1]);

    let slots = command_slots(h.world());
    assert_eq!(slots[0].command, Some(CommandId::BuildHq));
    assert_eq!(slots[1].command, Some(CommandId::BuildDepot));
    assert_eq!(slots[2].command, Some(CommandId::BuildBarracks));
    assert!(slots[0].enabled && slots[1].enabled && slots[2].enabled);
    for (i, s) in slots.iter().enumerate() {
        if !(0..=2).contains(&i) {
            assert_eq!(s.command, None, "slot {i} must be empty");
        }
    }
}

#[test]
fn producer_cards_show_train_and_rally() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    let slots = command_slots(h.world());
    assert_eq!(slots[0].command, Some(CommandId::TrainWorker));
    assert_eq!(slots[8].command, Some(CommandId::SetRally));
    assert!(slots[0].enabled && slots[8].enabled);
    for (i, s) in slots.iter().enumerate() {
        if i != 0 && i != 8 {
            assert_eq!(s.command, None, "slot {i} must be empty");
        }
    }
    h.world_mut().selection_mut().clear();

    let barracks = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Building(BuildingKind::Barracks),
            OWNER_PLAYER,
            [200.0, 210.0],
        )
        .expect("spawn finished barracks");
    h.world_mut().selection_mut().insert(barracks);
    let slots = command_slots(h.world());
    assert_eq!(slots[0].command, Some(CommandId::TrainSoldier));
    assert_eq!(slots[8].command, Some(CommandId::SetRally));
}

#[test]
fn mixed_or_empty_selection_disables_card() {
    let h = scene();
    assert!(h.world().selection().is_empty());
    let slots = command_slots(h.world());
    assert!(slots.iter().all(|s| s.command.is_none() && !s.enabled));

    let mut h = scene();
    let w = workers(&h)[0];
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(w);
    h.world_mut().selection_mut().insert(hq);
    let slots = command_slots(h.world());
    assert!(
        slots.iter().all(|s| s.command.is_none()),
        "a worker mixed with a building must disable the card"
    );

    // An unfinished building alone must also disable the card.
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
    h.world_mut().entities_mut().set_progress(slot, 10, 100);
    h.world_mut().selection_mut().insert(site);
    let slots = command_slots(h.world());
    assert!(
        slots.iter().all(|s| s.command.is_none()),
        "an under-construction building must disable the card"
    );
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
    assert!(
        group(&frame, SLOT_RTS_WORKER).is_empty(),
        "a stale primary draws no portrait"
    );
}

#[test]
fn pack_hud_does_not_mutate_the_world() {
    let mut h = scene();
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    assert!(
        h.world_mut()
            .set_rally(hq, Some(mmd_engine::scenario::Cell { x: 180, y: 176 }))
    );

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
