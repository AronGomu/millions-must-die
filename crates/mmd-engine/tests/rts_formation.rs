//! T5 — deterministic group formations and fair chokes.
//!
//! One group order acquires **one** pooled anchor field and hands every member
//! a distinct lattice slot around that anchor, planned atomically: either every
//! member gets a legal slot or nothing is ordered at all. Movement descends the
//! shared field until it is inside the terminal capture ring, then steers
//! straight at its own slot — a local placement step, never a second field.
//!
//! Pure logic: no GPU and no clock — every case here is a headless CPU check of
//! `mmd_engine::rts` through `testkit::RtsHarness`.

use std::collections::HashSet;

use mmd_engine::rts::{
    EntityId, EntityKind, FORMATION_ARRIVAL_CELLS, FORMATION_SPACING_CELLS, FormationError,
    FormationGoal, OWNER_PLAYER, Order, RTS_UNIT_BODY_RADIUS_CELLS, UnitKind,
};
use mmd_engine::scenario::{Cell, RtsSpec, ScenarioSpec};
use mmd_engine::testkit::RtsHarness;

/// A small, validly-shaped RTS scenario with `obstacle_cells` as its terrain.
/// The HQ and both node kinds sit in a far corner, clear of any geometry a
/// case places.
fn scene(width: u32, height: u32, obstacle_cells: Vec<u32>) -> ScenarioSpec {
    ScenarioSpec {
        version: "rts_prototype_v1".to_string(),
        width,
        height,
        cell_size_px: 4,
        sprite_size_px: 48,
        hard_agent_count: 0,
        stretch_agent_count: 0,
        seed: 1,
        destination: Cell { x: 0, y: 0 },
        spawn_cells: vec![Cell { x: 0, y: 0 }],
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
            start_supply_cap: 200,
            hq_cell: Cell {
                x: width - 13,
                y: height - 13,
            },
            crystal_nodes: vec![Cell {
                x: 1,
                y: height - 2,
            }],
            gas_nodes: vec![Cell {
                x: 1,
                y: height - 3,
            }],
        }),
    }
}

fn harness(width: u32, height: u32, obstacles: Vec<u32>) -> RtsHarness {
    RtsHarness::spec(scene(width, height, obstacles))
        .build()
        .expect("rts harness")
}

fn flat(x: u32, y: u32, width: u32) -> u32 {
    x + y * width
}

/// `count` player workers in a row starting at `origin`, one body diameter
/// plus a cell apart so nothing starts merged.
fn spawn_row(h: &mut RtsHarness, count: usize, origin: [f32; 2], per_row: usize) -> Vec<EntityId> {
    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        let col = (i % per_row) as f32;
        let row = (i / per_row) as f32;
        let pos = [origin[0] + col * 7.0, origin[1] + row * 7.0];
        ids.push(
            h.world_mut()
                .entities_mut()
                .spawn(EntityKind::Unit(UnitKind::Worker), OWNER_PLAYER, pos)
                .expect("spawn worker"),
        );
    }
    ids
}

fn goal_of(h: &RtsHarness, id: EntityId) -> FormationGoal {
    match h.world().order_of(id) {
        Some(Order::Move { goal, .. }) => goal,
        other => panic!("{id:?} is not moving: {other:?}"),
    }
}

fn position_of(h: &RtsHarness, id: EntityId) -> [f32; 2] {
    let slot = h.world().entities().slot(id).expect("live entity");
    h.world().entities().position(slot)
}

