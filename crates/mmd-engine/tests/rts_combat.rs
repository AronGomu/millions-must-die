//! Combat T1 — HP, armor and death: the combat data core.
//!
//! Pure logic, no GPU, no clock: every case is a headless CPU check of
//! `mmd_engine::rts` through `testkit::RtsHarness`. Damage enters only via
//! `RtsWorld::apply_damage` — nothing in these worlds can attack yet, so an
//! undamaged world is untouched by this slice.

use mmd_engine::rts::{
    BARRACKS_ARMOR, BARRACKS_MAX_HP, BuildingKind, DEPOT_ARMOR, DEPOT_MAX_HP, DEPOT_SUPPLY_GRANT,
    DamageResult, DeathEvent, EntityId, EntityKind, FormationGoal, GHOUL_SPEED_CELLS_PER_SEC,
    GatherPhase, HQ_ARMOR, HQ_MAX_HP, MAX_ENTITIES, OWNER_ENEMY, OWNER_PLAYER, Order, ResourceKind,
    SOLDIER_ARMOR, SOLDIER_MAX_HP, UnitKind, WORKER_ARMOR, WORKER_MAX_HP, armor, max_hp, weapon,
};
use mmd_engine::scenario::{Cell, EnemySpec, RtsSpec, ScenarioSpec};
use mmd_engine::testkit::{FIXTURE_RTS_COMBAT_V1, RtsHarness, fixture_path};

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

// =========================================================================
// T3 — instant-hit combat, enemy march AI and auto-acquire.
//
// Headless, seeded, clock-free: every case drives `RtsWorld::tick` through
// `testkit::RtsHarness`. Player commands do not exist yet (T4), so orders
// are placed through the testkit seams.
// =========================================================================

const W: u32 = 96;
const H: u32 = 96;

/// A combat sandbox: HQ (12-cell footprint, centre [88.0, 88.0]) in the
/// south-east corner, nodes in the north-east corner, the one mandatory
/// seeded worker parked in the north-west corner far from every fight.
/// Each case spawns and places its own combatants.
fn combat_spec() -> ScenarioSpec {
    ScenarioSpec {
        version: "rts_prototype_v1".to_string(),
        width: W,
        height: H,
        cell_size_px: 4,
        sprite_size_px: 48,
        hard_agent_count: 0,
        stretch_agent_count: 0,
        seed: 1,
        destination: Cell { x: 0, y: 0 },
        spawn_cells: vec![Cell { x: 4, y: 4 }],
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 0,
        separation_strength_q8: 0,
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
        obstacle_cells: vec![],
        rts: Some(RtsSpec {
            start_crystal: 300,
            start_gas: 100,
            start_supply_cap: 10,
            hq_cell: Cell { x: 82, y: 82 },
            crystal_nodes: vec![Cell { x: 94, y: 1 }],
            gas_nodes: vec![Cell { x: 93, y: 1 }],
            enemies: None,
        }),
    }
}

fn harness() -> RtsHarness {
    RtsHarness::spec(combat_spec())
        .build()
        .expect("combat harness")
}

fn spawn_unit(h: &mut RtsHarness, kind: UnitKind, owner: u8, pos: [f32; 2]) -> EntityId {
    h.world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(kind), owner, pos)
        .expect("store has room")
}

/// The one place HP is read; if T1 named its accessor differently, fix here.
fn hp_of(h: &RtsHarness, id: EntityId) -> u32 {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().hp(slot)
}

fn pos_of(h: &RtsHarness, id: EntityId) -> [f32; 2] {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().position(slot)
}

fn attack_move_goal(h: &RtsHarness, id: EntityId) -> FormationGoal {
    match h.world().order_of(id) {
        Some(Order::AttackMove { goal, .. }) => goal,
        other => panic!("expected an AttackMove order, got {other:?}"),
    }
}

/// One `apply_damage` call the HQ cannot survive: 402 - armor 2 = 400 = max HP.
/// Killing the only player building parks the enemy AI (objective `None`),
/// so a case controls exactly who moves and who fires.
fn kill_hq(h: &mut RtsHarness) {
    let hq = h.world().start_hq().expect("seeded hq");
    let _ = h.world_mut().apply_damage(hq, 402);
    assert!(!h.world().entities().contains(hq), "the hq must be dead");
}

