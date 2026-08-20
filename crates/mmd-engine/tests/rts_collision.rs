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
    BuildingKind, EntityId, EntityKind, FormationGoal, GATHER_PAIR_ACTIVE,
    GATHER_SEPARATION_STEP_CELLS, GATHER_SEPARATION_TICKS, GatherPhase, OWNER_PLAYER, Order,
    RTS_UNIT_BODY_DIAMETER_CELLS, RTS_UNIT_BODY_RADIUS_CELLS, ResourceKind, TickError, UnitKind,
    moving_circle_hits_point, units_overlap,
};
use mmd_engine::scenario::{BUILD_SQUARE_CELLS, Cell, HQ_FOOTPRINT_CELLS, RtsSpec, ScenarioSpec};
use mmd_engine::testkit::RtsHarness;

mod common;
use common::{ONE_FREE_CENTRE, one_free_centre_spec};

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
                x: (W - HQ_FOOTPRINT_CELLS) / BUILD_SQUARE_CELLS * BUILD_SQUARE_CELLS,
                y: (H - HQ_FOOTPRINT_CELLS) / BUILD_SQUARE_CELLS * BUILD_SQUARE_CELLS,
            },
            crystal_nodes: vec![Cell { x: 1, y: H - 2 }],
            gas_nodes: vec![Cell { x: 1, y: H - 3 }],
            enemies: None,
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
    // A non-player owner: hard collision is owner-blind, and must hold for
    // the enemy units later phases add.
    //
    // Deliberately an **unarmed** kind. The claim under test is about bodies,
    // not about fire: an armed enemy auto-acquires this mover the moment it
    // walks into reach and kills it long before 300 ticks are up, which would
    // make this case fail on a combat fact rather than a collision one. The
    // two kinds share one body radius, so nothing about the geometry changes.
    const OWNER_ENEMY: u8 = 1;
    assert_ne!(OWNER_ENEMY, OWNER_PLAYER);
    let enemy = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Worker),
            OWNER_ENEMY,
            [20.5, 20.5],
        )
        .expect("spawn enemy worker");
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
    assert_eq!(
        h.world().overlap_repair_runs(),
        1,
        "the forced overlap must have armed exactly one repair pass"
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

/// An overlap the grid cannot repair is reported on **every** tick, not just
/// the first: the pass stays armed until it ends clean, so a world holding an
/// unrepairable penetration never quietly resumes moving.
///
/// The scene's grid holds exactly one legal body centre, which the seeded
/// worker already stands on, so a second body forced onto the same point has
/// nowhere at all to go.
#[test]
fn an_unrepairable_overlap_is_reported_on_every_tick() {
    let mut h = RtsHarness::spec(one_free_centre_spec())
        .build()
        .expect("one-free-centre scene");
    let seeded = workers(&h);
    assert_eq!(seeded.len(), 1, "the pocket scene seeds one worker");
    assert_eq!(
        pos(&h, seeded[0]),
        ONE_FREE_CENTRE,
        "the seeded worker must stand on the grid's only legal body centre"
    );

    let twin = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Worker),
            OWNER_PLAYER,
            ONE_FREE_CENTRE,
        )
        .expect("spawn a second worker on the same point");
    assert!(
        units_overlap(pos(&h, seeded[0]), RADIUS, pos(&h, twin), RADIUS),
        "the forced state must actually be illegal, or this proves nothing"
    );

    for tick in 1..=2u64 {
        h.step_exact(1);
        assert_eq!(
            h.world().last_tick_error(),
            Some(TickError::UnrepairableOverlap),
            "tick {tick} must report the overlap it could not repair"
        );
        assert_eq!(
            h.world().overlap_repair_runs(),
            tick,
            "the pass must stay armed and retry on tick {tick}"
        );
        assert_eq!(
            pos(&h, seeded[0]),
            ONE_FREE_CENTRE,
            "an unrepairable tick moves nobody"
        );
        assert_eq!(
            pos(&h, twin),
            ONE_FREE_CENTRE,
            "an unrepairable tick moves nobody"
        );
    }
}

// ---------------------------------------------------------------------------
// The repair pass is not part of the game
// ---------------------------------------------------------------------------

/// The tracked scene, seeded and then walked as a gathering group, never arms
/// the overlap-repair pass — and never merges two bodies either.
///
/// This is the invariant the shipping build depends on: it compiles no repair
/// pass at all, so if seeding or movement could hand the world a penetration,
/// nothing would clear it. `overlap_repair_runs() == 0` is the proof that the
/// pass never had anything to do here; `body_overlap_count() == 0` is the
/// proof that this is because there was no overlap, not because the oracle is
/// blind.
#[test]
fn movement_never_runs_overlap_repair() {
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let group = workers(&h);
    assert_eq!(group.len(), 6, "the tracked scene seeds six workers");
    assert_eq!(
        h.world().body_overlap_count(),
        0,
        "the seed path handed the world a merged pair"
    );
    assert_eq!(
        h.world().overlap_repair_runs(),
        0,
        "seeding must not need the repair pass"
    );

    let node = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal))[0];
    assert!(
        h.world_mut().order_gather_group(&group, node).is_ok(),
        "the whole group must take the gather order"
    );

    for tick in 1..=600u64 {
        h.step_exact(1);
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "tick {tick} ended with a merged pair"
        );
        assert_eq!(
            h.world().overlap_repair_runs(),
            0,
            "tick {tick} ran the overlap repair pass on a normal movement path"
        );
    }
}

