//! T7 — in-place flow-field rebuilds and the LRU field pool.
//!
//! Pure CPU, no GPU, no clock. The pool is the engine's answer to "a player
//! clicks anywhere": a small fixed set of flow fields keyed by destination,
//! never a per-unit path.

use mmd_engine::nav::field_pool::{FieldPool, FieldPoolError, NAV_FIELD_SLOTS};
use mmd_engine::nav::flow_field::{COST_UNREACHABLE, FieldScratch, FlowField, FlowFieldError};
use mmd_engine::scenario::Cell;
use mmd_engine::testkit::SplitMix64;

/// Open square grid of `n` cells a side.
fn open_pool(n: u32) -> FieldPool {
    FieldPool::new(n, n, &[]).expect("open grid pool")
}

/// Eight distinct destinations on a grid `n` a side, in a stable order.
fn dests(n: u32, count: u32) -> Vec<Cell> {
    (0..count)
        .map(|i| Cell {
            x: (i * 3) % n,
            y: (i * 7) % n,
        })
        .collect()
}

// --- FlowField::rebuild_in_place ------------------------------------------

#[test]
fn rebuild_in_place_matches_build() {
    let w = 40u32;
    let h = 40u32;
    let n = (w * h) as usize;
    let mut rng = SplitMix64::new(0x7715).derive("obstacles");
    let mut field = FlowField::blank(w, h).expect("blank field");
    let mut scratch = FieldScratch::with_capacity(n);

    for _ in 0..30 {
        // A seeded obstacle set, ~15% of the grid, never on the border row 0.
        let mut obstacles: Vec<u32> = Vec::new();
        let mut blocked = vec![false; n];
        for _ in 0..(n / 7) {
            let idx = rng.next_bounded(n as u64) as u32;
            if !blocked[idx as usize] {
                blocked[idx as usize] = true;
                obstacles.push(idx);
            }
        }
        obstacles.sort_unstable();

        for k in 0..5u32 {
            let dest = Cell {
                x: (rng.next_bounded(w as u64)) as u32,
                y: (rng.next_bounded(h as u64)) as u32,
            };
            let built = FlowField::build(w, h, dest, &obstacles);
            let rebuilt = field.rebuild_in_place(dest, &blocked, &mut scratch);

            match (built, rebuilt) {
                (Ok(reference), Ok(())) => {
                    assert_eq!(
                        field.costs(),
                        reference.costs(),
                        "costs diverged for dest {dest:?} (set {k})"
                    );
                    for y in 0..h {
                        for x in 0..w {
                            assert_eq!(
                                field.vector_at(x, y),
                                reference.vector_at(x, y),
                                "vector diverged at ({x}, {y}) for dest {dest:?}"
                            );
                        }
                    }
                }
                (Err(a), Err(b)) => {
                    // A blocked destination is rejected by both paths.
                    assert_eq!(a, b, "the two paths disagreed on the rejection");
                }
                (a, b) => panic!("one path accepted what the other refused: {a:?} vs {b:?}"),
            }
        }
    }
}

#[test]
fn rebuild_in_place_rejects_a_wrong_mask() {
    let mut field = FlowField::blank(8, 8).expect("blank field");
    let mut scratch = FieldScratch::with_capacity(64);
    // Warm it with a real rebuild so "untouched" means something.
    field
        .rebuild_in_place(Cell { x: 7, y: 7 }, &[false; 64], &mut scratch)
        .expect("first rebuild");
    let before: Vec<u32> = field.costs().to_vec();

    let short = vec![false; 63];
    let err = field
        .rebuild_in_place(Cell { x: 0, y: 0 }, &short, &mut scratch)
        .expect_err("a short mask must be refused");
    assert_eq!(
        err,
        FlowFieldError::MaskLength {
            got: 63,
            expected: 64
        }
    );
    assert_eq!(field.costs(), before.as_slice(), "a refused rebuild wrote");
}