// --- firing rules ---------------------------------------------------------

/// An idle soldier defends itself: it acquires the nearest hostile that walks
/// into its range and fires on exactly its cooldown period, without ever
/// being given an order.
#[test]
fn soldier_auto_acquires_idle() {
    let mut h = harness();
    kill_hq(&mut h);
    let _soldier = spawn_unit(&mut h, UnitKind::Soldier, OWNER_PLAYER, [30.5, 30.5]);
    let ghoul = spawn_unit(&mut h, UnitKind::Ghoul, OWNER_ENEMY, [60.5, 30.5]);
    // Surface distance 60.5 - 30.5 - 3.0 = 27.0 > the soldier's 24.0 reach:
    // the ghoul has to walk in before a shot is possible.
    let dest = Cell { x: 40, y: 30 };
    let field = h.world_mut().nav_mut().acquire(dest).expect("field");
    assert!(h.world_mut().force_order_for_test(
        ghoul,
        Order::Move {
            goal: FormationGoal {
                anchor: dest,
                slot: dest,
            },
            field,
        },
    ));

    let mut hits: Vec<(u64, u32)> = Vec::new();
    let mut hp = hp_of(&h, ghoul);
    for _ in 0..80 {
        h.step_exact(1);
        if !h.world().entities().contains(ghoul) {
            hits.push((h.tick_index(), hp));
            break;
        }
        let now = hp_of(&h, ghoul);
        if now < hp {
            hits.push((h.tick_index(), hp - now));
            hp = now;
        }
    }

    assert!(
        hits.len() >= 2,
        "the soldier must have fired at least twice, got {hits:?}"
    );
    for &(_, drop) in &hits {
        assert_eq!(
            drop, 6,
            "every hit is the soldier's flat 6 damage: {hits:?}"
        );
    }
    assert!(
        hits[0].0 > 1,
        "the first shot must come after the walk-in, got tick {}",
        hits[0].0
    );
    for w in hits.windows(2) {
        assert_eq!(
            w[1].0 - w[0].0,
            15,
            "the firing period is exactly cooldown_ticks: {hits:?}"
        );
    }
}

/// A unit under a plain `Order::Move` walks past a hostile without ever
/// firing — and the hostile shooting *back* is what keeps that from being a
/// claim about an empty stretch of map.
#[test]
fn plain_move_never_fires() {
    let mut h = harness();
    kill_hq(&mut h);
    let soldier = spawn_unit(&mut h, UnitKind::Soldier, OWNER_PLAYER, [20.5, 30.5]);
    let ghoul = spawn_unit(&mut h, UnitKind::Ghoul, OWNER_ENEMY, [44.5, 36.5]);
    // 70 cells at 24.0 c/s is 175 ticks of walking, so the soldier is still
    // under `Move` — never Idle — for the whole measured window. The
    // assertion below pins that rather than trusting the arithmetic.
    assert!(h.world_mut().order_move(soldier, Cell { x: 90, y: 30 }));
    h.step_exact(140);

    assert!(
        matches!(h.world().order_of(soldier), Some(Order::Move { .. })),
        "the soldier must still be walking, or this measures an idle unit"
    );
    assert_eq!(
        hp_of(&h, ghoul),
        30,
        "a moving unit never fires, whatever walks into its reach"
    );
    assert!(
        h.world().entities().contains(soldier),
        "the soldier must survive the crossing"
    );
    assert!(
        hp_of(&h, soldier) < 40,
        "the ghoul must actually have had the soldier in range, or the case \
         proves nothing about a unit that could have fired"
    );
}

/// Two hostiles at an exact f32 tie are broken by slot: the lower one is
/// shot. Both fire back under the idle rule.
#[test]
fn nearest_target_lowest_slot_tie() {
    let mut h = harness();
    kill_hq(&mut h);
    let soldier = spawn_unit(&mut h, UnitKind::Soldier, OWNER_PLAYER, [30.5, 30.5]);
    let a = spawn_unit(&mut h, UnitKind::Ghoul, OWNER_ENEMY, [20.5, 30.5]);
    let b = spawn_unit(&mut h, UnitKind::Ghoul, OWNER_ENEMY, [40.5, 30.5]);
    let (sa, sb) = (
        h.world().entities().slot(a).expect("live"),
        h.world().entities().slot(b).expect("live"),
    );
    assert!(sa < sb, "spawn order must give A the lower slot");

    h.step_exact(1);

    assert_eq!(hp_of(&h, a), 24, "the lower slot takes the shot");
    assert_eq!(hp_of(&h, b), 30, "the tied higher slot is untouched");
    assert_eq!(
        hp_of(&h, soldier),
        30,
        "both ghouls returned fire under the idle rule"
    );
}