/// A repair-capable run and a repair-disabled run of the same world end in the
/// same state, hash for hash.
///
/// Arming the pass on a valid world is a no-op — that is exactly why the
/// shipping build may omit it. The armed run re-arms before every tick (an
/// identity reposition writes the body's own coordinates back, which changes
/// no state), so the only difference between the two worlds is whether the
/// pass ran at all: 400 times against 0.
#[test]
fn arming_overlap_repair_on_a_valid_world_changes_no_state() {
    fn contended_run() -> RtsHarness {
        let mut h = harness(scattered_spawns(4));
        let w = workers(&h);
        assert_eq!(w.len(), 4);
        assert_eq!(
            h.world_mut().order_move_group(&w, Cell { x: 20, y: 20 }),
            Ok(4)
        );
        h
    }

    let mut disabled = contended_run();
    disabled.step_exact(400);
    assert_eq!(
        disabled.world().overlap_repair_runs(),
        0,
        "a run that touches no test hook must never run the repair pass"
    );

    let mut armed = contended_run();
    let first = workers(&armed)[0];
    for _ in 0..400 {
        let p = pos(&armed, first);
        assert!(
            armed.world_mut().force_position_for_test(first, p),
            "the arming reposition must accept a live id"
        );
        armed.step_exact(1);
    }
    assert_eq!(
        armed.world().overlap_repair_runs(),
        400,
        "the armed run must have run the pass on every tick"
    );

    assert_eq!(
        disabled.state_hash_hex(),
        armed.state_hash_hex(),
        "the repair pass changed a valid world's state; the shipping build \
         omits it, so it must change nothing"
    );
    assert_eq!(
        disabled.state_hash_hex(),
        canonical_hash(),
        "the repair-disabled run must still be the canonical contended run the \
         cross-process determinism test hashes"
    );
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

fn hash_from_child_process(child_env: &str, child_test: &str, prefix: &str) -> String {
    let exe = std::env::current_exe().expect("current test binary");
    let out = Command::new(&exe)
        .args([
            "--exact",
            child_test,
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(child_env, "1")
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
        .find(prefix)
        .map(|i| &stdout[i + prefix.len()..])
        .unwrap_or_else(|| panic!("child printed no `{prefix}` marker:\n{stdout}"));
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
        hash_from_child_process(CHILD_ENV, CHILD_TEST, HASH_PREFIX),
        "a contended collision run must reproduce its state hash across processes"
    );
}

// ---------------------------------------------------------------------------
// T14 — the bounded gather-exit transition
// ---------------------------------------------------------------------------
//
// An exemption never outlives the order that earned it, but revoking one in a
// single frame teleports two merged workers apart in front of the player. So
// a pair that has *stopped* gathering keeps its exemption for at most
// `GATHER_SEPARATION_TICKS` separation attempts of `GATHER_SEPARATION_STEP_CELLS`
// each, and the tick that spends the last one relocates instead. Either way
// the pair's byte is back to `0` — hard, counted, repairable — the moment the
// bound is gone. There is no state in which a pair keeps an exemption it did
// not earn.

/// How far one attempt may move one body.
const STEP: f32 = GATHER_SEPARATION_STEP_CELLS;
/// The whole bound, in completed attempts.
const BOUND: u8 = GATHER_SEPARATION_TICKS;

/// Cancel every gather order, which is what starts an exit.
fn exit_gather(h: &mut RtsHarness, ids: &[EntityId]) {
    for &id in ids {
        assert!(h.world_mut().force_order_for_test(id, Order::Idle));
    }
}

/// Put `ids` under a `Mining` gather order and run one tick, so the pair table
/// carries this tick's active-gather provenance into the exit below.
fn gather_one_tick(h: &mut RtsHarness, ids: &[EntityId]) {
    for &id in ids {
        gathering(h, id, GatherPhase::Mining { ticks_left: 10_000 });
    }
    h.step_exact(1);
    for (n, &a) in ids.iter().enumerate() {
        for &b in &ids[n + 1..] {
            assert_eq!(
                h.world().gather_pair_state_for_test(a, b),
                GATHER_PAIR_ACTIVE,
                "every active gather pair must carry provenance before the exit"
            );
        }
    }
}

fn pair_state(h: &RtsHarness, a: EntityId, b: EntityId) -> u8 {
    h.world().gather_pair_state_for_test(a, b)
}

/// Advance one tick and hand back every unit's displacement over it.
fn step_and_measure(h: &mut RtsHarness, ids: &[EntityId]) -> Vec<f32> {
    let before: Vec<[f32; 2]> = ids.iter().map(|&id| pos(h, id)).collect();
    h.step_exact(1);
    ids.iter()
        .enumerate()
        .map(|(k, &id)| dist(before[k], pos(h, id)))
        .collect()
}

/// [`gather_one_tick`], but padded so the movement pass that the *next*
/// `step_exact(1)` runs starts its rotation at `start`.
///
/// The rotation is `tick_index % unit_count` and `tick` increments the index
/// before the movement pass, so the next pass runs at `tick_index() + 1`. The
/// padding is spent *while the pair is still gathering* on purpose: an active
/// pair is skipped by the transition pass entirely, so no attempt of the bound
/// under test is quietly spent lining the rotation up.
fn gather_until_rotation(h: &mut RtsHarness, ids: &[EntityId], units: u64, start: u64) {
    for &id in ids {
        gathering(h, id, GatherPhase::Mining { ticks_left: 10_000 });
    }
    for _ in 0..=units {
        h.step_exact(1);
        if (h.tick_index() + 1) % units == start {
            return;
        }
    }
    panic!("no tick in one full rotation starts at {start}");
}

#[test]
fn active_pair_overlaps_during_movement_then_exits_into_transition() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    park_merged(&mut h, &w);
    gather_one_tick(&mut h, &w);
    assert!(h.world().raw_body_overlap_count() >= 1);

    exit_gather(&mut h, &w);
    let moved = step_and_measure(&mut h, &w);

    // A transition, not a repair: the exemption is gone, but nothing was
    // teleported to a free cell centre to pay for it.
    for (k, &d) in moved.iter().enumerate() {
        assert!(
            d <= STEP + EPS,
            "worker {k} moved {d} cells in one exit tick: that is a relocation, \
             not a bounded separation attempt"
        );
    }
    assert_eq!(
        pair_state(&h, w[0], w[1]),
        1,
        "the pair must be one completed attempt into its bound"
    );
    assert!(
        h.world().raw_body_overlap_count() >= 1,
        "one {STEP}-cell attempt cannot clear a body-deep penetration, so the \
         case would be measuring nothing"
    );
    assert_eq!(
        h.world().body_overlap_count(),
        0,
        "a pair inside its exit bound is not a policy violation"
    );
    assert_eq!(h.world().last_tick_error(), None);
}

#[test]
fn first_attempt_stores_one_not_two_fifty_six() {
    // The one arithmetic the encoding cannot get wrong: the byte a pair
    // carries out of an active gather is `GATHER_PAIR_ACTIVE`, and the first
    // completed attempt has to *restart* the count at 1. Incrementing it
    // instead wraps to 0 and drops the pair straight into the repair pass;
    // leaving it alone gives the pair an exemption with no bound at all.
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    park_merged(&mut h, &w);
    gather_one_tick(&mut h, &w);
    assert_eq!(pair_state(&h, w[0], w[1]), GATHER_PAIR_ACTIVE);

    exit_gather(&mut h, &w);
    h.step_exact(1);

    assert_eq!(pair_state(&h, w[0], w[1]), 1);
}

#[test]
fn exited_pair_separates_gradually() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    park_merged(&mut h, &w);
    gather_one_tick(&mut h, &w);
    exit_gather(&mut h, &w);

    // Parked half a cell apart, so 11 attempts of half a cell each is exactly
    // what reaching contact costs.
    let mut last = dist(pos(&h, w[0]), pos(&h, w[1]));
    for attempt in 1..=11u8 {
        let moved = step_and_measure(&mut h, &w);
        let now = dist(pos(&h, w[0]), pos(&h, w[1]));
        assert!(
            now >= last - EPS,
            "attempt {attempt}: the pair closed up, from {last} to {now}"
        );
        assert!(
            now - last <= STEP + EPS,
            "attempt {attempt}: the pair jumped {} cells, which is a teleport",
            now - last
        );
        assert!(
            moved.iter().all(|&d| d <= STEP + EPS),
            "attempt {attempt}: a body moved further than one attempt allows"
        );
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "attempt {attempt}: the pair is still inside its bound"
        );
        assert_eq!(h.world().last_tick_error(), None, "attempt {attempt}");
        last = now;
    }

    assert!(
        last >= DIAMETER - EPS,
        "eleven attempts must have carried the pair to contact: {last} cells"
    );
    assert_eq!(
        pair_state(&h, w[0], w[1]),
        0,
        "a settled exit is hard again"
    );
    assert_eq!(h.world().raw_body_overlap_count(), 0);
}

