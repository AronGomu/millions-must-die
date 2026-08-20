//! Combat T5 — the Turret: the first armed building.
//!
//! Headless CPU checks through `testkit::RtsHarness` on the tracked scene
//! (320×320, HQ min corner (160,160), 300 starting crystal). Enemies are
//! raw testkit spawns; T3's enemy AI marches every Idle Ghoul at the HQ,
//! so single-tick assertions read positions *as spawned* (the combat scan
//! runs before movement) and longer windows tolerate the 0.3 cells/tick
//! drift with generous bounds.

use mmd_engine::rts::{
    BUILD_MENU, BuildingKind, COMMAND_SLOT_KEYS, CommandId, DEPOT_BUILD_TICKS, DamageResult,
    EntityId, EntityKind, GatherPhase, OWNER_ENEMY, OWNER_PLAYER, Order, PlacementError,
    ResourceKind, Resources, TURRET_ARMOR, TURRET_BUILD_TICKS, TURRET_COST, TURRET_MAX_HP,
    TURRET_SUPPLY_GRANT, UnitKind, armor, build_ticks, building_cost, building_weapon,
    command_slots, max_hp, placement_valid, supply_grant,
};
use mmd_engine::scenario::{Cell, TURRET_FOOTPRINT_CELLS};
use mmd_engine::testkit::RtsHarness;

/// Obstacle-free, build-square-aligned 8×8 corner of the tracked scene
/// (verified against the RON's obstacle and node lists), north of the
/// 24-cell HQ and more than one body radius clear of it.
const TURRET_CORNER: Cell = Cell { x: 160, y: 144 };
/// A building spawns centred at `min + edge/2`, so this turret's centre.
/// Its footprint rectangle spans x 160..168, y 144..152.
const TURRET_CENTER_CELL: (u32, u32) = (164, 148);
/// Obstacle-free 8×8 corner whose finished turret sits 4.5 cells from
/// the first crystal node (140,150) — the HQ is ~41 away, so a
/// drop-off bug would pick the turret.
const NODE_SIDE_CORNER: Cell = Cell { x: 128, y: 144 };

/// Where the twelve ghouls of [`ghouls_kill_turret`] start: a ring around
/// the turret at [`TURRET_CORNER`].
///
/// Why a ring and not a column: the tracked scene carries a diagonal
/// obstacle lattice, and a body of radius 3 is barred from every position
/// within 3.0 cells of an obstacle cell rectangle — whole lanes are
/// impassable, and a column of attackers strands itself out of its own
/// 8-cell reach. Every position below is instead derived against four
/// rules: at least 3.0 cells clear of the turret's stamped rectangle
/// (x 160..168, y 144..152), of every obstacle and node rectangle, and of
/// the 24-cell HQ (x 160..184, y 160..184); and at least 6 cells from the
/// next spawn (radius 3, so no two bodies touch — the closest pair here is
/// 6.5). Their footprint distances run 4.5 to 12.5 cells, so every one of
/// the twelve is inside or a few steps from the 8-cell ghoul reach and the
/// whole ring engages.
const GHOUL_RING: [[f32; 2]; 12] = [
    [180.5, 148.5],
    [173.5, 153.5],
    [167.5, 156.5],
    [160.5, 156.5],
    [155.5, 162.5],
    [154.5, 153.5],
    [150.5, 147.5],
    [149.5, 139.5],
    [158.5, 138.5],
    [163.5, 131.5],
    [169.5, 138.5],
    [175.5, 141.5],
];

fn first_worker(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0]
}

fn hp_of(h: &RtsHarness, id: EntityId) -> u32 {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().hp(slot)
}

/// Raw testkit Ghoul spawn. Arms the overlap-repair pass; every position
/// in this file is ≥ 7 cells from any other body, so the pass finds
/// nothing to repair.
fn spawn_ghoul(h: &mut RtsHarness, pos: [f32; 2]) -> EntityId {
    h.world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, pos)
        .expect("store has room")
}

/// Place and fully attend a turret until it finishes. Generous bound:
/// the builder walks in first (2000 ≫ walk + 180 attended ticks).
fn build_and_finish_turret(h: &mut RtsHarness, corner: Cell, builder: EntityId) -> EntityId {
    assert!(h.world_mut().begin_placement(BuildingKind::Turret));
    let site = h
        .world_mut()
        .confirm_placement(corner, builder)
        .expect("confirm turret placement");
    h.step_exact(2_000);
    assert!(
        !h.world().is_site(site),
        "turret must finish within 2000 ticks"
    );
    site
}

