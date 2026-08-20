//! Combat T1 — HP, armor and death: the combat data core.
//!
//! Pure logic, no GPU, no clock: every case is a headless CPU check of
//! `mmd_engine::rts` through `testkit::RtsHarness`. Damage enters only via
//! `RtsWorld::apply_damage` — nothing in these worlds can attack yet, so an
//! undamaged world is untouched by this slice.

use mmd_engine::rts::{
    BARRACKS_ARMOR, BARRACKS_MAX_HP, BuildingKind, DEPOT_ARMOR, DEPOT_MAX_HP, DEPOT_SUPPLY_GRANT,
    DamageResult, EntityId, EntityKind, GatherPhase, HQ_ARMOR, HQ_MAX_HP, Order, ResourceKind,
    SOLDIER_ARMOR, SOLDIER_MAX_HP, UnitKind, WORKER_ARMOR, WORKER_MAX_HP, armor, max_hp,
};
use mmd_engine::scenario::Cell;
use mmd_engine::testkit::RtsHarness;

fn first_worker(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0]
}

fn crystal_node(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0]
}

/// Clear, obstacle-free, node-free, HQ-free corners of the tracked scene —
/// the same corners `rts_production.rs` builds at.
const DEPOT_CORNER: Cell = Cell { x: 180, y: 176 };
const BARRACKS_CORNER: Cell = Cell { x: 198, y: 176 };

/// A damage amount no current kind survives through its armor.
const OVERKILL: u32 = 100_000;

/// Place, confirm and fully attend a building until it finishes. Generous
/// on ticks (2 000, well past any build time): the builder walks in first.
fn build_and_finish(
    h: &mut RtsHarness,
    kind: BuildingKind,
    corner: Cell,
    builder: EntityId,
) -> EntityId {
    assert!(h.world_mut().begin_placement(kind));
    let site = h
        .world_mut()
        .confirm_placement(corner, builder)
        .expect("confirm placement");
    h.step_exact(2_000);
    assert!(
        !h.world().is_site(site),
        "building must have finished within 2000 ticks"
    );
    site
}

// --- stats and spawn state -----------------------------------------------

#[test]
fn stats_are_the_published_constants() {
    assert_eq!(max_hp(EntityKind::Unit(UnitKind::Worker)), WORKER_MAX_HP);
    assert_eq!(WORKER_MAX_HP, 25);
    assert_eq!(max_hp(EntityKind::Unit(UnitKind::Soldier)), SOLDIER_MAX_HP);
    assert_eq!(SOLDIER_MAX_HP, 40);
    assert_eq!(max_hp(EntityKind::Building(BuildingKind::Hq)), HQ_MAX_HP);
    assert_eq!(HQ_MAX_HP, 400);
    assert_eq!(
        max_hp(EntityKind::Building(BuildingKind::Depot)),
        DEPOT_MAX_HP
    );
    assert_eq!(DEPOT_MAX_HP, 150);
    assert_eq!(
        max_hp(EntityKind::Building(BuildingKind::Barracks)),
        BARRACKS_MAX_HP
    );
    assert_eq!(BARRACKS_MAX_HP, 200);
    assert_eq!(max_hp(EntityKind::Node(ResourceKind::Crystal)), 0);
    assert_eq!(max_hp(EntityKind::Node(ResourceKind::Gas)), 0);

    assert_eq!(armor(EntityKind::Unit(UnitKind::Worker)), WORKER_ARMOR);
    assert_eq!(WORKER_ARMOR, 0);
    assert_eq!(armor(EntityKind::Unit(UnitKind::Soldier)), SOLDIER_ARMOR);
    assert_eq!(SOLDIER_ARMOR, 0);
    assert_eq!(armor(EntityKind::Building(BuildingKind::Hq)), HQ_ARMOR);
    assert_eq!(HQ_ARMOR, 2);
    assert_eq!(
        armor(EntityKind::Building(BuildingKind::Depot)),
        DEPOT_ARMOR
    );
    assert_eq!(DEPOT_ARMOR, 1);
    assert_eq!(
        armor(EntityKind::Building(BuildingKind::Barracks)),
        BARRACKS_ARMOR
    );
    assert_eq!(BARRACKS_ARMOR, 1);
    assert_eq!(armor(EntityKind::Node(ResourceKind::Crystal)), 0);
}