#[test]
fn separated_pair_clears_to_hard_in_the_same_tick() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    // Exactly one attempt from contact, so the exit finishes on its first try.
    park(&mut h, &w, &[[20.5, 20.5], [26.0, 20.5]]);
    gather_one_tick(&mut h, &w);
    exit_gather(&mut h, &w);

    h.step_exact(1);

    let d = dist(pos(&h, w[0]), pos(&h, w[1]));
    assert!(
        d >= DIAMETER - EPS,
        "one attempt reaches contact: {d} cells"
    );
    assert_eq!(
        pair_state(&h, w[0], w[1]),
        0,
        "the attempt that reaches contact must clear the byte in that same \
         tick, not leave the pair one attempt in"
    );
    assert_eq!(h.world().raw_body_overlap_count(), 0);
    assert_eq!(h.world().body_overlap_count(), 0);

    // And the byte really is the hard one: forced back together, this pair is
    // now repaired outright rather than given another bounded exit — a repair
    // separates them completely in one tick, an attempt would move half a cell.
    park(&mut h, &w, &[[20.5, 20.5], [21.0, 20.5]]);
    h.step_exact(1);
    let d = dist(pos(&h, w[0]), pos(&h, w[1]));
    assert!(
        d >= DIAMETER - EPS,
        "a cleared pair is a hard pair: the generic repair must separate it \
         whole, but it is {d} cells apart"
    );
}

#[test]
fn separated_pair_cannot_remerge_on_the_tick_it_cleared() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    let (walker, standing) = (w[0], w[1]);
    park(&mut h, &w, &[[20.5, 20.5], [26.0, 20.5]]);
    // The standing body takes the first proposal, so the walk below starts
    // from exactly where it was measured rather than from a separation step.
    gather_until_rotation(&mut h, &w, 2, 1);

    // The walker leaves the gather and is sent *through* its old partner. The
    // separation pass clears the pair on this very tick; normal movement runs
    // afterwards in the same tick and must already see a hard body.
    assert!(h.world_mut().force_order_for_test(walker, Order::Idle));
    assert!(h.world_mut().order_move(walker, Cell { x: 40, y: 20 }));
    let before = pos(&h, walker);

    h.step_exact(1);

    assert_eq!(pair_state(&h, walker, standing), 0);
    assert!(
        pos(&h, walker)[0] > before[0],
        "the walker never moved: the case measured no normal movement at all"
    );
    assert_eq!(
        h.world().raw_body_overlap_count(),
        0,
        "the pair separated and then walked straight back into itself on the \
         same tick"
    );
    assert_eq!(h.world().body_overlap_count(), 0);
}

