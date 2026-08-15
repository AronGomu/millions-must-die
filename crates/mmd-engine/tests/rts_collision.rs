//! T4 — hard RTS unit collision: two unit bodies never occupy the same space.
//!
//! Pure logic, no GPU, no clock: every case here is a headless CPU check of
//! `mmd_engine::rts::{units_overlap, moving_circle_hits_point}` and of the
//! movement system inside `RtsWorld::tick`, driven through
//! `testkit::RtsHarness`.
//!
//! What this slice does **not** do, deliberately: push, swap, relax or
//! otherwise resolve a standoff. A rejected candidate simply does not move,
//! so two units walking head-on stop a body diameter apart and stay there.
//! Formation destinations are a later ticket; the invariant proven here is
//! only that no completed tick leaves two bodies merged.

use std::process::Command;

use mmd_engine::nav::field_pool::FieldRef;
use mmd_engine::rts::{
    BuildingKind, EntityId, EntityKind, FormationGoal, GatherPhase, OWNER_PLAYER, Order,
    RTS_UNIT_BODY_DIAMETER_CELLS, RTS_UNIT_BODY_RADIUS_CELLS, ResourceKind, UnitKind,
    moving_circle_hits_point, units_overlap,
};
use mmd_engine::scenario::{Cell, RtsSpec, ScenarioSpec};
use mmd_engine::testkit::RtsHarness;

const W: u32 = 64;
const H: u32 = 64;
const RADIUS: f32 = RTS_UNIT_BODY_RADIUS_CELLS;
const DIAMETER: f32 = RTS_UNIT_BODY_DIAMETER_CELLS;
/// Float slack for a distance that the collision rule holds at exactly
/// `DIAMETER`: the rule itself is exact (`<` on squared distances), the
/// accumulated walk that reaches it is not.
const EPS: f32 = 1e-3;

/// A small, validly-shaped RTS scene whose HQ and resource nodes sit in a far
/// corner, so a case's own geometry never has to account for them. One worker
/// is seeded per entry in `spawns`.
fn collision_spec(spawns: Vec<Cell>, obstacle_cells: Vec<u32>) -> ScenarioSpec {
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
        spawn_cells: spawns,
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 0,
        separation_strength_q8: 0,
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
        obstacle_cells,
        rts: Some(RtsSpec {
            start_crystal: 300,
            start_gas: 100,
            start_supply_cap: 10,
            hq_cell: Cell {
                x: W - 13,
                y: H - 13,
            },
            crystal_nodes: vec![Cell { x: 1, y: H - 2 }],
            gas_nodes: vec![Cell { x: 1, y: H - 3 }],
        }),
    }
}

fn harness(spawns: Vec<Cell>) -> RtsHarness {
    harness_with_obstacles(spawns, vec![])
}

fn harness_with_obstacles(spawns: Vec<Cell>, obstacle_cells: Vec<u32>) -> RtsHarness {
    RtsHarness::spec(collision_spec(spawns, obstacle_cells))
        .build()
        .expect("collision scene harness")
}

/// Spawn cells far enough apart that the seed-time relocation search keeps
/// every one of them; each case then forces its own exact geometry.
fn scattered_spawns(n: u32) -> Vec<Cell> {
    (0..n)
        .map(|i| Cell {
            x: 6 + i * 8,
            y: 50,
        })
        .collect()
}

fn workers(h: &RtsHarness) -> Vec<EntityId> {
    h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))
}

fn pos(h: &RtsHarness, id: EntityId) -> [f32; 2] {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().position(slot)
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    (dx * dx + dy * dy).sqrt()
}