#[test]
fn every_entity_spawns_at_full_hp() {
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let mut slots = Vec::new();
    h.world().entities().collect_live(&mut slots);
    assert!(!slots.is_empty());
    for slot in slots {
        let kind = h.world().entities().kind(slot);
        assert_eq!(h.world().entities().hp(slot), max_hp(kind), "slot {slot}");
    }
}

// --- apply_damage arithmetic ---------------------------------------------

#[test]
fn damage_reduces_hp_by_damage_minus_armor() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let slot = h.world().entities().slot(hq).expect("live hq");
    assert_eq!(h.world().entities().hp(slot), HQ_MAX_HP);
    assert_eq!(
        h.world_mut().apply_damage(hq, 10),
        DamageResult::Damaged {
            remaining_hp: HQ_MAX_HP - 8
        },
        "10 damage through 2 armor must deal 8"
    );
    assert_eq!(h.world().entities().hp(slot), HQ_MAX_HP - 8);
}

#[test]
fn damage_floors_at_one() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    let slot = h.world().entities().slot(hq).expect("live hq");
    // 1 damage through 2 armor: saturates to 0, floors to 1.
    assert_eq!(
        h.world_mut().apply_damage(hq, 1),
        DamageResult::Damaged {
            remaining_hp: HQ_MAX_HP - 1
        }
    );
    // 2 damage through 2 armor: exactly 0, floors to 1.
    assert_eq!(
        h.world_mut().apply_damage(hq, 2),
        DamageResult::Damaged {
            remaining_hp: HQ_MAX_HP - 2
        }
    );
    assert_eq!(h.world().entities().hp(slot), HQ_MAX_HP - 2);
}

#[test]
fn damage_to_a_node_is_a_no_op() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let node = crystal_node(&h);
    let slot = h.world().entities().slot(node).expect("live node");
    let amount_before = h.world().entities().amount(slot);
    let hash_before = h.state_hash();
    assert_eq!(
        h.world_mut().apply_damage(node, OVERKILL),
        DamageResult::Indestructible
    );
    assert!(h.world().entities().contains(node));
    assert_eq!(h.world().entities().amount(slot), amount_before);
    assert_eq!(
        h.state_hash(),
        hash_before,
        "a refused hit must change nothing"
    );
}

#[test]
fn damage_to_a_stale_id_is_refused() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert_eq!(
        h.world_mut().apply_damage(w0, OVERKILL),
        DamageResult::Killed
    );
    assert_eq!(h.world_mut().apply_damage(w0, 5), DamageResult::NoTarget);
}

// --- unit death ----------------------------------------------------------

#[test]
fn a_unit_dies_at_zero_hp_and_its_slot_frees() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let n_before = h.world().entities().len();
    assert!(h.world_mut().select_only(w0));
    // 25 damage through 0 armor: exactly lethal for a full-health Worker.
    assert_eq!(
        h.world_mut().apply_damage(w0, WORKER_MAX_HP),
        DamageResult::Killed
    );
    assert!(
        !h.world().entities().contains(w0),
        "stale id must not resolve"
    );
    assert_eq!(h.world().entities().len(), n_before - 1);
    assert_eq!(h.world().order_of(w0), None);
    // Selection pruning stays where it has always been: the tick's last step.
    h.step_exact(1);
    assert!(h.world().selection().ids().is_empty());
}

// --- building death ------------------------------------------------------

#[test]
fn building_death_unstamps_its_footprint() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let depot = build_and_finish(&mut h, BuildingKind::Depot, DEPOT_CORNER, w0);
    let edge = BuildingKind::Depot.footprint_cells();
    let w = h.world().static_nav().width();
    let centre_idx = (DEPOT_CORNER.x + edge / 2 + (DEPOT_CORNER.y + edge / 2) * w) as usize;
    assert!(h.world().static_nav().placement_solids()[centre_idx]);
    assert!(h.world().nav().blocked()[centre_idx]);

    assert_eq!(
        h.world_mut().apply_damage(depot, OVERKILL),
        DamageResult::Killed
    );
    assert!(!h.world().entities().contains(depot));
    for dy in 0..edge {
        for dx in 0..edge {
            let idx = (DEPOT_CORNER.x + dx + (DEPOT_CORNER.y + dy) * w) as usize;
            assert!(
                !h.world().static_nav().placement_solids()[idx],
                "footprint cell (+{dx},+{dy}) must be clear again"
            );
        }
    }
    // The pool's mask followed the static one — the whole-mask replacement
    // is what invalidates every cached field, same rule as stamping.
    assert!(!h.world().nav().blocked()[centre_idx]);
}