#[test]
fn open_concentric_pair_reaches_contact_by_attempt_twelve() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    // The worst exit there is: no separation vector at all, and a whole body
    // diameter to cover.
    park(&mut h, &w, &[[20.5, 20.5], [20.5, 20.5]]);
    gather_one_tick(&mut h, &w);
    exit_gather(&mut h, &w);

    for attempt in 1..BOUND {
        let moved = step_and_measure(&mut h, &w);
        assert!(
            moved.iter().all(|&d| d <= STEP + EPS),
            "attempt {attempt}: a body moved further than one attempt allows, \
             so the fallback fired early"
        );
        assert_eq!(
            pair_state(&h, w[0], w[1]),
            attempt,
            "attempt {attempt}: the count must rise by exactly one per tick"
        );
        assert!(h.world().raw_body_overlap_count() >= 1);
        assert_eq!(h.world().body_overlap_count(), 0);
    }

    let moved = step_and_measure(&mut h, &w);
    assert!(
        moved.iter().all(|&d| d <= STEP + EPS),
        "the last attempt must reach contact by walking, not by relocating"
    );
    let d = dist(pos(&h, w[0]), pos(&h, w[1]));
    assert!(
        d >= DIAMETER - EPS,
        "the bound is only honest if the worst case fits inside it: {d} cells \
         after {BOUND} attempts"
    );
    assert_eq!(pair_state(&h, w[0], w[1]), 0);
    assert_eq!(h.world().raw_body_overlap_count(), 0);
    assert_eq!(
        h.world().last_tick_error(),
        None,
        "no fallback may have fired"
    );
}

#[test]
fn concentric_normal_moves_lower_slot_negative() {
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    park(&mut h, &w, &[[20.5, 20.5], [20.5, 20.5]]);
    gather_one_tick(&mut h, &w);
    let start: Vec<[f32; 2]> = w.iter().map(|&id| pos(&h, id)).collect();
    let slots: Vec<usize> = w
        .iter()
        .map(|&id| h.world().entities().slot(id).expect("live"))
        .collect();
    assert!(slots[0] < slots[1], "w[0] is the lower slot");

    exit_gather(&mut h, &w);
    // One tick each: the tick rotation hands the first proposal to the other
    // body every tick, and only one body moves per attempt.
    h.step_exact(2);

    let low = [
        pos(&h, w[0])[0] - start[0][0],
        pos(&h, w[0])[1] - start[0][1],
    ];
    let high = [
        pos(&h, w[1])[0] - start[1][0],
        pos(&h, w[1])[1] - start[1][1],
    ];
    assert_eq!(
        high,
        [-low[0], -low[1]],
        "the two bodies must separate along one axis in opposite senses"
    );
    assert_eq!(
        high[0] * high[0] + high[1] * high[1],
        STEP * STEP,
        "each body moved exactly one attempt: {high:?}"
    );
    assert!(
        high[0] == 0.0 || high[1] == 0.0,
        "the concentric normal must be cardinal: {high:?}"
    );
    let normal = [high[0] / STEP, high[1] / STEP];
    assert_eq!(
        low,
        [-normal[0] * STEP, -normal[1] * STEP],
        "the normal points from the lower slot toward the higher one, so the \
         lower slot moves along -normal"
    );
}

// --- blocked exits: a crowd the gradual attempt cannot get past -------------

/// Two merged workers with a hard body parked just outside contact on each
/// side of the separation axis, so every gradual attempt is refused by a
/// third-body sweep and the exit has to run its whole bound out.
///
/// `w[0]` and `w[1]` are the merged pair, `w[2]` and `w[3]` the blockers. The
/// 6.4-cell offsets are outside the 6-cell body diameter — nothing starts
/// merged but the pair — and a half-cell attempt closes them to 5.9, which is
/// inside it.
fn blocked_exit_scene() -> (RtsHarness, Vec<EntityId>) {
    let mut h = harness(scattered_spawns(4));
    let w = workers(&h);
    park(
        &mut h,
        &w,
        &[
            [20.5, 20.5],
            [21.0, 20.5],
            [20.5 - 6.4, 20.5],
            [21.0 + 6.4, 20.5],
        ],
    );
    gather_one_tick(&mut h, &w[..2]);
    assert_eq!(
        h.world().raw_body_overlap_count(),
        1,
        "only the pair may start merged, or the blockers are too close"
    );
    (h, w)
}

#[test]
fn blocked_pair_relocates_after_attempt_twelve_same_tick() {
    let (mut h, w) = blocked_exit_scene();
    exit_gather(&mut h, &w[..2]);

    for attempt in 1..BOUND {
        let moved = step_and_measure(&mut h, &w);
        assert!(
            moved.iter().all(|&d| d == 0.0),
            "attempt {attempt}: every gradual candidate is blocked, so nobody \
             may move: {moved:?}"
        );
        assert_eq!(pair_state(&h, w[0], w[1]), attempt);
        assert_eq!(h.world().body_overlap_count(), 0);
    }

    let moved = step_and_measure(&mut h, &w);

    let relocated = moved.iter().filter(|&&d| d > STEP + EPS).count();
    assert_eq!(
        relocated, 1,
        "the tick that spends the last attempt must relocate exactly one of \
         the two, in that same tick: {moved:?}"
    );
    let p = pos(&h, if moved[0] > 0.0 { w[0] } else { w[1] });
    assert_eq!(
        [p[0].fract(), p[1].fract()],
        [0.5, 0.5],
        "a relocation lands on a legal cell centre: {p:?}"
    );
    assert_eq!(
        pair_state(&h, w[0], w[1]),
        0,
        "the bound is spent: the pair is hard again, whatever happened"
    );
    assert_eq!(h.world().last_tick_error(), None);
    assert_eq!(h.world().body_overlap_count(), 0);
    assert_no_overlap(&h, "after a fallback relocation");
}