fn center(c: Cell) -> [f32; 2] {
    [c.x as f32 + 0.5, c.y as f32 + 0.5]
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    (dx * dx + dy * dy).sqrt()
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

/// The plan is a function of the *set* of live orderable units, never of the
/// order the caller happened to list them in.
#[test]
fn group_input_order_does_not_change_slots() {
    let dest = Cell { x: 60, y: 60 };

    let mut ascending = harness(120, 120, vec![]);
    let ids = spawn_row(&mut ascending, 6, [10.5, 10.5], 3);
    assert_eq!(
        ascending.world_mut().order_move_group(&ids, dest),
        Ok(ids.len())
    );

    let mut permuted = harness(120, 120, vec![]);
    let same_ids = spawn_row(&mut permuted, 6, [10.5, 10.5], 3);
    assert_eq!(same_ids, ids, "both worlds must issue the same entity ids");
    let mut shuffled = ids.clone();
    shuffled.reverse();
    shuffled.swap(1, 4);
    assert_eq!(
        permuted.world_mut().order_move_group(&shuffled, dest),
        Ok(ids.len())
    );

    for id in &ids {
        assert_eq!(
            goal_of(&ascending, *id),
            goal_of(&permuted, *id),
            "{id:?} was given a different slot by a permuted input"
        );
    }
    assert_eq!(
        ascending.state_hash(),
        permuted.state_hash(),
        "a permuted input must not change world state"
    );
}

/// One group command, one `FieldPool::acquire` — not one per member.
#[test]
fn group_move_acquires_one_anchor_field() {
    let mut h = harness(120, 120, vec![]);
    let ids = spawn_row(&mut h, 24, [10.5, 10.5], 6);
    let before = h.world().nav().acquire_count();

    assert_eq!(
        h.world_mut().order_move_group(&ids, Cell { x: 60, y: 60 }),
        Ok(24)
    );

    assert_eq!(
        h.world().nav().acquire_count(),
        before + 1,
        "a group command must acquire exactly one anchor field"
    );
    let slots: HashSet<u8> = ids
        .iter()
        .map(|id| match h.world().order_of(*id) {
            Some(Order::Move { field, .. }) => field.slot,
            other => panic!("{id:?} is not moving: {other:?}"),
        })
        .collect();
    assert_eq!(slots.len(), 1, "one anchor must mean one shared field");
}

/// Every planned slot is distinct and at least one body diameter from every
/// other, so a group that reaches its goals is a group that cannot merge.
#[test]
fn formation_slots_are_six_cells_apart() {
    let mut h = harness(120, 120, vec![]);
    let ids = spawn_row(&mut h, 9, [10.5, 10.5], 3);
    assert_eq!(
        h.world_mut().order_move_group(&ids, Cell { x: 60, y: 60 }),
        Ok(9)
    );

    let slots: Vec<Cell> = ids.iter().map(|id| goal_of(&h, *id).slot).collect();
    let unique: HashSet<(u32, u32)> = slots.iter().map(|c| (c.x, c.y)).collect();
    assert_eq!(
        unique.len(),
        slots.len(),
        "two units share a slot: {slots:?}"
    );

    for (i, a) in slots.iter().enumerate() {
        for b in &slots[i + 1..] {
            let d = dist(center(*a), center(*b));
            assert!(
                d >= FORMATION_SPACING_CELLS as f32,
                "slots {a:?} and {b:?} are {d} cells apart, closer than the \
                 {FORMATION_SPACING_CELLS}-cell lattice spacing"
            );
        }
    }
}

/// Not enough legal slots for the whole group is a whole-group refusal: no
/// member is left with half a formation order.
#[test]
fn formation_order_is_atomic_when_space_missing() {
    // A 7x7 open pocket walled in on every side. A 3-cell body fits its
    // centre cell and nothing else, so a two-unit group cannot be placed.
    const W: u32 = 120;
    let mut obstacles = Vec::new();
    for y in 39..=47u32 {
        for x in 39..=47u32 {
            let edge = x == 39 || x == 47 || y == 39 || y == 47;
            if edge {
                obstacles.push(flat(x, y, W));
            }
        }
    }
    let mut h = harness(W, W, obstacles);
    let ids = spawn_row(&mut h, 2, [10.5, 10.5], 2);
    let before = h.state_hash();

    assert_eq!(
        h.world_mut().order_move_group(&ids, Cell { x: 43, y: 43 }),
        Err(FormationError::NoFormationSpace)
    );

    for id in &ids {
        assert_eq!(
            h.world().order_of(*id),
            Some(Order::Idle),
            "{id:?} was mutated by a refused group order"
        );
    }
    assert_eq!(
        h.state_hash(),
        before,
        "a refused formation order must mutate nothing at all"
    );
}

// ---------------------------------------------------------------------------
// Terminal steering
// ---------------------------------------------------------------------------

/// Terminal slot steering is a local placement step: inside the capture ring a
/// unit steers straight at its own slot and the pool is never touched again.
#[test]
fn terminal_steering_uses_no_new_field() {
    let mut h = harness(120, 120, vec![]);
    let ids = spawn_row(&mut h, 6, [50.5, 50.5], 3);
    assert_eq!(
        h.world_mut().order_move_group(&ids, Cell { x: 60, y: 60 }),
        Ok(6)
    );
    // Every unit starts inside the capture ring of an anchor ten cells away,
    // so the whole run below is terminal steering.
    let acquires = h.world().nav().acquire_count();

    h.step_exact(400);

    assert_eq!(
        h.world().nav().acquire_count(),
        acquires,
        "terminal steering acquired a field"
    );
}

/// A group that is given slots finishes on them: every member reaches its own
/// slot centre and clears its order there, instead of merging at one point.
///
/// Arrival is captured on the tick the order clears rather than at the end of
/// the run: a parked unit is still a body, and a neighbour settling in beside
/// it may legally shove it a little way off afterwards (T4's bounded push) —
/// including later in the very tick it stopped on, which is why the tolerance
/// here is one body radius rather than the bare arrival radius that
/// `a_lone_unit_stops_on_its_slot` pins.
#[test]
fn a_group_spreads_into_distinct_final_positions() {
    let mut h = harness(120, 120, vec![]);
    let ids = spawn_row(&mut h, 6, [10.5, 10.5], 3);
    assert_eq!(
        h.world_mut().order_move_group(&ids, Cell { x: 60, y: 60 }),
        Ok(6)
    );
    let goals: Vec<FormationGoal> = ids.iter().map(|id| goal_of(&h, *id)).collect();

    let mut arrivals: Vec<Option<[f32; 2]>> = vec![None; ids.len()];
    for _ in 0..2_000 {
        h.step_exact(1);
        for (i, id) in ids.iter().enumerate() {
            if arrivals[i].is_none() && h.world().order_of(*id) == Some(Order::Idle) {
                arrivals[i] = Some(position_of(&h, *id));
            }
        }
        if arrivals.iter().all(Option::is_some) {
            break;
        }
    }

    for ((id, goal), arrival) in ids.iter().zip(&goals).zip(&arrivals) {
        let p = arrival.unwrap_or_else(|| panic!("{id:?} never finished its move order"));
        assert!(
            dist(p, center(goal.slot)) <= RTS_UNIT_BODY_RADIUS_CELLS,
            "{id:?} cleared its order at {p:?}, nowhere near its slot {:?}",
            goal.slot
        );
    }

    // ...and the finished formation is a formation: no two bodies merged, and
    // nobody piled onto one point.
    let finals: Vec<[f32; 2]> = ids.iter().map(|id| position_of(&h, *id)).collect();
    for (i, a) in finals.iter().enumerate() {
        for b in &finals[i + 1..] {
            assert!(
                dist(*a, *b) >= FORMATION_SPACING_CELLS as f32,
                "{a:?} and {b:?} ended closer than one body diameter"
            );
        }
    }
}

/// The arrival rule itself, with no neighbour to blur it: one unit, one slot,
/// stopped on the slot's own centre.
#[test]
fn a_lone_unit_stops_on_its_slot() {
    let mut h = harness(120, 120, vec![]);
    let ids = spawn_row(&mut h, 1, [10.5, 10.5], 1);
    let dest = Cell { x: 60, y: 60 };
    assert_eq!(h.world_mut().order_move_group(&ids, dest), Ok(1));
    let goal = goal_of(&h, ids[0]);
    assert_eq!(
        goal,
        FormationGoal {
            anchor: dest,
            slot: dest
        },
        "a group of one forms up on the anchor itself"
    );

    h.step_exact(2_000);

    let p = position_of(&h, ids[0]);
    assert!(
        dist(p, center(goal.slot)) <= FORMATION_ARRIVAL_CELLS,
        "the unit stopped at {p:?}, not within {FORMATION_ARRIVAL_CELLS} of its slot"
    );
    assert_eq!(h.world().order_of(ids[0]), Some(Order::Idle));
}

// ---------------------------------------------------------------------------
// Fair chokes
// ---------------------------------------------------------------------------

/// The rotated traversal from T4 is the whole fairness rule: two units queued
/// at a gap only one body wide both get through, and the run is reproducible.
#[test]
fn choke_priority_rotates() {
    const W: u32 = 120;
    // A wall across the map at y = 60 with a 7-cell gap at x in 57..=63 —
    // exactly one body-clear column.
    let mut obstacles = Vec::new();
    for x in 0..W {
        if (57..=63).contains(&x) {
            continue;
        }
        obstacles.push(flat(x, 60, W));
    }

    let run = |seed_ticks: u64| -> ([u8; 32], Vec<[f32; 2]>) {
        let mut h = harness(W, W, obstacles.clone());
        let ids = spawn_row(&mut h, 2, [57.5, 40.5], 1);
        assert_eq!(
            h.world_mut().order_move_group(&ids, Cell { x: 60, y: 80 }),
            Ok(2)
        );
        h.step_exact(seed_ticks);
        (
            h.state_hash(),
            ids.iter().map(|id| position_of(&h, *id)).collect(),
        )
    };

    let (hash_a, positions) = run(4_000);
    let (hash_b, positions_b) = run(4_000);
    assert_eq!(hash_a, hash_b, "the choke run is not reproducible");
    assert_eq!(positions, positions_b);

    for (i, p) in positions.iter().enumerate() {
        assert!(
            p[1] > 61.0,
            "unit {i} is at {p:?}, still on the near side of the choke — one \
             unit starved the other"
        );
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// The slot is hashed state, not a derived convenience.
///
/// Two worlds, identical down to the last body position, whose two units hold
/// the same anchor but different slots — one pair ordered as a group, the
/// other one unit at a time — must not hash the same.
#[test]
fn state_hash_sees_the_formation_slot() {
    let dest = Cell { x: 60, y: 60 };

    let mut grouped = harness(120, 120, vec![]);
    let ids = spawn_row(&mut grouped, 2, [10.5, 10.5], 2);
    assert_eq!(grouped.world_mut().order_move_group(&ids, dest), Ok(2));

    let mut singly = harness(120, 120, vec![]);
    let same_ids = spawn_row(&mut singly, 2, [10.5, 10.5], 2);
    assert_eq!(same_ids, ids);
    for id in &ids {
        assert_eq!(singly.world_mut().order_move_group(&[*id], dest), Ok(1));
    }

    // Same anchor either way; only the slots differ.
    for id in &ids {
        assert_eq!(goal_of(&grouped, *id).anchor, goal_of(&singly, *id).anchor);
    }
    assert_ne!(
        goal_of(&grouped, ids[0]).slot,
        goal_of(&singly, ids[0]).slot,
        "the two plans must actually disagree, or this proves nothing"
    );
    assert_ne!(
        grouped.state_hash(),
        singly.state_hash(),
        "a world where a unit holds a different slot must hash differently"
    );
}