/// The cooldown column, not the tick, is what gates a shot: a ghoul planted
/// in reach of the HQ hits it exactly every 30 ticks starting on tick 1.
#[test]
fn cooldown_gates_fire_rate() {
    let mut h = harness();
    let hq = h.world().start_hq().expect("seeded hq");
    // rect_distance([88.5, 74.5], [88.0, 88.0], 12) == 7.5 <= the ghoul's 8.0.
    let _ghoul = spawn_unit(&mut h, UnitKind::Ghoul, OWNER_ENEMY, [88.5, 74.5]);

    h.step_exact(90);
    assert_eq!(
        hp_of(&h, hq),
        HQ_MAX_HP - 9,
        "three hits of (5 - armor 2) on ticks 1, 31 and 61"
    );
    h.step_exact(1);
    assert_eq!(hp_of(&h, hq), HQ_MAX_HP - 12, "the fourth hit lands on 91");
    assert_eq!(h.world().first_combat_tick(), Some(1));
}

/// Nothing under a `Gather` order fires, and gathering keeps working with a
/// hostile parked next to the node.
#[test]
fn a_gathering_worker_never_fires() {
    let mut h = harness();
    kill_hq(&mut h);
    let soldier = spawn_unit(&mut h, UnitKind::Soldier, OWNER_PLAYER, [30.5, 30.5]);
    let ghoul = spawn_unit(&mut h, UnitKind::Ghoul, OWNER_ENEMY, [40.5, 30.5]);
    // A soldier has a weapon; put it under a Gather order it cannot own in
    // the game, through the seam, and it must still not fire.
    let node = crystal_node(&h);
    let field = h
        .world_mut()
        .nav_mut()
        .acquire(Cell { x: 60, y: 60 })
        .expect("field");
    assert!(h.world_mut().force_order_for_test(
        soldier,
        Order::Gather {
            node,
            phase: GatherPhase::ToNode {
                goal: FormationGoal {
                    anchor: Cell { x: 60, y: 60 },
                    slot: Cell { x: 60, y: 60 },
                },
                field,
            },
        },
    ));
    h.step_exact(60);
    assert_eq!(
        hp_of(&h, ghoul),
        30,
        "a gathering unit never fires, whatever is in reach"
    );
}

// --- enemy march AI -------------------------------------------------------

/// Every enemy marches on one shared objective, down one shared pooled
/// field: no per-enemy pathfinding, and no field churn.
#[test]
fn ghouls_march_on_hq() {
    let mut h = harness();
    let mut ghouls = Vec::new();
    for i in 0..6 {
        ghouls.push(spawn_unit(
            &mut h,
            UnitKind::Ghoul,
            OWNER_ENEMY,
            [10.5 + 9.0 * i as f32, 60.5],
        ));
    }

    h.step_exact(2);
    let rebuilds = h.world().nav().rebuild_count();
    let before: Vec<f32> = ghouls
        .iter()
        .map(|&g| {
            let p = pos_of(&h, g);
            (p[0] - 88.0).hypot(p[1] - 88.0)
        })
        .collect();

    h.step_exact(200);

    let anchor = attack_move_goal(&h, ghouls[0]).anchor;
    for (k, &g) in ghouls.iter().enumerate() {
        let p = pos_of(&h, g);
        let now = (p[0] - 88.0).hypot(p[1] - 88.0);
        assert!(
            now < before[k],
            "ghoul {k} must have closed on the HQ: {} -> {now}",
            before[k]
        );
        assert_eq!(
            attack_move_goal(&h, g).anchor,
            anchor,
            "the whole faction shares one objective cell"
        );
    }
    assert_eq!(
        h.world().nav().rebuild_count(),
        rebuilds,
        "six marching ghouls must ride one field, never rebuild it"
    );
}