#[test]
fn transition_never_exempts_third_body() {
    // The blockers are the third bodies, and the only reason the pair cannot
    // separate is that its attempts keep running into them. A transition that
    // leaked past its own pair would walk straight through one.
    let (mut h, w) = blocked_exit_scene();
    exit_gather(&mut h, &w[..2]);

    for attempt in 1..BOUND {
        h.step_exact(1);
        assert_eq!(
            h.world().raw_body_overlap_count(),
            1,
            "attempt {attempt}: exactly one geometric overlap — the pair's own"
        );
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "attempt {attempt}: and it is not a violation"
        );
        for k in 2..4 {
            for m in 0..2 {
                let d = dist(pos(&h, w[m]), pos(&h, w[k]));
                assert!(
                    d >= DIAMETER - EPS,
                    "attempt {attempt}: transitioning body {m} is {d} cells \
                     from blocker {k}, inside its body"
                );
            }
        }
    }
}

#[test]
fn normal_movement_respects_transition_pair() {
    // A pair may be transitioning and one of its bodies may be walking at the
    // same time. The walk is gated by the same pair table the separation is,
    // so the walker passes over its transition partner and over nobody else.
    let (mut h, w) = blocked_exit_scene();
    let (walker, partner) = (w[0], w[1]);
    exit_gather(&mut h, &w[..2]);
    // North, across the pair's own east-west separation axis, so the walk and
    // the attempts do not fight over the same direction.
    assert!(h.world_mut().order_move(walker, Cell { x: 20, y: 8 }));
    let start = pos(&h, walker);

    for tick in 1..=3 {
        h.step_exact(1);
        assert!(
            h.world().raw_body_overlap_count() >= 1,
            "tick {tick}: the walker left its partner behind rather than \
             walking over it, so the exemption was never exercised"
        );
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "tick {tick}: the pair is still inside its bound"
        );
        assert!(
            pair_state(&h, walker, partner) <= BOUND,
            "tick {tick}: the transition byte left its range"
        );
    }
    assert!(
        pos(&h, walker)[1] < start[1] - EPS,
        "the walker never moved: normal movement did not run at all"
    );
}

#[test]
fn fallback_priority_rotates() {
    // Which of the two the fallback relocates is the same tick-rotated rank a
    // walk uses, so the cost of a stuck exit is not paid by the same body
    // forever. Two runs of one scene, differing only in the tick the last
    // attempt lands on.
    let mut relocated = Vec::new();
    for pad in [0u32, 1] {
        let (mut h, w) = blocked_exit_scene();
        for _ in 0..pad {
            h.step_exact(1);
        }
        exit_gather(&mut h, &w[..2]);
        h.step_exact(BOUND as u64 - 1);
        let moved = step_and_measure(&mut h, &w);
        assert_eq!(
            moved.iter().filter(|&&d| d > STEP + EPS).count(),
            1,
            "pad {pad}: exactly one body relocates"
        );
        relocated.push(if moved[0] > 0.0 { 0 } else { 1 });
    }
    assert_ne!(
        relocated[0], relocated[1],
        "one tick of difference must hand the relocation to the other body"
    );
}

// --- pocket scenes: exits with nowhere to go -------------------------------

/// Grid edge of the pocket scenes below.
const POCKET_GRID: u32 = 48;
/// Half-open cell rectangle `[x0, x1) x [y0, y1)`.
type OpenRect = (u32, u32, u32, u32);
/// The HQ's own 24 x 24 footprint, which `StaticNav` stamps solid at load, so
/// this block holds no legal body centre at all.
const POCKET_HQ: OpenRect = (0, 24, 0, 24);
/// Four cells wide: raw-walkable, so the scenario's point-agent reachability
/// check passes, and far too narrow for a 3-cell body to stand or pass.
const POCKET_CORRIDOR: OpenRect = (24, 28, 8, 12);
/// A 7 x 7 pocket holds exactly one legal body centre — the smallest open
/// square a 3-cell body fits in, and it fits in one place.
const POCKET_ONE: OpenRect = (28, 35, 6, 13);
/// The single legal body centre of [`POCKET_ONE`].
const POCKET_ONE_CENTRE: [f32; 2] = [31.5, 9.5];

