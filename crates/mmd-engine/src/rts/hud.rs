//! On-screen resource and command HUD — T13.
//!
//! Pure layout: reads world state and appends instances to the frame's UI
//! groups. No input, no device, no world mutation. Every number is formatted
//! into a fixed stack buffer, never a `String` — the HUD runs every frame and
//! a heap allocation per number would put four allocations inside the frame
//! path.
//!
//! The `overlay` layer of a `ScenePass` is only honest for procedural rings;
//! a HUD is entirely textured, so every push here lands in `frame.ui[0]`
//! (props: panels and icons) or `frame.ui[1]` (font).

use crate::render::{GLYPH_H_PX, SpriteInstance, push_text};

use super::build::{Placement, building_cost};
use super::entity::{BuildingKind, EntityKind, ResourceKind, UnitKind};
use super::pack::{Prop, RtsFrame, prop_uv};
use super::production::{PRODUCTION_QUEUE_CAP, produce_ticks};
use super::world::RtsWorld;

/// Top resource bar.
pub const TOP_BAR_RECT: [f32; 4] = [0.0, 0.0, 1920.0, 40.0]; // x, y, w, h
/// Bottom command panel.
pub const BOTTOM_PANEL_RECT: [f32; 4] = [0.0, 920.0, 1920.0, 160.0];
/// Selection block inside the bottom panel.
pub const SELECTION_RECT: [f32; 4] = [16.0, 936.0, 560.0, 128.0];
/// Production block inside the bottom panel.
pub const PRODUCTION_RECT: [f32; 4] = [608.0, 936.0, 640.0, 128.0];
/// Build-menu block inside the bottom panel.
pub const BUILD_MENU_RECT: [f32; 4] = [1280.0, 936.0, 624.0, 128.0];

/// Icon edge in the top bar.
pub const ICON_PX: f32 = 32.0;
/// Text scale in the top bar (24 px glyphs).
pub const TOP_TEXT_SCALE: f32 = 3.0;
/// Text scale in the bottom panel (16 px glyphs).
pub const PANEL_TEXT_SCALE: f32 = 2.0;
/// Line height in the bottom panel.
pub const PANEL_LINE_PX: f32 = 22.0;

/// Panel tint, premultiplied. The sheet cell already carries the alpha; this
/// keeps the tint neutral so the panel colour lives in exactly one place.
pub const PANEL_TINT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
/// Normal HUD text.
pub const TEXT_TINT: [f32; 4] = [0.90, 0.92, 0.96, 1.0];
/// Text for something the player cannot currently afford or do.
pub const TEXT_TINT_BLOCKED: [f32; 4] = [0.75, 0.28, 0.24, 1.0];
/// Text for a hotkey letter.
pub const TEXT_TINT_HOTKEY: [f32; 4] = [0.95, 0.80, 0.25, 1.0];

/// The build menu's three rows, in display order, with their hotkey letters.
///
/// The letters are the HUD's copy of the binding, and `T14` asserts they match
/// the app's keyboard table — a menu that says `Q` while the key is `B` is worse
/// than no menu.
pub const BUILD_MENU: [(u8, BuildingKind); 3] = [
    (b'Q', BuildingKind::Hq),
    (b'W', BuildingKind::Depot),
    (b'E', BuildingKind::Barracks),
];

/// Longest decimal a `u32` needs, plus room for a `/` pair.
pub const NUM_BUF: usize = 12;

/// Format `v` into `buf` and return the written slice as a `&str`.
///
/// No allocation: the HUD runs every frame and a `String` per number would put
/// four heap allocations inside the frame path.
pub fn fmt_u32(buf: &mut [u8; NUM_BUF], v: u32) -> &str {
    let mut i = NUM_BUF;
    let mut n = v;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    std::str::from_utf8(&buf[i..]).expect("ascii digits")
}

/// Format `a/b` into `buf`.
pub fn fmt_ratio(buf: &mut [u8; NUM_BUF], a: u32, b: u32) -> &str {
    let mut i = NUM_BUF;

    let mut n = b;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }

    i -= 1;
    buf[i] = b'/';

    let mut n = a;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }

    std::str::from_utf8(&buf[i..]).expect("ascii digits")
}

/// A one-word name for an entity kind, for the selection panel.
pub fn kind_label(kind: EntityKind) -> &'static str {
    match kind {
        EntityKind::Unit(UnitKind::Worker) => "WORKER",
        EntityKind::Unit(UnitKind::Soldier) => "SOLDIER",
        EntityKind::Building(BuildingKind::Hq) => "HQ",
        EntityKind::Building(BuildingKind::Depot) => "DEPOT",
        EntityKind::Building(BuildingKind::Barracks) => "BARRACKS",
        EntityKind::Node(ResourceKind::Crystal) => "CRYSTAL",
        EntityKind::Node(ResourceKind::Gas) => "GAS",
    }
}

/// One stretched `Prop::PanelFill` quad.
fn push_panel(out: &mut Vec<SpriteInstance>, rect: [f32; 4], tint: [f32; 4]) {
    out.push(SpriteInstance::new(
        [rect[0], rect[1]],
        [rect[2], rect[3]],
        prop_uv(Prop::PanelFill),
        tint,
    ));
}