/// Every pair of live units is at least a body diameter apart.
fn assert_no_overlap(h: &RtsHarness, context: &str) {
    let store = h.world().entities();
    let mut live = Vec::new();
    store.collect_live(&mut live);
    let units: Vec<usize> = live
        .into_iter()
        .filter(|&slot| matches!(store.kind(slot), EntityKind::Unit(_)))
        .collect();
    for (n, &a) in units.iter().enumerate() {
        for &b in &units[n + 1..] {
            let d = dist(store.position(a), store.position(b));
            assert!(
                d >= DIAMETER - EPS,
                "{context}: units in slots {a} and {b} are {d} cells apart, \
                 inside the {DIAMETER}-cell body diameter"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Pure geometry
// ---------------------------------------------------------------------------

#[test]
fn touching_bodies_do_not_overlap() {
    assert!(
        !units_overlap([10.0, 10.0], RADIUS, [16.0, 10.0], RADIUS),
        "centres exactly one diameter apart touch, and touching is legal"
    );
    assert!(
        !units_overlap([10.0, 10.0], RADIUS, [10.0, 16.0], RADIUS),
        "the rule is isotropic: touching on the y axis is legal too"
    );
}

#[test]
fn sub_six_distance_penetrates() {
    assert!(
        units_overlap([10.0, 10.0], RADIUS, [15.999, 10.0], RADIUS),
        "0.001 cells inside contact is penetration"
    );
    assert!(
        units_overlap([10.0, 10.0], RADIUS, [10.0, 10.0], RADIUS),
        "two bodies at the same point penetrate"
    );
}

#[test]
fn a_sweep_catches_what_the_endpoints_miss() {
    // A body starting and ending clear of the other, whose straight path
    // passes right through it: the endpoints alone say "legal", the sweep
    // says "no". This is the tunneling case at high speed.
    let from = [0.0, 0.0];
    let to = [40.0, 0.0];
    let other = [20.0, 0.0];
    assert!(
        !units_overlap(from, RADIUS, other, RADIUS),
        "the start endpoint is clear"
    );
    assert!(
        !units_overlap(to, RADIUS, other, RADIUS),
        "the end endpoint is clear"
    );
    assert!(
        moving_circle_hits_point(from, to, RADIUS, other, RADIUS),
        "the swept segment must catch the body it passes straight through"
    );
    // Passing exactly one diameter to the side is contact, not penetration.
    assert!(
        !moving_circle_hits_point(from, to, RADIUS, [20.0, DIAMETER], RADIUS),
        "a sweep grazing at exactly one diameter touches, and touching is legal"
    );
    assert!(
        moving_circle_hits_point(from, to, RADIUS, [20.0, DIAMETER - 0.01], RADIUS),
        "0.01 cells inside contact is penetration"
    );
}

// ---------------------------------------------------------------------------
// Movement
// ---------------------------------------------------------------------------

/// Both units are sent to the *same* midpoint cell, not at each other's own
/// cell: since T5 an order onto a cell another body already stands on resolves
/// to a formation slot beside it, so aiming each unit at the other's feet no
/// longer produces a head-on walk at all. One shared destination does.
#[test]
fn head_on_units_never_penetrate() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    assert!(h.world_mut().force_position_for_test(w[0], [10.5, 20.5]));
    assert!(h.world_mut().force_position_for_test(w[1], [30.5, 20.5]));
    let midpoint = Cell { x: 20, y: 20 };
    assert!(h.world_mut().order_move(w[0], midpoint));
    assert!(h.world_mut().order_move(w[1], midpoint));

    for tick in 0..600 {
        h.step_exact(1);
        assert_no_overlap(&h, &format!("tick {tick}"));
    }

    // ...and the run really did walk them together: a movement system that
    // froze on tick 1 also never overlaps.
    let d = dist(pos(&h, w[0]), pos(&h, w[1]));
    assert!(
        d <= 8.0,
        "the two units started 20 cells apart and must have closed to contact; \
         they are {d} cells apart"
    );
}

#[test]
fn crossing_units_cannot_tunnel() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    assert!(h.world_mut().force_position_for_test(w[0], [10.5, 20.5]));
    assert!(h.world_mut().force_position_for_test(w[1], [20.5, 10.5]));
    assert!(h.world_mut().order_move(w[0], Cell { x: 30, y: 20 }));
    assert!(h.world_mut().order_move(w[1], Cell { x: 20, y: 30 }));

    let mut someone_waited = false;
    for tick in 0..400 {
        let before = [pos(&h, w[0]), pos(&h, w[1])];
        let ordered = [
            matches!(h.world().order_of(w[0]), Some(Order::Move { .. })),
            matches!(h.world().order_of(w[1]), Some(Order::Move { .. })),
        ];
        h.step_exact(1);
        assert_no_overlap(&h, &format!("tick {tick}"));
        for i in 0..2 {
            if ordered[i] && pos(&h, w[i]) == before[i] {
                someone_waited = true;
            }
        }
    }
    assert!(
        someone_waited,
        "two units crossing the same point must make one of them wait a tick, \
         not tunnel past each other"
    );
}

#[test]
fn idle_units_are_collision_bodies() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    let mover = w[0];
    let idle = w[1];
    assert!(h.world_mut().force_position_for_test(mover, [10.5, 20.5]));
    assert!(h.world_mut().force_position_for_test(idle, [20.5, 20.5]));
    assert!(h.world_mut().order_move(mover, Cell { x: 30, y: 20 }));

    let idle_start = pos(&h, idle);
    for tick in 0..300 {
        h.step_exact(1);
        assert_no_overlap(&h, &format!("tick {tick}"));
    }

    let p = pos(&h, mover);
    assert!(
        p[0] > 20.5,
        "the mover must have got past the idle unit's starting spot, by \
         shoving it out of the way; it is at {p:?}"
    );
    let q = pos(&h, idle);
    assert!(
        q != idle_start,
        "an idle unit is a body, not a hole in the world: it must have been \
         displaced rather than walked through"
    );
    assert!(
        q[0] > idle_start[0],
        "the displaced body must have been shoved along the contact normal, \
         away from the mover: {idle_start:?} -> {q:?}"
    );
}

#[test]
fn all_rts_owners_collide() {
    let mut h = harness(scattered_spawns(1));
    let mover = workers(&h)[0];
    // A non-player owner: nothing in phase 1 spawns one, and hard collision
    // must already hold for the enemy units a later phase adds.
    const OWNER_ENEMY: u8 = 1;
    assert_ne!(OWNER_ENEMY, OWNER_PLAYER);
    let enemy = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Soldier),
            OWNER_ENEMY,
            [20.5, 20.5],
        )
        .expect("spawn enemy soldier");
    let enemy_start = pos(&h, enemy);
    assert!(h.world_mut().force_position_for_test(mover, [10.5, 20.5]));
    assert!(h.world_mut().order_move(mover, Cell { x: 30, y: 20 }));

    for tick in 0..300 {
        h.step_exact(1);
        assert_no_overlap(&h, &format!("tick {tick}"));
    }

    let d = dist(pos(&h, mover), pos(&h, enemy));
    assert!(
        d >= DIAMETER - EPS,
        "a player unit must not merge into another owner's body: {d} cells apart"
    );
    assert_ne!(
        pos(&h, enemy),
        enemy_start,
        "another owner's unit is a body like any other: it must have been \
         shoved aside, not walked through"
    );
}