/// An RTS scene whose only open ground is `open`, everything else solid.
fn pocket_scene(open: &[OpenRect]) -> RtsHarness {
    let obstacle_cells = (0..POCKET_GRID * POCKET_GRID)
        .filter(|idx| {
            let (x, y) = (idx % POCKET_GRID, idx / POCKET_GRID);
            !open
                .iter()
                .any(|&(x0, x1, y0, y1)| x >= x0 && x < x1 && y >= y0 && y < y1)
        })
        .collect();
    let mut spec = collision_spec(vec![Cell { x: 30, y: 9 }], obstacle_cells);
    spec.width = POCKET_GRID;
    spec.height = POCKET_GRID;
    spec.destination = Cell { x: 31, y: 9 };
    spec.rts = Some(RtsSpec {
        start_crystal: 300,
        start_gas: 100,
        start_supply_cap: 10,
        hq_cell: Cell { x: 0, y: 0 },
        // Both nodes sit in the corridor: raw-walkable and reachable, and no
        // body can stand there anyway, so they add and remove no centre.
        crystal_nodes: vec![Cell { x: 25, y: 9 }],
        gas_nodes: vec![Cell { x: 26, y: 10 }],
        enemies: None,
    });
    RtsHarness::spec(spec)
        .build()
        .expect("pocket scene harness")
}

/// Add a worker directly to the store, on top of whatever is already there.
fn add_worker(h: &mut RtsHarness, at: [f32; 2]) -> EntityId {
    h.world_mut()
        .entities_mut()
        .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, at)
        .expect("spawn worker")
}

#[test]
fn failed_fallback_clears_to_hard_and_reports_a_violation() {
    // One legal body centre in the whole grid and two bodies on it. The
    // gradual attempts run out of pocket, and the fallback has nowhere to put
    // either body: whichever one it tries, the only centre is inside the
    // other's body.
    let mut h = pocket_scene(&[POCKET_HQ, POCKET_CORRIDOR, POCKET_ONE]);
    let seeded = workers(&h);
    assert_eq!(seeded.len(), 1);
    assert_eq!(pos(&h, seeded[0]), POCKET_ONE_CENTRE);
    let second = add_worker(&mut h, POCKET_ONE_CENTRE);
    let w = [seeded[0], second];
    gather_one_tick(&mut h, &w);
    exit_gather(&mut h, &w);

    h.step_exact(BOUND as u64);

    assert_eq!(
        pair_state(&h, w[0], w[1]),
        0,
        "a fallback that found nowhere must put the pair back to hard, not \
         park it on a marker it can never leave"
    );
    assert_eq!(
        h.world().last_tick_error(),
        Some(TickError::UnrepairableOverlap),
        "and it must say so"
    );
    assert!(
        h.world().body_overlap_count() >= 1,
        "a hard merged pair is a counted violation: the exit line has to see it"
    );

    // Self-retrying, not stuck: the generic repair takes the pair from here
    // and reports the same failure again for as long as it really is one.
    h.step_exact(1);
    assert_eq!(
        h.world().last_tick_error(),
        Some(TickError::UnrepairableOverlap)
    );
    assert!(h.world().body_overlap_count() >= 1);
}

#[test]
fn fallback_never_crosses_connected_component() {
    // A second pocket, joined by a corridor no body can walk, holds a free
    // legal centre closer to the stuck pair than anything in their own pocket
    // — which has none free at all. Taking it would drop a body somewhere it
    // could never have walked to and can never walk out of.
    const POCKET_TWO: OpenRect = (20, 27, 14, 21);
    const LINK: OpenRect = (22, 26, 13, 14);
    let mut h = pocket_scene(&[POCKET_HQ, POCKET_CORRIDOR, POCKET_ONE, LINK, POCKET_TWO]);
    let seeded = workers(&h);
    let second = add_worker(&mut h, POCKET_ONE_CENTRE);
    let w = [seeded[0], second];
    assert_eq!(pos(&h, w[0]), POCKET_ONE_CENTRE);
    gather_one_tick(&mut h, &w);
    exit_gather(&mut h, &w);

    h.step_exact(BOUND as u64);

    assert_eq!(
        h.world().last_tick_error(),
        Some(TickError::UnrepairableOverlap),
        "the free centre in the far pocket is not a destination"
    );
    for (k, &id) in w.iter().enumerate() {
        let p = pos(&h, id);
        assert!(
            p[1] < 13.0,
            "worker {k} left its own connected region for the far pocket: {p:?}"
        );
    }
    assert_eq!(pair_state(&h, w[0], w[1]), 0);
}

#[test]
fn fallback_tries_the_partner_when_the_first_body_has_nowhere_to_go() {
    // Both bodies are stuck in the body-impassable link between two pockets,
    // so neither can separate. They anchor to *different* pockets — each to
    // the nearer one — and the leading body's pocket is already occupied. The
    // relocation is only recoverable if the partner is tried too.
    const POCKET_TWO: OpenRect = (20, 27, 20, 27);
    const POCKET_TWO_CENTRE: [f32; 2] = [23.5, 23.5];
    const LINK: OpenRect = (22, 26, 13, 20);
    let mut h = pocket_scene(&[POCKET_HQ, POCKET_CORRIDOR, POCKET_ONE, LINK, POCKET_TWO]);
    let seeded = workers(&h);
    let near_two = add_worker(&mut h, [23.5, 17.0]);
    let squatter = add_worker(&mut h, POCKET_TWO_CENTRE);
    // The seeded worker moves into the link beside `near_two`; it is nearer
    // the first pocket, which is empty, so only it can be relocated.
    let near_one = seeded[0];
    park(&mut h, &[near_one], &[[23.5, 16.0]]);
    let w = [near_one, near_two, squatter];
    // `near_two` is the higher slot, so it takes the first proposal only on
    // the ticks the rotation starts at its own rank. The bound is a whole
    // number of ticks, so lining the *first* attempt up lines the last one up
    // too.
    gather_until_rotation(&mut h, &w[..2], w.len() as u64, 1);
    exit_gather(&mut h, &w[..2]);
    let before = pos(&h, near_two);

    h.step_exact(BOUND as u64);

    assert_eq!(
        pos(&h, near_two),
        before,
        "the first-priority body has no free centre in its own region, so it \
         must not have moved"
    );
    assert_eq!(
        pos(&h, near_one),
        POCKET_ONE_CENTRE,
        "the partner does, and the fallback has to try it"
    );
    assert_eq!(h.world().last_tick_error(), None);
    assert_eq!(pair_state(&h, near_one, near_two), 0);
}