/// An `AttackMove` halts on a target in range, kills it, then resumes the
/// march — and the counters see a player loss, not a kill.
#[test]
fn ghoul_attacks_first_thing_in_range() {
    let mut h = harness();
    let hq = h.world().start_hq().expect("seeded hq");
    let ghoul = spawn_unit(&mut h, UnitKind::Ghoul, OWNER_ENEMY, [40.5, 88.5]);
    let worker = spawn_unit(&mut h, UnitKind::Worker, OWNER_PLAYER, [58.5, 88.5]);

    let mut hits: Vec<(u64, u32)> = Vec::new();
    let mut hp = hp_of(&h, worker);
    let mut death_tick = None;
    for _ in 0..900 {
        h.step_exact(1);
        if !h.world().entities().contains(worker) {
            hits.push((h.tick_index(), hp));
            death_tick = Some(h.tick_index());
            break;
        }
        let now = hp_of(&h, worker);
        if now < hp {
            hits.push((h.tick_index(), hp - now));
            hp = now;
        }
    }
    assert!(death_tick.is_some(), "the worker must have died: {hits:?}");
    for &(_, drop) in &hits {
        assert_eq!(drop, 5, "a ghoul deals a flat 5 to an unarmored worker");
    }
    for w in hits.windows(2) {
        assert_eq!(w[1].0 - w[0].0, 30, "one shot per 30 ticks: {hits:?}");
    }
    assert_eq!(h.world().losses(), 1, "a dead player unit is a loss");
    assert_eq!(h.world().kills(), 0);

    // The march resumes the moment the target is gone.
    let x_at_death = pos_of(&h, ghoul)[0];
    h.step_exact(60);
    assert!(
        pos_of(&h, ghoul)[0] > x_at_death,
        "the ghoul must resume its march east once nothing is in reach"
    );
    h.step_exact(400);
    assert!(
        hp_of(&h, hq) < HQ_MAX_HP,
        "the resumed march must reach the HQ"
    );
}

/// The whole loop end to end: an undefended base falls, the footprint is
/// un-stamped, and the faction goes idle with nothing left to march on.
#[test]
fn ghouls_besiege_and_kill_hq() {
    let mut h = harness();
    let hq = h.world().start_hq().expect("seeded hq");
    let mut ghouls = Vec::new();
    for i in 0..5 {
        for x in [64.5_f32, 70.5] {
            ghouls.push(spawn_unit(
                &mut h,
                UnitKind::Ghoul,
                OWNER_ENEMY,
                [x, 62.5 + 6.0 * i as f32],
            ));
        }
    }

    h.step_exact(1_400);

    assert!(
        !h.world().entities().contains(hq),
        "ten ghouls must have brought a 400 hp HQ down inside 1400 ticks"
    );
    assert_eq!(h.world().losses(), 1, "the HQ is the only loss");
    assert_eq!(h.world().kills(), 0);
    assert!(h.world().first_combat_tick().is_some());
    assert!(
        !h.world().nav().blocked()[(88 + 88 * W) as usize],
        "the dead HQ's footprint must be un-stamped from the pooled mask"
    );

    h.step_exact(2);
    for (k, &g) in ghouls.iter().enumerate() {
        assert_eq!(
            h.world().order_of(g),
            Some(Order::Idle),
            "ghoul {k} has nothing left to march on"
        );
    }
}

/// Killing the objective re-aims the faction at the next player building,
/// measured from the *starting* HQ centre so the reference point survives
/// its death.
#[test]
fn objective_retargets_on_hq_death() {
    let mut h = harness();
    h.world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Building(BuildingKind::Depot),
            OWNER_PLAYER,
            [30.0, 30.0],
        )
        .expect("room");
    let ghoul = spawn_unit(&mut h, UnitKind::Ghoul, OWNER_ENEMY, [10.5, 88.5]);

    h.step_exact(1);
    let g1 = attack_move_goal(&h, ghoul);
    kill_hq(&mut h);
    h.step_exact(1);
    let g2 = attack_move_goal(&h, ghoul);

    assert_ne!(g2.anchor, g1.anchor, "the objective must have moved");
    // Depot edge 8, centre [30.0, 30.0] -> footprint rect [26, 34)^2.
    let c = [g2.anchor.x as f32 + 0.5, g2.anchor.y as f32 + 0.5];
    let dx = (c[0] - 30.0).abs() - 4.0;
    let dy = (c[1] - 30.0).abs() - 4.0;
    let d = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    assert!(
        d <= 1.0,
        "the new objective {:?} must ring the depot, distance {d}",
        g2.anchor
    );
}