// ---------------------------------------------------------------------------
// Push chains
// ---------------------------------------------------------------------------

/// Place `positions[k]` on worker `k`, in order.
fn park(h: &mut RtsHarness, ids: &[EntityId], positions: &[[f32; 2]]) {
    for (&id, &p) in ids.iter().zip(positions) {
        assert!(h.world_mut().force_position_for_test(id, p));
    }
}

#[test]
fn a_push_chain_moves_a_row_of_bodies() {
    let mut h = harness(scattered_spawns(4));
    let w = workers(&h);
    // A mover and three bodies in a row, each exactly one diameter from the
    // next: freeing the first needs all three to move, which is the shape the
    // tracked scene seeds its starting workers in.
    park(
        &mut h,
        &w,
        &[[10.5, 20.5], [16.5, 20.5], [22.5, 20.5], [28.5, 20.5]],
    );
    let before: Vec<[f32; 2]> = w.iter().map(|&id| pos(&h, id)).collect();
    assert!(h.world_mut().order_move(w[0], Cell { x: 50, y: 20 }));

    h.step_exact(1);

    assert_no_overlap(&h, "after one chained push");
    for k in 0..4 {
        let p = pos(&h, w[k]);
        assert!(
            p[0] > before[k][0],
            "body {k} must have moved east: {:?} -> {p:?}",
            before[k]
        );
    }
}

#[test]
fn a_chain_that_cannot_end_legally_moves_nobody() {
    let mut h = harness(scattered_spawns(4));
    let w = workers(&h);
    // The same row, but backed against the east map edge: the last body has
    // nowhere legal to go, so the whole chain is refused rather than half
    // applied.
    park(
        &mut h,
        &w,
        &[[43.0, 20.5], [49.0, 20.5], [55.0, 20.5], [61.0, 20.5]],
    );
    let before: Vec<[f32; 2]> = w.iter().map(|&id| pos(&h, id)).collect();
    assert!(h.world_mut().order_move(w[0], Cell { x: 60, y: 20 }));

    h.step_exact(1);

    assert_no_overlap(&h, "after a refused chain");
    for k in 1..4 {
        assert_eq!(
            pos(&h, w[k]),
            before[k],
            "body {k} moved even though the chain could not end legally"
        );
    }
}