#[test]
fn fallback_preserves_active_provenance_of_a_third_pair() {
    // The relocated body is still gathering with a third worker. That pair's
    // exemption is this tick's provenance, which every active pair is required
    // to carry: clearing it along with the transitions would take a mandatory
    // byte out of the state hash and revoke a collision the pair is still
    // entitled to.
    let (mut h, w) = blocked_exit_scene();
    let third = add_worker(&mut h, [20.5, 26.5]);
    // The pair's own provenance first, then the exit — and `w[0]` keeps
    // gathering, with the third worker, right through its own relocation.
    gather_one_tick(&mut h, &[w[0], w[1]]);
    assert!(h.world_mut().force_order_for_test(w[1], Order::Idle));
    gathering(&mut h, third, GatherPhase::Mining { ticks_left: 10_000 });

    h.step_exact(BOUND as u64);

    assert_eq!(
        pair_state(&h, w[0], w[1]),
        0,
        "the exiting pair spent its bound and is hard again"
    );
    assert_eq!(
        pair_state(&h, w[0], third),
        GATHER_PAIR_ACTIVE,
        "the relocated worker is still gathering with the third one: its \
         provenance must survive the relocation"
    );
    assert_eq!(h.world().body_overlap_count(), 0);
}

// --- several exits at once --------------------------------------------------

#[test]
fn multi_pair_pass_is_snapshot_eligible_and_order_stable() {
    // Three workers, three exiting pairs, one pass. Whether a pair acts at all
    // is read from the picture taken before the pass — `(0, 2)` is a body
    // apart there — even though `(0, 1)`'s own accepted move, committed
    // earlier in the same pass, has already pushed them inside each other by
    // the time `(0, 2)` is reached.
    let mut h = harness(scattered_spawns(3));
    let w = workers(&h);
    park(
        &mut h,
        &w,
        &[[20.5, 20.5], [21.0, 20.5], [20.5 - 6.4, 20.5]],
    );
    // `w[0]` must take the first proposal on `(0, 1)`, or its move — the one
    // that closes the gap to `w[2]` — never happens.
    gather_until_rotation(&mut h, &w, w.len() as u64, 0);
    assert_eq!(
        h.world().raw_body_overlap_count(),
        1,
        "only (0, 1) may start merged"
    );
    exit_gather(&mut h, &w);

    h.step_exact(1);

    assert_eq!(
        pair_state(&h, w[0], w[1]),
        1,
        "the pair that was merged in the snapshot spent an attempt"
    );
    assert_eq!(
        pair_state(&h, w[0], w[2]),
        0,
        "the pair that was a body apart in the snapshot must be cleared, \
         however the pass's own earlier moves left it"
    );
    assert_eq!(pair_state(&h, w[1], w[2]), 0);

    // One pass, not a fixpoint: a body is proposed once per pair it is in.
    let hashes: Vec<String> = (0..2)
        .map(|_| {
            let mut h = harness(scattered_spawns(3));
            let w = workers(&h);
            park(
                &mut h,
                &w,
                &[[20.5, 20.5], [21.0, 20.5], [20.5 - 6.4, 20.5]],
            );
            gather_one_tick(&mut h, &w);
            exit_gather(&mut h, &w);
            h.step_exact(40);
            h.state_hash_hex()
        })
        .collect();
    assert_eq!(
        hashes[0], hashes[1],
        "the pass must be a pure function of the world it walks"
    );
}

#[test]
fn multi_pair_move_never_decreases_other_transition_distance() {
    // `w[1]` is exiting both neighbours at once and its two escapes point at
    // each other. Backing away from `w[2]` would undo the progress `(0, 1)`
    // just made, so the candidate is refused and `w[2]` moves instead.
    let mut h = harness(scattered_spawns(3));
    let w = workers(&h);
    park(&mut h, &w, &[[20.5, 20.5], [21.0, 20.5], [26.5, 20.5]]);
    // Rank order `w[0] < w[1] < w[2]`, so `w[0]` moves for `(0, 1)` and `w[1]`
    // gets the refused first proposal on `(1, 2)`.
    gather_until_rotation(&mut h, &w, w.len() as u64, 0);
    exit_gather(&mut h, &w);
    let before = pos(&h, w[1]);

    h.step_exact(1);

    assert_eq!(
        pos(&h, w[1]),
        before,
        "the middle body's only candidate closes up a pair it is already \
         separating from, so it must stand still"
    );
    assert_eq!(
        pos(&h, w[0]),
        [20.0, 20.5],
        "the first pair still separated"
    );
    assert_eq!(
        pos(&h, w[2]),
        [27.0, 20.5],
        "and the partner moved instead, ending that exit at contact"
    );
    assert_eq!(pair_state(&h, w[1], w[2]), 0);
    assert_eq!(h.world().body_overlap_count(), 0);
}