/// Fresh scene, finished turret at [`TURRET_CORNER`], one Ghoul at
/// `pos`, one tick: the Ghoul's HP after the turret's first scan (which
/// reads the spawn position — combat runs before movement).
fn fire_probe(pos: [f32; 2]) -> u32 {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    build_and_finish_turret(&mut h, TURRET_CORNER, w0);
    let g = spawn_ghoul(&mut h, pos);
    h.step_exact(1);
    hp_of(&h, g)
}

// --- published surface ---------------------------------------------------

#[test]
fn turret_stats_cost_and_footprint_are_published() {
    assert_eq!(EntityKind::Building(BuildingKind::Turret).tag(), 0x23);
    assert_eq!(TURRET_FOOTPRINT_CELLS, 8);
    assert_eq!(
        BuildingKind::Turret.footprint_cells(),
        TURRET_FOOTPRINT_CELLS
    );
    assert_eq!(
        max_hp(EntityKind::Building(BuildingKind::Turret)),
        TURRET_MAX_HP
    );
    assert_eq!(TURRET_MAX_HP, 150);
    assert_eq!(
        armor(EntityKind::Building(BuildingKind::Turret)),
        TURRET_ARMOR
    );
    assert_eq!(TURRET_ARMOR, 1);
    assert_eq!(building_cost(BuildingKind::Turret), TURRET_COST);
    assert_eq!(
        TURRET_COST,
        Resources {
            crystal: 75,
            gas: 0
        }
    );
    assert_eq!(build_ticks(BuildingKind::Turret), TURRET_BUILD_TICKS);
    assert_eq!(
        TURRET_BUILD_TICKS, DEPOT_BUILD_TICKS,
        "deliberately the Depot's duration; retuning one must be a choice"
    );
    assert_eq!(TURRET_BUILD_TICKS, 180);
    assert_eq!(supply_grant(BuildingKind::Turret), TURRET_SUPPLY_GRANT);
    assert_eq!(TURRET_SUPPLY_GRANT, 0);
    assert!(!BuildingKind::Turret.is_drop_off());
}

#[test]
fn building_weapon_arms_only_the_turret() {
    let w = building_weapon(BuildingKind::Turret).expect("turret is armed");
    assert_eq!(w.damage, 10);
    assert_eq!(w.cooldown_ticks, 20);
    assert_eq!(w.range_cells, 36.0);
    assert!(building_weapon(BuildingKind::Hq).is_none());
    assert!(building_weapon(BuildingKind::Depot).is_none());
    assert!(building_weapon(BuildingKind::Barracks).is_none());
}

// --- placement and construction ------------------------------------------

#[test]
fn turret_placeable_under_four_rules() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    // Rule order is the existing one; each probe trips exactly one rule.
    assert!(placement_valid(h.world(), BuildingKind::Turret, TURRET_CORNER).is_ok());
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Turret, Cell { x: 315, y: 315 }),
        Err(PlacementError::OutOfBounds),
        "315 + 6 leaves the 320 grid"
    );
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Turret, Cell { x: 0, y: 0 }),
        Err(PlacementError::BlockedTerrain { x: 0, y: 0 }),
        "the tracked scene's obstacle at flat index 0"
    );
    // A *site* is not in the stamped mask, so rule 2 passes and rule 3
    // must catch the overlap. Raw-spawn a Depot site (centre = its
    // min (198,176) + 4) on terrain-verified clear ground; no tick runs,
    // so arming the repair pass is inert.
    let site = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Building(BuildingKind::Depot),
            OWNER_PLAYER,
            [202.0, 180.0],
        )
        .expect("store has room");
    let s = h.world().entities().slot(site).expect("live site");
    h.world_mut()
        .entities_mut()
        .set_progress(s, 10, DEPOT_BUILD_TICKS);
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Turret, Cell { x: 200, y: 178 }),
        Err(PlacementError::OverlapsBuilding)
    );
    assert_eq!(
        placement_valid(h.world(), BuildingKind::Turret, Cell { x: 136, y: 144 }),
        Err(PlacementError::CoversNode { x: 140, y: 150 }),
        "the first crystal node sits inside the probe footprint"
    );
}