/// One `ICON_PX` icon at `pos`, white — the sheet already carries the colour.
fn push_icon(out: &mut Vec<SpriteInstance>, pos: [f32; 2], prop: Prop) {
    out.push(SpriteInstance::new(
        pos,
        [ICON_PX, ICON_PX],
        prop_uv(prop),
        SpriteInstance::WHITE,
    ));
}

/// Section A: the top resource bar.
fn push_top_bar(world: &RtsWorld, props: &mut Vec<SpriteInstance>, font: &mut Vec<SpriteInstance>) {
    push_panel(props, TOP_BAR_RECT, PANEL_TINT);

    let resources = world.resources();
    let supply = world.supply();
    let mut buf = [0u8; NUM_BUF];
    let icon_y = 4.0;
    let text_y = TOP_BAR_RECT[1] + (TOP_BAR_RECT[3] - GLYPH_H_PX * TOP_TEXT_SCALE) * 0.5;

    push_icon(props, [16.0, icon_y], Prop::CrystalIcon);
    let s = fmt_u32(&mut buf, resources.crystal);
    push_text(font, s, [56.0, text_y], TOP_TEXT_SCALE, TEXT_TINT);

    push_icon(props, [280.0, icon_y], Prop::GasIcon);
    let s = fmt_u32(&mut buf, resources.gas);
    push_text(font, s, [320.0, text_y], TOP_TEXT_SCALE, TEXT_TINT);

    push_icon(props, [544.0, icon_y], Prop::SupplyIcon);
    let supply_tint = if supply.free() == 0 {
        TEXT_TINT_BLOCKED
    } else {
        TEXT_TINT
    };
    let s = fmt_ratio(&mut buf, supply.used(), supply.cap());
    push_text(font, s, [584.0, text_y], TOP_TEXT_SCALE, supply_tint);
}