#[test]
fn a_push_chain_stops_at_its_depth_bound() {
    let mut h = harness(scattered_spawns(5));
    let w = workers(&h);
    // One body deeper than `a_push_chain_moves_a_row_of_bodies`, on open
    // ground with room to spare: the only thing refusing this is the depth
    // bound itself.
    park(
        &mut h,
        &w,
        &[
            [10.5, 20.5],
            [16.5, 20.5],
            [22.5, 20.5],
            [28.5, 20.5],
            [34.5, 20.5],
        ],
    );
    let before: Vec<[f32; 2]> = w.iter().map(|&id| pos(&h, id)).collect();
    assert!(h.world_mut().order_move(w[0], Cell { x: 50, y: 20 }));

    h.step_exact(1);

    assert_no_overlap(&h, "after a chain past the depth bound");
    for k in 1..5 {
        assert_eq!(
            pos(&h, w[k]),
            before[k],
            "body {k} moved: a chain four bodies deep must be refused whole"
        );
    }
}

#[test]
fn no_body_is_displaced_twice_in_one_tick() {
    let mut h = harness(scattered_spawns(3));
    let w = workers(&h);
    // One body, two movers meeting it on perpendicular headings: one shoves it
    // due east, the other due north. Only the first may — a body displaced
    // twice in a tick would come out moved on *both* axes.
    park(&mut h, &w, &[[10.0, 20.5], [16.2, 26.7], [16.2, 20.5]]);
    let body_start = pos(&h, w[2]);
    assert!(h.world_mut().order_move(w[0], Cell { x: 40, y: 20 }));
    assert!(h.world_mut().order_move(w[1], Cell { x: 16, y: 5 }));

    h.step_exact(1);

    assert_no_overlap(&h, "after two movers met one body");
    let p = pos(&h, w[2]);
    let moved_x = p[0] != body_start[0];
    let moved_y = p[1] != body_start[1];
    assert!(
        moved_x || moved_y,
        "the body must have been shoved once: {body_start:?} -> {p:?}"
    );
    assert!(
        moved_x != moved_y,
        "the body moved on both axes ({body_start:?} -> {p:?}): it was shoved \
         by both movers in one tick"
    );
}

// ---------------------------------------------------------------------------
// Deflection
// ---------------------------------------------------------------------------

/// The case this fallback exists for, reduced from the one that was traced on
/// the tracked scene: a body pinned against a wall's clearance, brushed by a
/// mover heading past it. No shove is legal — every direction that would clear
/// the mover drives the body into the wall — so without a deflected heading
/// the mover is frozen for good with open ground beside it.
#[test]
fn a_mover_deflects_around_a_body_it_cannot_shove() {
    // A wall along the cell row `y == 10`, x in 10..30.
    let wall: Vec<u32> = (10..30u32).map(|x| x + 10 * W).collect();
    let mut h = harness_with_obstacles(scattered_spawns(2), wall);
    let w = workers(&h);
    let mover = w[0];
    let wedged = w[1];
    // The wedged body sits exactly one body radius below the wall: it cannot
    // be shoved north (the wall), and the mover's contact normal points north.
    park(&mut h, &w, &[[18.44, 19.68], [20.5, 14.0]]);
    let wedged_start = pos(&h, wedged);
    assert!(h.world_mut().order_move(mover, Cell { x: 40, y: 19 }));

    for tick in 0..400 {
        h.step_exact(1);
        assert_no_overlap(&h, &format!("tick {tick}"));
    }

    assert_eq!(
        pos(&h, wedged),
        wedged_start,
        "the wedged body has nowhere legal to go and must not have moved"
    );
    let p = pos(&h, mover);
    assert!(
        p[0] > 30.0,
        "the mover must have got past the body it could not shove, by taking a \
         deflected heading; it is at {p:?}"
    );
}

