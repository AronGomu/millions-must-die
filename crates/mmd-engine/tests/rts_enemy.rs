//! T2 — enemy faction and waves: the Ghoul kind, `OWNER_ENEMY`, the
//! scenario enemy block and the deterministic wave spawner. Enemies in
//! this slice stand `Idle`; marching and fighting are the next ticket's.

use mmd_engine::render::{Camera, IsoView};
use mmd_engine::rts::{
    EntityKind, MAX_ENTITIES, OWNER_ENEMY, OWNER_NEUTRAL, Order, ResourceKind, UnitKind,
};
use mmd_engine::scenario::{Cell, EnemySpec, RtsSpec, ScenarioSpec, WaveSpec};
use mmd_engine::testkit::{FIXTURE_RTS_COMBAT_V1, RtsHarness, fixture_path};

/// The tracked combat fixture, hash-verified.
fn combat() -> RtsHarness {
    RtsHarness::path(fixture_path(FIXTURE_RTS_COMBAT_V1))
        .build()
        .expect("combat fixture loads hash-verified")
}

/// The view the drag test projects through — sized to the fixture's 96×96 grid.
fn combat_view() -> IsoView {
    Camera::new(96, 96, 4.0, [1920.0, 1080.0], [46.0, 46.0]).iso_view()
}

fn ghouls(h: &RtsHarness) -> Vec<mmd_engine::rts::EntityId> {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Ghoul))
}

#[test]
fn ghoul_kind_tags_stably() {
    assert_eq!(EntityKind::Unit(UnitKind::Ghoul).tag(), 0x12);
    assert_eq!(UnitKind::Ghoul.body_radius_cells(), 3.0);
}

#[test]
fn pre_placed_ghouls_spawn_at_start() {
    let h = combat();
    assert_eq!(h.tick_index(), 0);
    let ids = ghouls(&h);
    assert_eq!(ids.len(), 2, "the fixture pre-places exactly two ghouls");
    // Both preferred cells are legal and 6.0 apart (touching is legal),
    // so placement is exact, not relocated.
    let expect = [[20.5_f32, 20.5], [26.5, 20.5]];
    for (&id, want) in ids.iter().zip(expect) {
        let slot = h.world().entities().slot(id).expect("live");
        assert_eq!(h.world().entities().owner(slot), OWNER_ENEMY);
        assert_eq!(h.world().entities().position(slot), want);
        assert_eq!(h.world().order_of(id), Some(Order::Idle));
    }
    assert_eq!(h.world().enemies_spawned(), 2);
}

#[test]
fn wave_spawns_at_exact_tick() {
    let mut h = combat();
    h.step_exact(49);
    assert_eq!(ghouls(&h).len(), 2, "tick 49: pre-placed only");
    assert_eq!(h.world().enemies_spawned(), 2);
    h.step_exact(1);
    let ids = ghouls(&h);
    assert_eq!(ids.len(), 10, "tick 50: the 8-ghoul wave fires");
    assert_eq!(h.world().enemies_spawned(), 10);
    for &id in &ids {
        let slot = h.world().entities().slot(id).expect("live");
        assert_eq!(h.world().entities().owner(slot), OWNER_ENEMY);
    }
}

#[test]
fn enemies_never_consume_supply() {
    let mut h = combat();
    let before = h.world().supply().used();
    assert_eq!(before, 2, "two seeded workers at 1 supply each");
    h.step_exact(60); // through the tick-50 wave
    assert_eq!(ghouls(&h).len(), 10);
    assert_eq!(h.world().supply().used(), before);
}

#[test]
fn drag_box_excludes_enemies() {
    let mut h = combat();
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    let enemies = ghouls(&h);
    assert_eq!(workers.len(), 2);
    assert_eq!(enemies.len(), 2);
    // Park all four in one row; box select reads ground points only, so
    // overlapping bodies are a valid fixture (same trick as
    // `rts_selection.rs::park_workers_in_a_row`).
    for (i, &id) in workers.iter().chain(enemies.iter()).enumerate() {
        let slot = h.world().entities().slot(id).expect("live");
        h.world_mut()
            .entities_mut()
            .set_position(slot, [30.5 + i as f32, 30.5]);
    }
    let view = combat_view();
    let a = view.project(29.5, 30.5);
    let b = view.project(34.5, 30.5);
    let n = h.world_mut().box_select_into_selection(&view, a, b);
    assert_eq!(n, 2, "only the player's workers are boxable");
    assert_eq!(h.world().selection().ids(), workers.as_slice());
}

#[test]
fn spawn_determinism() {
    let run = || {
        let mut h = combat();
        h.step_exact(200); // both waves done: 2 + 8 + 4
        (h.world().enemies_spawned(), h.state_hash())
    };
    let (count_a, hash_a) = run();
    let (count_b, hash_b) = run();
    assert_eq!(count_a, 14);
    assert_eq!(count_b, 14);
    assert_eq!(hash_a, hash_b, "two fresh runs must agree bit-for-bit");
}

/// In-memory combat spec: the fixture's geometry, one wave of 8 at tick 10.
fn deferral_spec() -> ScenarioSpec {
    ScenarioSpec {
        version: "rts_prototype_v1".to_string(),
        width: 96,
        height: 96,
        cell_size_px: 4,
        sprite_size_px: 48,
        hard_agent_count: 0,
        stretch_agent_count: 0,
        seed: 20260817,
        destination: Cell { x: 46, y: 60 },
        spawn_cells: vec![Cell { x: 46, y: 56 }],
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 768,
        separation_strength_q8: 256,
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
        obstacle_cells: vec![],
        rts: Some(RtsSpec {
            start_crystal: 100,
            start_gas: 50,
            start_supply_cap: 10,
            hq_cell: Cell { x: 40, y: 40 },
            crystal_nodes: vec![Cell { x: 30, y: 46 }],
            gas_nodes: vec![Cell { x: 62, y: 46 }],
            enemies: Some(EnemySpec {
                pre_placed: vec![],
                spawn_points: vec![Cell { x: 76, y: 76 }],
                waves: vec![WaveSpec {
                    at_tick: 10,
                    count: 8,
                    spawn_point: 0,
                }],
            }),
        }),
    }
}

#[test]
fn wave_defers_when_store_full() {
    let mut h = RtsHarness::spec(deferral_spec())
        .build()
        .expect("deferral spec");
    // Fill the store to MAX_ENTITIES - 3 with neutral nodes: nodes are
    // neither unit bodies nor navigation solids, so they consume slots only.
    while h.world().entities().len() < MAX_ENTITIES - 3 {
        h.world_mut()
            .entities_mut()
            .spawn(
                EntityKind::Node(ResourceKind::Crystal),
                OWNER_NEUTRAL,
                [0.5, 0.5],
            )
            .expect("store not yet full");
    }
    h.step_exact(10);
    assert_eq!(
        ghouls(&h).len(),
        3,
        "three free slots -> three spawns, five deferred"
    );
    assert_eq!(h.world().enemies_spawned(), 3);
    // Free five slots; the remainder arrives on the very next tick.
    let nodes = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal));
    for &id in nodes.iter().take(5) {
        assert!(h.world_mut().entities_mut().despawn(id));
    }
    h.step_exact(1);
    assert_eq!(ghouls(&h).len(), 8, "deferred remainder spawns, none lost");
    assert_eq!(h.world().enemies_spawned(), 8);
}