#[test]
fn forced_non_gather_overlap_repairs_immediately() {
    // Nothing about the transition softens a plain forced overlap: a pair that
    // never gathered has no provenance to spend, so it is repaired on the
    // first tick rather than given a bound.
    let mut h = harness(scattered_spawns(2));
    let w = workers(&h);
    park_merged(&mut h, &w);
    assert_eq!(pair_state(&h, w[0], w[1]), 0);

    h.step_exact(1);

    assert_eq!(
        pair_state(&h, w[0], w[1]),
        0,
        "no bound was ever handed out"
    );
    assert_no_overlap(&h, "a pair that never gathered is repaired outright");
    assert_eq!(h.world().raw_body_overlap_count(), 0);
    assert_eq!(h.world().body_overlap_count(), 0);
}

// --- determinism ------------------------------------------------------------

const EXIT_CHILD_ENV: &str = "MMD_RTS_GATHER_EXIT_CHILD_HASH";
const EXIT_CHILD_TEST: &str = "print_gather_exit_hash_for_child_process";
const EXIT_HASH_PREFIX: &str = "MMD_RTS_GATHER_EXIT_HASH=";

/// Four workers merged into one heap, given gather provenance and then cut
/// loose: every branch of the transition — gradual attempts, refused
/// candidates, the concentric normal and the fallback — runs inside this.
fn gather_exit_hash() -> String {
    let mut h = harness(scattered_spawns(4));
    let w = workers(&h);
    park(
        &mut h,
        &w,
        &[[20.5, 20.5], [20.5, 20.5], [21.0, 20.5], [20.5, 21.0]],
    );
    gather_one_tick(&mut h, &w);
    exit_gather(&mut h, &w);
    h.step_exact(60);
    h.state_hash_hex()
}

/// Child entry point for `gather_exit_state_reproduces_cross_process`.
#[test]
#[ignore = "child entry point driven by gather_exit_state_reproduces_cross_process"]
fn print_gather_exit_hash_for_child_process() {
    assert!(
        std::env::var_os(EXIT_CHILD_ENV).is_some(),
        "child entry point must only run via the parent, which sets {EXIT_CHILD_ENV}"
    );
    println!("{EXIT_HASH_PREFIX}{}", gather_exit_hash());
}

#[test]
fn gather_exit_state_reproduces_cross_process() {
    // The pair table is hashed state, and the transition writes to it every
    // tick. A replay that spent its attempts in a different order, or kept one
    // byte a tick longer, diverges here.
    let first = gather_exit_hash();
    assert_eq!(
        first,
        gather_exit_hash(),
        "a bounded exit must reproduce its state hash in-process"
    );
    assert_eq!(
        first,
        hash_from_child_process(EXIT_CHILD_ENV, EXIT_CHILD_TEST, EXIT_HASH_PREFIX),
        "a bounded exit must reproduce its state hash across processes"
    );
}

// --- T8: the builder-trap fix does not widen the overlap policy ----------------

/// T8 changed how a *stalled* site resolves, and nothing else: it added no
/// collision exemption, no phasing, no relaxed body placement. So the repro
/// scenario that fix was written against must still end every single tick with
/// ADR 021's policy claim intact — `body_overlap_count() == 0` — including the
/// tick a 16-cell Barracks becomes solid on top of five bodies at once.
#[test]
fn the_builder_fix_does_not_widen_the_overlap_policy() {
    // Same plot, builder and boxing ring as
    // `rts_build::a_builder_is_never_trapped_by_the_building_it_finished`.
    const BARRACKS_MIN: Cell = Cell { x: 136, y: 152 };
    const BARRACKS_CENTER: [f32; 2] = [144.0, 160.0];

    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let builder = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Unit(UnitKind::Worker),
            OWNER_PLAYER,
            BARRACKS_CENTER,
        )
        .expect("spawn the builder");
    let ring = 2.0 * RADIUS + 0.5;
    for [dx, dy] in [[ring, 0.0], [-ring, 0.0], [0.0, ring], [0.0, -ring]] {
        h.world_mut()
            .entities_mut()
            .spawn(
                EntityKind::Unit(UnitKind::Worker),
                OWNER_PLAYER,
                [BARRACKS_CENTER[0] + dx, BARRACKS_CENTER[1] + dy],
            )
            .expect("spawn a boxing worker");
    }
    assert!(h.world_mut().begin_placement(BuildingKind::Barracks));
    let site = h
        .world_mut()
        .confirm_placement(BARRACKS_MIN, builder)
        .expect("Barracks placement refused");

    for tick in 1..=600u64 {
        h.step_exact(1);
        assert_eq!(
            h.world().body_overlap_count(),
            0,
            "tick {tick} ended with a merged pair"
        );
    }
    // A site that cancelled itself never stamped anything, so the tick this
    // case exists to sample would never have happened.
    let site_slot = h
        .world()
        .entities()
        .slot(site)
        .expect("the Barracks cancelled itself instead of finishing");
    assert_eq!(
        h.world().entities().progress_target(site_slot),
        0,
        "the Barracks never finished, so the completion tick was never sampled"
    );

    // The bodies are legal geometry too, not merely un-merged by policy.
    let store = h.world().entities();
    let bodies: Vec<(usize, [f32; 2])> = (0..store.slot_count())
        .filter(|&s| store.alive(s) && matches!(store.kind(s), EntityKind::Unit(_)))
        .map(|s| (s, store.position(s)))
        .collect();
    for &(slot, p) in &bodies {
        assert!(
            h.world().static_nav().position_clear(p, RADIUS),
            "slot {slot} stands at {p:?}, inside static geometry"
        );
    }
    for (i, &(sa, a)) in bodies.iter().enumerate() {
        for &(sb, b) in &bodies[i + 1..] {
            assert!(
                !units_overlap(a, RADIUS, b, RADIUS),
                "slots {sa} and {sb} are merged at {a:?} and {b:?}"
            );
        }
    }
}