#[test]
fn a_shovable_body_is_shoved_rather_than_walked_around() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    // Same approach as the wedged case, but on open ground: the body can be
    // shoved, so the mover must keep its straight heading instead of
    // deflecting. Deflection is the fallback, not the first move.
    park(&mut h, &w, &[[10.5, 20.5], [20.5, 20.5]]);
    assert!(h.world_mut().order_move(w[0], Cell { x: 40, y: 20 }));

    for tick in 0..60 {
        h.step_exact(1);
        assert_no_overlap(&h, &format!("tick {tick}"));
        assert_eq!(
            pos(&h, w[0])[1],
            20.5,
            "the mover deflected off its straight heading on tick {tick} \
             although the body ahead could be shoved"
        );
        assert_eq!(
            pos(&h, w[1])[1],
            20.5,
            "the shove must follow the contact normal, which is due east here"
        );
    }
    assert!(
        pos(&h, w[1])[0] > 20.5,
        "the body ahead must have been shoved east"
    );
}

/// Which of two contending units gets the first proposal rotates with the tick
/// index, so a follower is never permanently starved by its slot number.
///
/// Both worlds are identical except for the tick the contended step lands on:
/// the follower sits just behind the leader, close enough that what happens —
/// the follower walking into space the leader has already vacated, or the
/// follower reaching the leader first and having to shove it — is decided
/// purely by which of the two is proposed first.
#[test]
fn priority_rotates_deterministically() {
    fn contended_step(idle_ticks: u64) -> ([f32; 2], [f32; 2]) {
        let mut h = harness(scattered_spawns(2));
        let w = workers(&h);
        h.step_exact(idle_ticks);
        assert!(h.world_mut().force_position_for_test(w[0], [10.5, 20.5]));
        assert!(h.world_mut().force_position_for_test(w[1], [16.8, 20.5]));
        assert_eq!(
            h.world_mut().order_move_group(&w, Cell { x: 40, y: 20 }),
            Ok(2)
        );
        h.step_exact(1);
        (pos(&h, w[0]), pos(&h, w[1]))
    }

    // Two live units, so the rotation cursor is `tick_index % 2`.
    let leader_first = contended_step(0); // the contended step lands on tick 1
    let follower_first = contended_step(1); // ...and on tick 2 here

    assert_ne!(
        leader_first, follower_first,
        "the traversal order must rotate with the tick: a contended step that \
         lands on an odd tick and one that lands on an even tick cannot leave \
         the same world"
    );
    for (follower, leader) in [leader_first, follower_first] {
        assert!(
            dist(follower, leader) >= DIAMETER - EPS,
            "whichever went first, the two bodies must not have merged: \
             {follower:?} and {leader:?}"
        );
    }
}

#[test]
fn forced_overlap_is_repaired() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    assert!(h.world_mut().force_position_for_test(w[0], [20.5, 20.5]));
    assert!(h.world_mut().force_position_for_test(w[1], [21.0, 20.5]));
    assert!(
        units_overlap(pos(&h, w[0]), RADIUS, pos(&h, w[1]), RADIUS),
        "the forced state must actually be illegal, or this proves nothing"
    );

    h.step_exact(1);

    assert_eq!(
        h.world().last_tick_error(),
        None,
        "an open map always has a legal free centre to repair into"
    );
    assert_no_overlap(&h, "after one repaired tick");
    // The repaired unit must land on a legal centre, not merely a free one.
    let blocked = h.world().static_nav().center_blocked();
    for &id in &w {
        let p = pos(&h, id);
        let idx = (p[0].floor() as u32 + p[1].floor() as u32 * W) as usize;
        assert!(
            !blocked[idx],
            "repaired unit at {p:?} stands on a blocked cell"
        );
    }
}

// ---------------------------------------------------------------------------
// T13 — the gather-pair collision exemption
// ---------------------------------------------------------------------------
//
// Two workers that are both collecting resources may pass through and stand
// on each other; everything else about hard collision is unchanged. The rule
// is deliberately blind to owner and to gather phase, and it exempts a pair
// from *each other only* — never from static geometry, and never from a body
// outside the pair.

/// A never-issued field handle. Every order the mover walks re-checks its
/// cached handle and re-acquires when it is stale, so a forced order does not
/// have to know the pool's internals.
const STALE_FIELD: FieldRef = FieldRef { slot: 0, epoch: 0 };