#[test]
fn blank_field_moves_nobody() {
    let field = FlowField::blank(8, 8).expect("blank field");
    for y in 0..8 {
        for x in 0..8 {
            assert_eq!(field.vector_at(x, y), (0.0, 0.0));
            assert_eq!(field.cost_at(x, y), COST_UNREACHABLE);
        }
    }
}

// --- FieldPool ---------------------------------------------------------------

#[test]
fn pool_hit_does_not_rebuild() {
    let mut pool = open_pool(64);
    let dest = Cell { x: 5, y: 5 };
    let a = pool.acquire(dest).expect("first acquire");
    let b = pool.acquire(dest).expect("second acquire");
    assert_eq!(a, b, "the same destination must reuse its slot");
    assert_eq!(pool.rebuild_count(), 1, "a hit must not rebuild");
    assert_eq!(pool.key(a), Some(dest));
}

#[test]
fn pool_holds_eight_distinct_destinations() {
    let mut pool = open_pool(64);
    let mut slots = Vec::new();
    for d in dests(64, NAV_FIELD_SLOTS as u32) {
        slots.push(pool.acquire(d).expect("acquire"));
    }
    slots.sort_unstable();
    slots.dedup();
    assert_eq!(slots.len(), NAV_FIELD_SLOTS, "eight distinct slots");
    assert_eq!(pool.rebuild_count(), NAV_FIELD_SLOTS as u64);
}

#[test]
fn pool_evicts_the_least_recently_used() {
    let mut pool = open_pool(64);
    let keys = dests(64, NAV_FIELD_SLOTS as u32 + 1);
    let mut slots = Vec::new();
    for d in &keys[..NAV_FIELD_SLOTS] {
        slots.push(pool.acquire(*d).expect("acquire"));
    }
    // Touch A: B is now the least recently used.
    let a_slot = pool.acquire(keys[0]).expect("re-acquire a");
    assert_eq!(a_slot, slots[0]);

    let i_slot = pool.acquire(keys[NAV_FIELD_SLOTS]).expect("acquire i");
    assert_eq!(i_slot, slots[1], "the new key must land in B's slot");
    assert_eq!(pool.key(slots[0]), Some(keys[0]), "A must survive");
    assert_eq!(pool.key(i_slot), Some(keys[NAV_FIELD_SLOTS]));
}

#[test]
fn eviction_ties_prefer_the_lowest_slot() {
    let mut pool = open_pool(64);
    let slot = pool.acquire(Cell { x: 9, y: 9 }).expect("acquire");
    assert_eq!(slot, 0, "an all-empty pool must fill slot 0 first");
}

#[test]
fn pool_rejects_an_out_of_bounds_destination() {
    let mut pool = open_pool(320);
    let err = pool
        .acquire(Cell { x: 400, y: 0 })
        .expect_err("off-grid destination");
    assert!(
        matches!(
            err,
            FieldPoolError::Field(FlowFieldError::DestinationOutOfBounds)
        ),
        "got {err:?}"
    );
    assert_eq!(pool.rebuild_count(), 0);
}

#[test]
fn pool_rejects_a_blocked_destination() {
    let rock = Cell { x: 3, y: 4 };
    let mut pool = FieldPool::new(64, 64, &[rock.x + rock.y * 64]).expect("pool with a rock");
    let err = pool.acquire(rock).expect_err("blocked destination");
    assert!(
        matches!(
            err,
            FieldPoolError::Field(FlowFieldError::DestinationBlocked)
        ),
        "got {err:?}"
    );
    assert_eq!(pool.rebuild_count(), 0);
}

#[test]
fn set_blocked_invalidates_every_slot() {
    let mut pool = open_pool(64);
    let keys = dests(64, NAV_FIELD_SLOTS as u32);
    for d in &keys {
        pool.acquire(*d).expect("acquire");
    }
    let warm = pool.rebuild_count();
    assert_eq!(warm, NAV_FIELD_SLOTS as u64);

    pool.set_blocked(Cell { x: 40, y: 40 }, true);
    assert!(pool.blocked()[(40 + 40 * 64) as usize]);
    for slot in 0..NAV_FIELD_SLOTS as u8 {
        assert_eq!(pool.key(slot), None, "slot {slot} kept a stale key");
    }

    pool.acquire(keys[0]).expect("re-acquire the first key");
    assert_eq!(
        pool.rebuild_count(),
        warm + 1,
        "an invalidated key must rebuild"
    );
}