/// Section C: the selection block, all five primary-kind branches.
fn push_selection_block(world: &RtsWorld, font: &mut Vec<SpriteInstance>) {
    let sel = world.selection();
    let mut buf = [0u8; NUM_BUF];
    let x = SELECTION_RECT[0];
    let mut y = SELECTION_RECT[1];

    let mut cx = x;
    cx += push_text(font, "SELECTED ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
    let s = fmt_u32(&mut buf, sel.len() as u32);
    push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);

    let Some(primary_id) = sel.primary() else {
        return;
    };
    let Some(slot) = world.entities().slot(primary_id) else {
        // A stale primary (despawned since selection) — report the count
        // above and stop rather than reading dead columns.
        return;
    };

    y += PANEL_LINE_PX;
    let kind = world.entities().kind(slot);
    push_text(font, kind_label(kind), [x, y], PANEL_TEXT_SCALE, TEXT_TINT);

    y += PANEL_LINE_PX;
    match kind {
        EntityKind::Unit(UnitKind::Worker) => match world.entities().carry(slot) {
            Some((res_kind, amount)) => {
                let mut cx = x;
                cx += push_text(font, "CARRYING ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                cx += push_text(
                    font,
                    kind_label(EntityKind::Node(res_kind)),
                    [cx, y],
                    PANEL_TEXT_SCALE,
                    TEXT_TINT,
                );
                cx += push_text(font, " ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                let s = fmt_u32(&mut buf, amount);
                push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
            }
            None => {
                push_text(
                    font,
                    "CARRYING NOTHING",
                    [x, y],
                    PANEL_TEXT_SCALE,
                    TEXT_TINT,
                );
            }
        },
        EntityKind::Unit(UnitKind::Soldier) => {
            push_text(font, "IDLE", [x, y], PANEL_TEXT_SCALE, TEXT_TINT);
        }
        EntityKind::Building(_) => {
            let target = world.entities().progress_target(slot);
            // Not a manual `checked_div`: `target == 0` is the site-vs-finished
            // business branch (READY has no percentage at all), not a guard
            // against dividing by zero.
            #[allow(clippy::manual_checked_ops)]
            if target == 0 {
                push_text(font, "READY", [x, y], PANEL_TEXT_SCALE, TEXT_TINT);
            } else {
                let progress = world.entities().progress(slot);
                let pct = progress * 100 / target;
                let mut cx = x;
                cx += push_text(font, "BUILDING ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                let s = fmt_u32(&mut buf, pct);
                cx += push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                push_text(font, "%", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
            }

            if let Some(cell) = world.rally(primary_id) {
                y += PANEL_LINE_PX;
                let mut cx = x;
                cx += push_text(font, "RALLY ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                let s = fmt_u32(&mut buf, cell.x);
                cx += push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                cx += push_text(font, ",", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
                let s = fmt_u32(&mut buf, cell.y);
                push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
            }
        }
        EntityKind::Node(_) => {
            let amount = world.entities().amount(slot);
            let mut cx = x;
            cx += push_text(font, "REMAINING ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
            let s = fmt_u32(&mut buf, amount);
            push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
        }
    }
}

/// Section D: the production block, only for a finished building with a
/// non-empty queue.
fn push_production_block(world: &RtsWorld, font: &mut Vec<SpriteInstance>) {
    let Some(primary_id) = world.selection().primary() else {
        return;
    };
    let Some(slot) = world.entities().slot(primary_id) else {
        return;
    };
    if !matches!(world.entities().kind(slot), EntityKind::Building(_)) {
        return;
    }
    if world.entities().progress_target(slot) > 0 {
        return; // still under construction
    }
    let Some(queue) = world.production_queue(primary_id) else {
        return;
    };
    let Some(head) = queue.head() else {
        return; // empty queue
    };

    let mut buf = [0u8; NUM_BUF];
    let x = PRODUCTION_RECT[0];
    let mut y = PRODUCTION_RECT[1];

    let mut cx = x;
    cx += push_text(font, "PRODUCING ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
    push_text(
        font,
        kind_label(EntityKind::Unit(head)),
        [cx, y],
        PANEL_TEXT_SCALE,
        TEXT_TINT,
    );

    y += PANEL_LINE_PX;
    let pct = queue.progress() * 100 / produce_ticks(head);
    let mut cx = x;
    let s = fmt_u32(&mut buf, pct);
    cx += push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
    push_text(font, "%", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);

    y += PANEL_LINE_PX;
    let mut cx = x;
    cx += push_text(font, "QUEUE ", [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
    let s = fmt_ratio(&mut buf, queue.len() as u32, PRODUCTION_QUEUE_CAP as u32);
    push_text(font, s, [cx, y], PANEL_TEXT_SCALE, TEXT_TINT);
}

/// Section E: the build menu, always, one row per [`BUILD_MENU`] entry.
fn push_build_menu(world: &RtsWorld, font: &mut Vec<SpriteInstance>) {
    let pending_kind = match world.placement() {
        Placement::Pending { kind } => Some(kind),
        Placement::None => None,
    };
    let resources = world.resources();
    let mut buf = [0u8; NUM_BUF];

    for (i, (letter, kind)) in BUILD_MENU.into_iter().enumerate() {
        let y = BUILD_MENU_RECT[1] + i as f32 * PANEL_LINE_PX;
        let cost = building_cost(kind);
        let affordable = resources.covers(cost);
        let tint = if affordable {
            TEXT_TINT
        } else {
            TEXT_TINT_BLOCKED
        };
        let prefix = if pending_kind == Some(kind) { ">" } else { " " };

        let mut cx = BUILD_MENU_RECT[0];
        cx += push_text(font, prefix, [cx, y], PANEL_TEXT_SCALE, tint);
        cx += push_text(font, "[", [cx, y], PANEL_TEXT_SCALE, tint);
        let letter_buf = [letter];
        let letter_str = std::str::from_utf8(&letter_buf).expect("ascii hotkey letter");
        cx += push_text(
            font,
            letter_str,
            [cx, y],
            PANEL_TEXT_SCALE,
            TEXT_TINT_HOTKEY,
        );
        cx += push_text(font, "] ", [cx, y], PANEL_TEXT_SCALE, tint);
        cx += push_text(
            font,
            kind_label(EntityKind::Building(kind)),
            [cx, y],
            PANEL_TEXT_SCALE,
            tint,
        );
        cx += push_text(font, " ", [cx, y], PANEL_TEXT_SCALE, tint);
        let s = fmt_u32(&mut buf, cost.crystal);
        cx += push_text(font, s, [cx, y], PANEL_TEXT_SCALE, tint);
        cx += push_text(font, "C", [cx, y], PANEL_TEXT_SCALE, tint);
        if cost.gas != 0 {
            cx += push_text(font, " ", [cx, y], PANEL_TEXT_SCALE, tint);
            let s = fmt_u32(&mut buf, cost.gas);
            cx += push_text(font, s, [cx, y], PANEL_TEXT_SCALE, tint);
            push_text(font, "G", [cx, y], PANEL_TEXT_SCALE, tint);
        }
    }
}

/// Append the whole HUD to `frame`.
///
/// Appends to `frame.ui[0]` (props: panels and icons) and `frame.ui[1]` (font).
/// Call **after** [`super::pack_frame`], which owns the world-space half of the
/// UI layer. Allocation-free.
pub fn pack_hud(world: &RtsWorld, frame: &mut RtsFrame) {
    // A. Top bar.
    {
        // Split the borrow once: props and font never need to be mutably
        // borrowed at the same instant, but the two indices into `frame.ui`
        // are taken together to keep every section's call sites uniform.
        let (props_slice, font_slice) = frame.ui.split_at_mut(1);
        push_top_bar(
            world,
            &mut props_slice[0].instances,
            &mut font_slice[0].instances,
        );
    }

    // B. Bottom panel.
    push_panel(&mut frame.ui[0].instances, BOTTOM_PANEL_RECT, PANEL_TINT);

    // C. Selection block.
    push_selection_block(world, &mut frame.ui[1].instances);

    // D. Production block.
    push_production_block(world, &mut frame.ui[1].instances);

    // E. Build menu.
    push_build_menu(world, &mut frame.ui[1].instances);
}