/// No player building left is not a crash and not a stampede: the faction
/// stops where it stands.
#[test]
fn no_player_buildings_enemies_idle() {
    let mut h = harness();
    let mut ghouls = Vec::new();
    for i in 0..3 {
        ghouls.push(spawn_unit(
            &mut h,
            UnitKind::Ghoul,
            OWNER_ENEMY,
            [20.5, 20.5 + 8.0 * i as f32],
        ));
    }
    h.step_exact(1);
    assert!(
        ghouls
            .iter()
            .all(|&g| matches!(h.world().order_of(g), Some(Order::AttackMove { .. }))),
        "the ghouls must have been marching first, or this proves nothing"
    );

    kill_hq(&mut h);
    h.step_exact(1);
    for (k, &g) in ghouls.iter().enumerate() {
        assert_eq!(h.world().order_of(g), Some(Order::Idle), "ghoul {k}");
    }
    let parked: Vec<[f32; 2]> = ghouls.iter().map(|&g| pos_of(&h, g)).collect();

    h.step_exact(60);
    for (k, &g) in ghouls.iter().enumerate() {
        assert_eq!(pos_of(&h, g), parked[k], "ghoul {k} must not have moved");
    }
}

// --- counters and determinism --------------------------------------------

/// The three exit-token counters, against a controlled one-sided fight.
#[test]
fn counters_and_first_combat_tick() {
    let mut h = harness();
    kill_hq(&mut h);
    let _soldier = spawn_unit(&mut h, UnitKind::Soldier, OWNER_PLAYER, [30.5, 30.5]);
    // Surface 20.0: inside the soldier's 24.0, well outside the ghoul's 8.0.
    let ghoul = spawn_unit(&mut h, UnitKind::Ghoul, OWNER_ENEMY, [53.5, 30.5]);

    h.step_exact(61);

    assert!(
        !h.world().entities().contains(ghoul),
        "five hits of 6 is exactly the ghoul's 30 hp"
    );
    assert_eq!(h.world().kills(), 1);
    assert_eq!(h.world().losses(), 0);
    assert_eq!(h.world().first_combat_tick(), Some(1));
}

/// A full scripted fight is bit-identical between two processes' worth of
/// world, tick by tick — not just at the end.
#[test]
fn combat_determinism() {
    let mut a = RtsHarness::path(fixture_path(FIXTURE_RTS_COMBAT_V1))
        .build()
        .expect("combat fixture");
    let mut b = RtsHarness::path(fixture_path(FIXTURE_RTS_COMBAT_V1))
        .build()
        .expect("combat fixture");
    assert_eq!(a.state_hash(), b.state_hash());
    for t in 1..=600 {
        a.step_exact(1);
        b.step_exact(1);
        assert_eq!(a.state_hash(), b.state_hash(), "diverged at tick {t}");
    }
    assert!(
        a.world().first_combat_tick().is_some(),
        "600 ticks of the combat fixture must contain a fight"
    );
}

/// The cooldown column is world state, so it is in the digest.
#[test]
fn cooldown_enters_state_hash() {
    let mut a = harness();
    let mut b = harness();
    let id = spawn_unit(&mut a, UnitKind::Soldier, OWNER_PLAYER, [30.5, 30.5]);
    let _ = spawn_unit(&mut b, UnitKind::Soldier, OWNER_PLAYER, [30.5, 30.5]);
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "two identically seeded worlds must agree"
    );

    let slot = a.world().entities().slot(id).expect("live soldier");
    a.world_mut().entities_mut().set_cooldown(slot, 5);
    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "a cooled-down weapon must change the digest"
    );
}