fn crystal_node(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0]
}

fn hq(h: &RtsHarness) -> EntityId {
    h.ids_of_kind(EntityKind::Building(BuildingKind::Hq))[0]
}

/// One of each gather phase, in the order the round trip runs them.
///
/// `Mining` is parked far from finishing so a case that ticks for a while
/// keeps the phase it asked for instead of advancing into the next one.
fn all_phases(h: &RtsHarness) -> [GatherPhase; 3] {
    let node_cell = Cell { x: 1, y: H - 2 };
    [
        GatherPhase::ToNode {
            goal: FormationGoal {
                anchor: node_cell,
                slot: node_cell,
            },
            field: STALE_FIELD,
        },
        GatherPhase::Mining { ticks_left: 10_000 },
        GatherPhase::Returning {
            drop_off: hq(h),
            field: STALE_FIELD,
        },
    ]
}

/// Park `ids` deep inside one another — half a cell apart, a fifth of the
/// body diameter — so one tick of walking cannot separate them by accident.
fn park_merged(h: &mut RtsHarness, ids: &[EntityId]) {
    let ps: Vec<[f32; 2]> = (0..ids.len())
        .map(|k| [20.5 + k as f32 * 0.5, 20.5])
        .collect();
    park(h, ids, &ps);
}

fn gathering(h: &mut RtsHarness, id: EntityId, phase: GatherPhase) {
    let node = crystal_node(h);
    assert!(
        h.world_mut()
            .force_order_for_test(id, Order::Gather { node, phase }),
        "the order hook must accept a live id"
    );
}

#[test]
fn two_gathering_workers_may_overlap() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    park_merged(&mut h, &w);
    for &id in &w {
        gathering(&mut h, id, GatherPhase::Mining { ticks_left: 10_000 });
    }

    for tick in 0..60 {
        h.step_exact(1);
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "tick {tick}: an active gather pair's overlap is not a violation"
        );
    }

    let d = dist(pos(&h, w[0]), pos(&h, w[1]));
    assert!(
        d < DIAMETER,
        "the pair must still be merged after 60 ticks, not quietly repaired \
         apart: {d} cells"
    );
    assert!(
        h.world().raw_body_overlap_count() >= 1,
        "the raw oracle must see the penetration the policy is forgiving, or \
         the zero above proves nothing"
    );
}

#[test]
fn all_gather_phase_pairs_qualify() {
    for a in 0..3 {
        for b in 0..3 {
            let mut h = harness(scattered_spawns(2));
            let w = workers(&h);
            let phases = all_phases(&h);
            park_merged(&mut h, &w);
            gathering(&mut h, w[0], phases[a]);
            gathering(&mut h, w[1], phases[b]);

            h.step_exact(1);

            assert!(
                h.world().raw_body_overlap_count() >= 1,
                "phases ({a}, {b}): the pair must still be geometrically merged"
            );
            assert_eq!(
                h.world().body_overlap_count(),
                0,
                "phases ({a}, {b}) must be exempt: all nine combinations count \
                 as gathering"
            );
        }
    }
}

#[test]
fn gather_exemption_is_owner_blind() {
    let mut h = harness(scattered_spawns(1));
    let mine = workers(&h)[0];
    const OWNER_ENEMY: u8 = 1;
    assert_ne!(OWNER_ENEMY, OWNER_PLAYER);
    let theirs = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Worker),
            OWNER_ENEMY,
            [20.5, 20.5],
        )
        .expect("spawn enemy worker");
    park_merged(&mut h, &[mine, theirs]);
    for &id in &[mine, theirs] {
        gathering(&mut h, id, GatherPhase::Mining { ticks_left: 10_000 });
    }

    h.step_exact(30);

    assert!(
        h.world().raw_body_overlap_count() >= 1,
        "the two bodies must still be merged"
    );
    assert_eq!(
        h.world().body_overlap_count(),
        0,
        "a resource cluster is crowded by whoever works it: the exemption \
         must not check ownership"
    );
}