#[test]
fn turret_costs_75_and_builds() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let crystal_before = h.world().resources().crystal;
    let gas_before = h.world().resources().gas;
    let cap_before = h.world().supply().cap();

    assert!(h.world_mut().begin_placement(BuildingKind::Turret));
    let site = h
        .world_mut()
        .confirm_placement(TURRET_CORNER, w0)
        .expect("confirm turret placement");
    assert_eq!(h.world().resources().crystal, crystal_before - 75);
    assert_eq!(h.world().resources().gas, gas_before);
    assert!(h.world().is_site(site));

    h.step_exact(2_000);
    assert!(!h.world().is_site(site), "must finish within 2000 ticks");
    let w = h.world().static_nav().width();
    let (cx, cy) = TURRET_CENTER_CELL;
    let idx = (cx + cy * w) as usize;
    assert!(
        h.world().static_nav().placement_solids()[idx],
        "finished footprint is stamped"
    );
    assert!(
        h.world().nav().blocked()[idx],
        "the pool mask followed the stamp (fields invalidated)"
    );
    assert_eq!(h.world().supply().cap(), cap_before, "no supply grant");
}

// --- firing --------------------------------------------------------------

#[test]
fn unfinished_turret_never_fires() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Turret));
    let site = h
        .world_mut()
        .confirm_placement(TURRET_CORNER, w0)
        .expect("confirm turret placement");
    // Builder walks in and starts attending.
    let mut attending = false;
    for _ in 0..600 {
        h.step_exact(1);
        let slot = h.world().entities().slot(site).expect("site alive");
        if h.world().entities().progress(slot) > 0 {
            attending = true;
            break;
        }
    }
    assert!(attending, "builder must start the site within 600 ticks");

    // 34.0 effective cells: rect face x=168, minus body radius 3. Far
    // from the builder, well inside would-be weapon range, and in one of
    // the obstacle lattice's clear windows on this row.
    let g = spawn_ghoul(&mut h, [205.0, 148.0]);
    h.step_exact(10);
    assert_eq!(hp_of(&h, g), 30, "a site never target-scans");

    // Fast-forward to the finish boundary: exactly one attended tick
    // remains (the builder is still attending).
    let slot = h.world().entities().slot(site).expect("site alive");
    h.world_mut()
        .entities_mut()
        .set_progress(slot, TURRET_BUILD_TICKS - 1, TURRET_BUILD_TICKS);
    let mut finished = false;
    for _ in 0..5 {
        h.step_exact(1);
        if !h.world().is_site(site) {
            finished = true;
            break;
        }
    }
    assert!(finished, "one attended tick finishes the site");
    // First shot lands on the finish tick or the one after — the
    // intra-tick scan order is T3's pin, not this one's. Never earlier.
    if hp_of(&h, g) == 30 {
        h.step_exact(1);
    }
    assert_eq!(hp_of(&h, g), 20, "the finished turret opens fire");
}

#[test]
fn turret_auto_fires_nearest() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    build_and_finish_turret(&mut h, TURRET_CORNER, w0);
    let near = spawn_ghoul(&mut h, [196.0, 148.0]); // 25.0 effective
    let far = spawn_ghoul(&mut h, [205.0, 148.0]); // 34.0 effective
    h.step_exact(1);
    assert_eq!(hp_of(&h, near), 20, "nearest ghoul takes the first 10");
    assert_eq!(hp_of(&h, far), 30, "one shot per cooldown, one target");
    // No second shot inside the 20-tick cooldown…
    h.step_exact(19);
    assert_eq!(hp_of(&h, near), 20);
    // …and the second lands within a couple of ticks of it (the exact
    // decrement phase is T3's `cooldown_gates_fire_rate` pin).
    h.step_exact(5);
    assert_eq!(hp_of(&h, near), 10);
    // Three hits kill a 30 HP Ghoul; by tick 50 the turret retargets.
    h.step_exact(25);
    assert!(!h.world().entities().contains(near), "3 hits by tick 50");
    h.step_exact(40);
    assert!(
        !h.world().entities().contains(far) || hp_of(&h, far) < 30,
        "after the kill the turret moves to the next ghoul"
    );
}

#[test]
fn turret_target_ties_break_to_lowest_slot() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    build_and_finish_turret(&mut h, TURRET_CORNER, w0);
    // Both exactly 28.0 cells from the footprint rectangle on the
    // centre row: x = 168 + 28 east, x = 160 − 28 west. Exact f32
    // equality — no rounding enters a subtraction of these literals.
    let east = spawn_ghoul(&mut h, [196.0, 148.0]); // spawned first → lower slot
    let west = spawn_ghoul(&mut h, [132.0, 148.0]);
    h.step_exact(1);
    assert_eq!(hp_of(&h, east), 20, "equal distance → lower slot");
    assert_eq!(hp_of(&h, west), 30);
}