/// Hundreds of enemies, one field: the march is a pooled-field descent, not
/// three hundred paths.
#[test]
fn field_pool_not_churned() {
    let mut spec = combat_spec();
    spec.width = 160;
    spec.height = 160;
    let rts = spec.rts.as_mut().expect("rts block");
    rts.hq_cell = Cell { x: 144, y: 144 };
    rts.crystal_nodes = vec![Cell { x: 158, y: 1 }];
    rts.gas_nodes = vec![Cell { x: 157, y: 1 }];
    rts.enemies = Some(EnemySpec {
        pre_placed: (0..300)
            .map(|k| Cell {
                x: 4 + 7 * (k % 15),
                y: 4 + 7 * (k / 15),
            })
            .collect(),
        spawn_points: vec![],
        waves: vec![],
    });
    let mut h = RtsHarness::spec(spec).build().expect("300-ghoul harness");
    assert_eq!(h.world().enemies_spawned(), 300);

    h.step_exact(5);
    let rebuilds = h.world().nav().rebuild_count();
    let acquires = h.world().nav().acquire_count();
    assert!(
        acquires > 0,
        "the faction must actually have gone through the pool"
    );
    let ghouls = h.ids_of_kind(EntityKind::Unit(UnitKind::Ghoul));
    let before: Vec<[f32; 2]> = ghouls.iter().map(|&g| pos_of(&h, g)).collect();

    h.step_exact(150);

    assert_eq!(
        h.world().nav().rebuild_count(),
        rebuilds,
        "300 marching ghouls must not rebuild a single field"
    );
    let moved = ghouls
        .iter()
        .zip(&before)
        .filter(|&(&g, &p)| pos_of(&h, g) != p)
        .count();
    assert!(
        moved > 250,
        "the measured ticks must contain a real march, only {moved} of 300 moved"
    );
}

/// The published stats and speed, pinned where a balance edit is a visible
/// diff.
#[test]
fn the_ghoul_is_the_slowest_unit() {
    assert_eq!(GHOUL_SPEED_CELLS_PER_SEC, 18.0);
    assert!(weapon(UnitKind::Worker).is_none());
    assert_eq!(weapon(UnitKind::Ghoul).expect("armed").range_cells, 8.0);
}

// --- T6: the death-event buffer ------------------------------------------

#[test]
fn death_events_drain_once() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let w0 = first_worker(&h);
    let depot = build_and_finish(&mut h, BuildingKind::Depot, DEPOT_CORNER, w0);
    h.step_exact(1); // settle: any event this setup produced is cleared
    // Raw spawn after the last tick, so its position stays exact.
    let ghoul = h
        .world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Ghoul), OWNER_ENEMY, [30.5, 30.5])
        .expect("store has room");

    assert_eq!(
        h.world_mut().apply_damage(ghoul, OVERKILL),
        DamageResult::Killed
    );
    assert_eq!(
        h.world_mut().apply_damage(depot, OVERKILL),
        DamageResult::Killed
    );

    let mut out = Vec::with_capacity(MAX_ENTITIES);
    h.world_mut().drain_death_events(&mut out);
    assert_eq!(out.len(), 2, "two kills, two events, in kill order");
    assert_eq!(
        out[0],
        DeathEvent {
            kind: EntityKind::Unit(UnitKind::Ghoul),
            owner: OWNER_ENEMY,
            center: [30.5, 30.5],
        }
    );
    assert_eq!(
        out[1],
        DeathEvent {
            kind: EntityKind::Building(BuildingKind::Depot),
            owner: OWNER_PLAYER,
            // DEPOT_CORNER (180, 176) + edge 8 / 2.
            center: [184.0, 180.0],
        }
    );
    h.world_mut().drain_death_events(&mut out);
    assert!(out.is_empty(), "a second drain must find nothing");

    // An undrained event does not survive the next tick: offscreen runs
    // that never drain cost nothing and accumulate nothing.
    assert_eq!(
        h.world_mut().apply_damage(w0, OVERKILL),
        DamageResult::Killed
    );
    h.step_exact(1);
    h.world_mut().drain_death_events(&mut out);
    assert!(out.is_empty(), "the tick must clear an unconsumed buffer");
}

#[test]
fn death_events_never_enter_the_state_hash() {
    let mut a = RtsHarness::scene().build().expect("rts scene harness");
    let mut b = RtsHarness::scene().build().expect("rts scene harness");
    let wa = first_worker(&a);
    let wb = first_worker(&b);
    assert_eq!(
        a.world_mut().apply_damage(wa, OVERKILL),
        DamageResult::Killed
    );
    assert_eq!(
        b.world_mut().apply_damage(wb, OVERKILL),
        DamageResult::Killed
    );
    let mut out = Vec::with_capacity(MAX_ENTITIES);
    a.world_mut().drain_death_events(&mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "a drained and an undrained buffer must hash identically"
    );
}