#[test]
fn one_non_gather_worker_keeps_pair_hard() {
    // The gate is *both* sides gathering. Idle and Move are the two ways the
    // other side can fail it.
    for other in [None, Some(Cell { x: 40, y: 20 })] {
        let mut h = harness(scattered_spawns(2));
        let w = workers(&h);
        park_merged(&mut h, &w);
        gathering(&mut h, w[0], GatherPhase::Mining { ticks_left: 10_000 });
        if let Some(dest) = other {
            assert!(h.world_mut().order_move(w[1], dest));
        }

        h.step_exact(1);

        assert_eq!(
            h.world().last_tick_error(),
            None,
            "the open map has a legal free centre to repair into"
        );
        assert_no_overlap(&h, "a half-gathering pair is a hard pair");
        assert_eq!(h.world().raw_body_overlap_count(), 0);
    }
}

#[test]
fn worker_soldier_pair_stays_hard() {
    let mut h = harness(scattered_spawns(1));
    let worker = workers(&h)[0];
    let soldier = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Soldier),
            OWNER_PLAYER,
            [20.5, 20.5],
        )
        .expect("spawn soldier");
    park_merged(&mut h, &[worker, soldier]);
    // Both under a gather order: only the *kind* gate may refuse this pair,
    // so a soldier that somehow holds one must still be a hard body.
    for &id in &[worker, soldier] {
        gathering(&mut h, id, GatherPhase::Mining { ticks_left: 10_000 });
    }

    h.step_exact(1);

    assert_no_overlap(&h, "a soldier is never half of a gather pair");
    assert_eq!(h.world().raw_body_overlap_count(), 0);
}

#[test]
fn gather_workers_still_hit_static_geometry() {
    // A wall across the whole map at `y == 10`, with one body-wide gap at
    // `x in 54..62`: the only way north is through the gap.
    let wall: Vec<u32> = (0..W)
        .filter(|x| !(54..62).contains(x))
        .map(|x| x + 10 * W)
        .collect();
    let mut h = harness_with_obstacles(scattered_spawns(2), wall);
    let w = workers(&h);
    park_merged(&mut h, &w);
    // Both walking at a cell on the far side of the wall, and exempt from each
    // other — so nothing but static geometry is left to stop them.
    let north = Cell { x: 20, y: 4 };
    for &id in &w {
        gathering(
            &mut h,
            id,
            GatherPhase::ToNode {
                goal: FormationGoal {
                    anchor: north,
                    slot: north,
                },
                field: STALE_FIELD,
            },
        );
    }
    let start: Vec<[f32; 2]> = w.iter().map(|&id| pos(&h, id)).collect();

    let mut saw_merged = false;
    for tick in 0..600 {
        h.step_exact(1);
        saw_merged |= h.world().raw_body_overlap_count() >= 1;
        for &id in &w {
            let p = pos(&h, id);
            assert!(
                h.world().static_nav().position_clear(p, RADIUS),
                "tick {tick}: an exempt gather body at {p:?} is inside static \
                 geometry"
            );
        }
    }

    assert!(
        saw_merged,
        "the pair must have been merged while walking, or the static rule was \
         never tested against an exempt pair"
    );
    for (k, &id) in w.iter().enumerate() {
        assert_ne!(
            pos(&h, id),
            start[k],
            "worker {k} never moved: this measured nothing"
        );
    }
}

#[test]
fn nested_push_checks_use_pair_policy() {
    let mut h = harness(scattered_spawns(3));
    let w = workers(&h);
    let (mover, near, far) = (w[0], w[1], w[2]);
    // The row `a_push_chain_moves_a_row_of_bodies` uses, but the two bodies
    // ahead of the mover are an exempt gather pair. The mover is not part of
    // it, so it still has to shove `near`; `near`'s displacement runs into
    // `far`, and *that* check is the nested one the policy must reach.
    park(&mut h, &w, &[[10.5, 20.5], [16.5, 20.5], [22.5, 20.5]]);
    for &id in &[near, far] {
        gathering(&mut h, id, GatherPhase::Mining { ticks_left: 10_000 });
    }
    let before: Vec<[f32; 2]> = w.iter().map(|&id| pos(&h, id)).collect();
    assert!(h.world_mut().order_move(mover, Cell { x: 50, y: 20 }));

    h.step_exact(1);

    assert!(
        pos(&h, mover)[0] > before[0][0],
        "the chain must not be falsely rejected: the mover stood still"
    );
    assert!(
        pos(&h, near)[0] > before[1][0],
        "the mover is not in the exempt pair, so `near` must still be shoved"
    );
    assert_eq!(
        pos(&h, far),
        before[2],
        "`near` and `far` are exempt from each other, so the shove must not \
         have propagated to `far`"
    );
    assert!(
        units_overlap(pos(&h, near), RADIUS, pos(&h, far), RADIUS),
        "the nested check really was reached: `near` landed inside `far`"
    );
    assert!(
        !units_overlap(pos(&h, mover), RADIUS, pos(&h, near), RADIUS),
        "the mover/displaced check is not exempt and must still hold"
    );
    assert_eq!(
        h.world().body_overlap_count(),
        0,
        "no hard pair may be left merged by an exempt chain"
    );
}