#[test]
fn set_blocked_changes_the_walk() {
    // 8x8 open grid, then a full wall at x == 4 with one gap at y == 1.
    let mut pool = FieldPool::new(8, 8, &[]).expect("open grid");
    let dest = Cell { x: 7, y: 7 };
    let slot = pool.acquire(dest).expect("open field");
    let open_path = descent_path(pool.field(slot), Cell { x: 0, y: 0 }, dest);
    let gap = Cell { x: 4, y: 1 };
    assert!(
        !open_path.contains(&gap),
        "the open walk must not already run through the gap, or this proves nothing"
    );

    for y in 0..8u32 {
        if y != gap.y {
            pool.set_blocked(Cell { x: 4, y }, true);
        }
    }
    let slot = pool.acquire(dest).expect("walled field");
    let walled_path = descent_path(pool.field(slot), Cell { x: 0, y: 0 }, dest);
    assert!(
        walled_path.contains(&gap),
        "the walk must go through the only gap, got {walled_path:?}"
    );
    assert_eq!(
        walled_path.last(),
        Some(&dest),
        "the walk must still arrive"
    );
}

#[test]
fn a_cached_acquire_allocates_nothing() {
    let mut pool = open_pool(64);
    let keys = dests(64, NAV_FIELD_SLOTS as u32);
    // Warm every slot twice: whatever the scratch heap grows, it grows here.
    for _ in 0..2 {
        for d in &keys {
            pool.acquire(*d).expect("warm acquire");
        }
    }
    let capacity = pool.scratch_capacity();
    let rebuilds = pool.rebuild_count();

    for _ in 0..100 {
        pool.acquire(keys[3]).expect("cached acquire");
    }
    assert_eq!(
        pool.scratch_capacity(),
        capacity,
        "a cached acquire grew the scratch heap"
    );
    assert_eq!(
        pool.rebuild_count(),
        rebuilds,
        "a cached acquire rebuilt a field"
    );
}

#[test]
fn a_miss_may_grow_scratch_only_once() {
    let mut pool = open_pool(64);
    for d in dests(64, NAV_FIELD_SLOTS as u32) {
        pool.acquire(d).expect("warm acquire");
    }
    let warm_capacity = pool.scratch_capacity();

    // Cycle far more destinations than the pool holds: every one is a miss.
    for i in 0..32u32 {
        let d = Cell {
            x: (i * 5 + 1) % 64,
            y: (i * 11 + 2) % 64,
        };
        pool.acquire(d).expect("miss acquire");
    }
    assert_eq!(
        pool.rebuild_count(),
        NAV_FIELD_SLOTS as u64 + 32,
        "every cycled destination must be a miss"
    );
    assert_eq!(
        pool.scratch_capacity(),
        warm_capacity,
        "the scratch heap grew after warmup; a miss may grow it once, not repeatedly"
    );
}

/// Cells visited walking the field's descent vectors from `from` to `dest`.
///
/// Bounded by cell count so a field with no route returns what it reached
/// instead of spinning.
fn descent_path(field: &FlowField, from: Cell, dest: Cell) -> Vec<Cell> {
    let mut path = vec![from];
    let mut cur = from;
    for _ in 0..(field.width() * field.height()) {
        if cur == dest {
            break;
        }
        let (vx, vy) = field.vector_at(cur.x, cur.y);
        if vx == 0.0 && vy == 0.0 {
            break;
        }
        let nx = cur.x as i32 + vx.round() as i32;
        let ny = cur.y as i32 + vy.round() as i32;
        cur = Cell {
            x: nx as u32,
            y: ny as u32,
        };
        path.push(cur);
    }
    path
}