#[test]
fn turret_range_measured_from_footprint() {
    // Effective distance = distance from the footprint *rectangle* to
    // the target's hull. The rect's +x face is at x = 168: a ghoul on
    // the centre row at x = 206.9 reads 38.9 to the rect = 35.9
    // effective (in range); x = 207.1 reads 39.1 = 36.1 (out). Measured
    // from the *centre* the first case would read 206.9 − 164 − 3 = 39.9
    // > 36 and never fire — the "an 8-cell footprint must not lose 4
    // cells of range" claim, pinned.
    assert_eq!(fire_probe([206.9, 148.0]), 20, "35.9 effective: in range");
    assert_eq!(fire_probe([207.1, 148.0]), 30, "36.1 effective: out");
}

// --- death and economy ---------------------------------------------------

#[test]
fn ghouls_kill_turret() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let turret = build_and_finish_turret(&mut h, TURRET_CORNER, w0);

    // Anti-vacuity 1: the turret is alive and untouched before the march.
    let slot = h.world().entities().slot(turret).expect("live turret");
    assert_eq!(
        h.world().entities().hp(slot),
        TURRET_MAX_HP,
        "the turret must be at full HP before the first ghoul spawns"
    );

    // Point the horde at the turret: with the HQ gone it is the only
    // remaining player building, so it becomes the march objective.
    let hq = h.world().start_hq().expect("hq");
    assert_eq!(
        h.world_mut().apply_damage(hq, 100_000),
        DamageResult::Killed
    );
    let losses_before = h.world().losses();
    let kills_before = h.world().kills();

    for &pos in &GHOUL_RING {
        spawn_ghoul(&mut h, pos);
    }

    // Anti-vacuity 2: at the midpoint the fight is real — the turret is
    // still standing and has already lost HP to the ring.
    h.step_exact(50);
    assert!(
        h.world().entities().contains(turret),
        "the turret must still be standing at tick 50"
    );
    let slot = h.world().entities().slot(turret).expect("live turret");
    assert!(
        h.world().entities().hp(slot) < TURRET_MAX_HP,
        "the ring must have bitten into the turret by tick 50"
    );

    // Full window: the turret must fall within 3000 ticks (50 already run).
    let mut dead = false;
    for _ in 0..2_950 {
        h.step_exact(1);
        if !h.world().entities().contains(turret) {
            dead = true;
            break;
        }
    }
    assert!(dead, "12 ghouls must grind 150 HP down within 3000 ticks");
    // Anti-vacuity 3: a fight, not a walkover — the turret is the only
    // armed player entity, so a kill on the books is a turret kill.
    assert!(
        h.world().kills() > kills_before,
        "the turret must take at least one ghoul with it"
    );
    let w = h.world().static_nav().width();
    let (cx, cy) = TURRET_CENTER_CELL;
    let idx = (cx + cy * w) as usize;
    assert!(
        !h.world().static_nav().placement_solids()[idx],
        "turret death un-stamps its footprint"
    );
    assert!(!h.world().nav().blocked()[idx], "pool mask followed");
    assert!(
        h.world().losses() > losses_before,
        "the turret's death is counted as a loss"
    );
}

#[test]
fn turret_grants_no_supply_not_dropoff() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let cap_before = h.world().supply().cap();
    build_and_finish_turret(&mut h, NODE_SIDE_CORNER, w0);
    assert_eq!(h.world().supply().cap(), cap_before, "no grant");
    // The turret hugs the node (4.5 cells to its rect; the HQ is ~21.7
    // away): a drop-off bug would send the hauler here.
    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    assert!(h.world_mut().order_gather(w0, node));
    let mut returning = false;
    for _ in 0..4_000 {
        if let Some(Order::Gather {
            phase: GatherPhase::Returning { drop_off, .. },
            ..
        }) = h.world().order_of(w0)
        {
            let hq = h.world().start_hq().expect("hq");
            assert_eq!(drop_off, hq, "cargo returns to the HQ, never the turret");
            returning = true;
            break;
        }
        h.step_exact(1);
    }
    assert!(returning, "worker must turn for home within 4000 ticks");
}

// --- build card ----------------------------------------------------------

#[test]
fn turret_card_button_positional() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().select_only(w0));
    let slots = command_slots(h.world());
    assert_eq!(slots[3].command, Some(CommandId::BuildTurret));
    assert!(slots[3].enabled);
    assert_eq!(
        COMMAND_SLOT_KEYS[3], b'A',
        "positional key follows the slot"
    );
    assert_eq!(BUILD_MENU.len(), 4);
    assert_eq!(BUILD_MENU[3], (b'A', BuildingKind::Turret));
}