#[test]
fn body_overlap_count_reports_policy_violations() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    park_merged(&mut h, &w);
    for &id in &w {
        gathering(&mut h, id, GatherPhase::Mining { ticks_left: 10_000 });
    }
    h.step_exact(1);
    assert_eq!(
        h.world().body_overlap_count(),
        0,
        "the exempt pair's overlap is not a violation"
    );

    // A third body dropped on top of the pair is in no exempt pair at all, so
    // both of its overlaps are violations — while the pair's own stays free.
    let on_top = pos(&h, w[0]);
    let intruder = h
        .world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, on_top)
        .expect("spawn intruder");
    assert!(h.world().entities().slot(intruder).is_some());

    assert_eq!(
        h.world().raw_body_overlap_count(),
        3,
        "three merged bodies are three penetrating pairs, geometrically"
    );
    assert_eq!(
        h.world().body_overlap_count(),
        2,
        "only the two pairs the policy does not exempt are violations"
    );
}

// ---------------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------------

/// Env flag + test name used to re-enter this binary as a child process.
const CHILD_ENV: &str = "MMD_RTS_COLLISION_CHILD_HASH";
const CHILD_TEST: &str = "print_collision_hash_for_child_process";
const HASH_PREFIX: &str = "MMD_RTS_COLLISION_HASH=";

/// Four workers sent to one destination cell: they contend, are rejected,
/// re-propose and rotate priority for the whole run.
fn canonical_hash() -> String {
    let mut h = harness(scattered_spawns(4));
    let w = workers(&h);
    assert_eq!(w.len(), 4);
    assert_eq!(
        h.world_mut().order_move_group(&w, Cell { x: 20, y: 20 }),
        Ok(4)
    );
    h.step_exact(400);
    h.state_hash_hex()
}

/// Child entry point for `hard_collision_is_reproducible`. A *fixture*, not a
/// check: it asserts nothing about collision and is `#[ignore]`d so a normal
/// run does not count it as a passing test.
#[test]
#[ignore = "child entry point driven by hard_collision_is_reproducible"]
fn print_collision_hash_for_child_process() {
    assert!(
        std::env::var_os(CHILD_ENV).is_some(),
        "child entry point must only run via the parent, which sets {CHILD_ENV}"
    );
    println!("{HASH_PREFIX}{}", canonical_hash());
}

fn hash_from_child_process() -> String {
    let exe = std::env::current_exe().expect("current test binary");
    let out = Command::new(&exe)
        .args([
            "--exact",
            CHILD_TEST,
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(CHILD_ENV, "1")
        // Coverage runs point this at the parent's profile file; letting the
        // child inherit it would clobber the parent's data.
        .env_remove("LLVM_PROFILE_FILE")
        .output()
        .expect("spawn child test process");
    assert!(
        out.status.success(),
        "child process failed: {}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1 passed"),
        "child ran no test — filter or test name drifted:\n{stdout}"
    );
    let tail = stdout
        .find(HASH_PREFIX)
        .map(|i| &stdout[i + HASH_PREFIX.len()..])
        .unwrap_or_else(|| panic!("child printed no `{HASH_PREFIX}` marker:\n{stdout}"));
    let hash: String = tail.chars().take_while(char::is_ascii_hexdigit).collect();
    assert_eq!(hash.len(), 64, "child hash malformed: {hash:?}");
    hash
}

#[test]
fn hard_collision_is_reproducible() {
    let first = canonical_hash();
    let second = canonical_hash();
    assert_eq!(
        first, second,
        "a contended collision run must reproduce its state hash in-process"
    );
    assert_eq!(
        first,
        hash_from_child_process(),
        "a contended collision run must reproduce its state hash across processes"
    );
}