#[test]
fn building_death_cancels_its_queue_without_refund() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let barracks = build_and_finish(&mut h, BuildingKind::Barracks, BARRACKS_CORNER, w0);
    let used_before = h.world().supply().used();
    assert!(
        h.world_mut()
            .enqueue_unit(barracks, UnitKind::Soldier)
            .is_ok()
    );
    let resources_after_enqueue = h.world().resources();
    assert_eq!(
        h.world().reserved_supply(),
        2,
        "one queued Soldier reserves 2"
    );

    assert_eq!(
        h.world_mut().apply_damage(barracks, OVERKILL),
        DamageResult::Killed
    );
    // No refund: the stock is exactly what it was after paying.
    assert_eq!(h.world().resources(), resources_after_enqueue);
    assert!(h.world().production_queue(barracks).is_none());
    assert_eq!(h.world().reserved_supply(), 0);
    // used self-heals on the next tick's recount.
    h.step_exact(1);
    assert_eq!(h.world().supply().used(), used_before);
}

#[test]
fn building_death_revokes_its_supply_grant() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let depot = build_and_finish(&mut h, BuildingKind::Depot, DEPOT_CORNER, w0);
    let cap_with_depot = h.world().supply().cap();
    assert_eq!(
        h.world_mut().apply_damage(depot, OVERKILL),
        DamageResult::Killed
    );
    assert_eq!(
        h.world().supply().cap(),
        cap_with_depot - DEPOT_SUPPLY_GRANT,
        "a dead Depot's grant must be revoked"
    );
}

#[test]
fn hq_death_idles_its_returning_gatherers() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let node = crystal_node(&h);
    assert!(h.world_mut().order_gather(w0, node));
    // Walk in, mine a load, turn for home — bounded, deterministic.
    let mut returning = false;
    for _ in 0..4_000 {
        if matches!(
            h.world().order_of(w0),
            Some(Order::Gather {
                phase: GatherPhase::Returning { .. },
                ..
            })
        ) {
            returning = true;
            break;
        }
        h.step_exact(1);
    }
    assert!(returning, "worker must turn for home within 4000 ticks");
    let hq = h.world().start_hq().expect("hq");
    assert!(matches!(
        h.world().order_of(w0),
        Some(Order::Gather {
            phase: GatherPhase::Returning { drop_off, .. },
            ..
        }) if drop_off == hq
    ));

    assert_eq!(
        h.world_mut().apply_damage(hq, OVERKILL),
        DamageResult::Killed
    );
    assert_eq!(
        h.world().order_of(w0),
        Some(Order::Idle),
        "a hauler bound for a dead drop-off must go idle in the same call"
    );
    assert_eq!(h.world().start_hq(), None);
}

#[test]
fn site_death_idles_its_builders_without_refund() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let site = h
        .world_mut()
        .confirm_placement(DEPOT_CORNER, w0)
        .expect("confirm placement");
    assert!(matches!(
        h.world().order_of(w0),
        Some(Order::Build { site: s, .. }) if s == site
    ));
    let resources_after_confirm = h.world().resources();

    assert_eq!(
        h.world_mut().apply_damage(site, OVERKILL),
        DamageResult::Killed
    );
    assert!(!h.world().entities().contains(site));
    assert_eq!(h.world().order_of(w0), Some(Order::Idle));
    // Destruction is not a cancel: the cost stays spent.
    assert_eq!(h.world().resources(), resources_after_confirm);
}

// --- state hash ----------------------------------------------------------

#[test]
fn hp_enters_the_state_hash() {
    let mut a = RtsHarness::scene().build().expect("rts scene harness");
    let b = RtsHarness::scene().build().expect("rts scene harness");
    assert_eq!(a.state_hash(), b.state_hash());
    let hq = a.world().start_hq().expect("hq");
    a.world_mut().apply_damage(hq, 10);
    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "a damaged HQ must change the digest"
    );
}
