//! Zero-allocation-per-frame contract: counting allocator + measure guard.
//!
//! **Code-health invariant, not a performance gate** (T12, reframed by T28).
//! It catches accidental per-frame heap allocation; it says nothing about
//! throughput and consumes no timing threshold. Runs on every merge under a
//! short deterministic policy — see `docs/05-testing.md`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use mmd_engine::alloc_guard::{
    CountingAllocator, MeasureGuard, alloc_count, is_counting, reset_count,
};
use mmd_engine::render::Camera;
use mmd_engine::rts::{
    BuildingKind, DragBox, EntityKind, GatherPhase, OWNER_PLAYER, Order, ResourceKind, RtsFrame,
    UnitKind, pack_frame, pack_hud,
};
use mmd_engine::runtime::InputAction;
use mmd_engine::scenario::Cell;
use mmd_engine::sim::SpatialGrid;
use mmd_engine::testkit::{
    COLLISION_SPRITE_SCENE, FIXTURE_DENSE_V1, GridSpec, Harness, RtsHarness, ScenarioSource,
    scene_path,
};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// The allocation counter is process-wide — serialize these tests so two of
/// them never share one measure scope.
///
/// The mutex is necessary but **not sufficient**: it orders the tests in this
/// binary, and it cannot stop libtest's own harness thread (or a sibling test
/// thread starting up / tearing down) from allocating while a guard is open.
/// That residual cross-talk is what
/// `foreign_thread_allocations_do_not_leak_into_a_measure_scope` pins down, and
/// why `MeasureGuard` counts the entering thread only.
fn lock_alloc_tests() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Injected per-frame allocation must still hard-fail the invariant.
#[test]
fn alloc_invariant_still_enforced() {
    let _lock = lock_alloc_tests();
    reset_count();
    let guard = MeasureGuard::enter();
    // Injected Vec growth must be visible to the counting allocator.
    let mut v = Vec::new();
    v.push(1u8);
    v.push(2u8);
    v.reserve(1024);
    std::hint::black_box(&v);
    let n = guard.allocations();
    assert!(n > 0, "injected Vec growth must count, got {n}");
    assert!(guard.check_zero().is_err());
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        guard.assert_zero();
    }));
    assert!(err.is_err(), "assert_zero must hard-fail on allocs");
}

#[test]
fn warmup_allocation_passes() {
    let _lock = lock_alloc_tests();
    reset_count();
    // Warmup / setup allocs outside the measure guard.
    let mut warm = Vec::with_capacity(4096);
    warm.extend_from_slice(&[1u8, 2, 3, 4]);
    std::hint::black_box(&warm);

    let guard = MeasureGuard::enter();
    // No heap traffic while measuring.
    let x = std::hint::black_box(warm.len() + 7);
    assert_eq!(x, 11);
    assert_eq!(guard.allocations(), 0);
    guard.assert_zero();
}

#[test]
fn guard_resets_between_trials() {
    let _lock = lock_alloc_tests();
    reset_count();

    let n1 = {
        let g = MeasureGuard::enter();
        let forced = vec![0u8; 128];
        std::hint::black_box(forced);
        let n = g.allocations();
        assert!(n >= 1, "trial1 must see alloc");
        n
    };
    assert!(!is_counting());

    let n2 = {
        let g = MeasureGuard::enter();
        // Fresh delta — prior trial must not leak into this count.
        assert_eq!(g.allocations(), 0, "trial2 starts at 0 delta");
        let forced = vec![1u8; 256];
        std::hint::black_box(forced);
        g.allocations()
    };
    assert!(!is_counting());
    assert!(n1 >= 1 && n2 >= 1);
    // Counters are independent deltas (not cumulative across guards).
    let _ = (n1, n2);
}

#[test]
fn panic_restores_guard() {
    let _lock = lock_alloc_tests();
    reset_count();
    assert!(!is_counting());

    let result = std::panic::catch_unwind(|| {
        let _g = MeasureGuard::enter();
        assert!(is_counting());
        panic!("unwind under measure guard");
    });
    assert!(result.is_err());
    assert!(
        !is_counting(),
        "Drop must restore counting flag after panic"
    );

    // Later measure still works and starts clean.
    let before = alloc_count();
    {
        let g = MeasureGuard::enter();
        assert!(is_counting());
        assert_eq!(g.allocations(), 0);
        // no alloc
    }
    assert!(!is_counting());
    assert_eq!(alloc_count(), before);
}

/// A thread the measured code did not start must not be able to add to the
/// count. This is the deterministic form of the flake that used to fail
/// `warmup_allocation_passes` and `panic_restores_guard` under parallel load:
/// libtest's harness thread and the sibling test threads allocate on their own
/// schedule, and while the counter was process-wide those allocations landed
/// inside whichever measure scope happened to be open.
///
/// The mutex above cannot fix that — foreign threads never take it. Only
/// scoping the count to the thread that entered the guard can, so this test
/// fails outright against a process-wide counter rather than 3 runs in 100.
#[test]
fn foreign_thread_allocations_do_not_leak_into_a_measure_scope() {
    let _lock = lock_alloc_tests();
    reset_count();

    let stop = Arc::new(AtomicBool::new(false));
    let started = Arc::new(AtomicBool::new(false));
    let noisy = {
        let stop = Arc::clone(&stop);
        let started = Arc::clone(&started);
        std::thread::spawn(move || {
            // Allocate continuously on a thread that never enters a guard.
            while !stop.load(Ordering::Relaxed) {
                let churn: Vec<u8> = Vec::with_capacity(4096);
                std::hint::black_box(&churn);
                started.store(true, Ordering::Relaxed);
            }
        })
    };
    // Do not measure until the foreign thread is demonstrably running, or the
    // test would pass by racing past a thread that had not allocated yet.
    // Bounded by iterations rather than wall clock: this suite asserts on no
    // timing, and an unbounded spin would hang the gate instead of failing it.
    let mut spins = 0u64;
    while !started.load(Ordering::Relaxed) {
        std::hint::spin_loop();
        spins += 1;
        assert!(
            spins < 5_000_000_000,
            "the noisy thread never allocated; this test cannot prove anything \
             about a scope it never contended with"
        );
    }

    let observed = {
        let guard = MeasureGuard::enter();
        // Hold the scope open long enough for the foreign thread to allocate
        // many times over. No allocation happens on *this* thread.
        for _ in 0..200_000 {
            std::hint::black_box(guard.allocations());
        }
        guard.finish()
    };

    stop.store(true, Ordering::Relaxed);
    noisy.join().expect("noisy thread joins");

    assert_eq!(
        observed, 0,
        "a measure scope counted {observed} allocation(s) made by a thread it \
         never entered; the counter must be scoped to the measuring thread"
    );
}

/// The positive control for the test above, and for the whole worker arm.
///
/// `foreign_thread_allocations_do_not_leak_into_a_measure_scope` pins that an
/// *unarmed* thread is invisible. That is exactly why
/// `a_threaded_collision_tick_allocates_nothing` would pass vacuously if the
/// pool's workers never armed themselves — an invisible thread allocates zero
/// by construction. This pins the other half: a thread that *does* arm is
/// counted into the scope the measuring thread is reading. Without it, the
/// claim that threading made the guard stronger is untested.
#[test]
fn an_armed_thread_allocations_do_reach_a_measure_scope() {
    let _lock = lock_alloc_tests();
    reset_count();

    let go = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let worker = {
        let go = Arc::clone(&go);
        let done = Arc::clone(&done);
        std::thread::spawn(move || {
            while !go.load(Ordering::Acquire) {
                std::hint::spin_loop();
            }
            // Arm exactly the way a separation worker arms around its chunk.
            let _arm = mmd_engine::alloc_guard::arm_worker();
            let churn: Vec<u8> = Vec::with_capacity(4096);
            std::hint::black_box(&churn);
            drop(_arm);
            done.store(true, Ordering::Release);
        })
    };

    let observed = {
        let guard = MeasureGuard::enter();
        go.store(true, Ordering::Release);
        let mut spins = 0u64;
        while !done.load(Ordering::Acquire) {
            std::hint::spin_loop();
            spins += 1;
            assert!(spins < 5_000_000_000, "the armed thread never finished");
        }
        guard.finish()
    };
    worker.join().expect("armed thread joins");

    assert!(
        observed > 0,
        "an armed worker's allocation was invisible to the measuring thread; \
         a zero-allocation assertion would then say nothing about the pool"
    );
}

/// The isometric projection and the cull are free of the heap.
///
/// The projection added arithmetic and a per-instance rect test to the hottest
/// loop the frame has. Neither may cost an allocation — and the cull in
/// particular must not: a packer that collected the surviving instances into a
/// fresh `Vec` before pushing them would still pass every correctness case in
/// `render_correctness.rs` while allocating once per frame per group.
///
/// Measured on the tracked scene precisely *because* the cull bites there: its
/// map diamond is larger than the view, so most agents take the reject path and
/// only some take the push path, and both are inside the guard.
///
/// This lives here, not in `render_correctness.rs`, because the counting
/// allocator is installed in *this* test binary — a `MeasureGuard` anywhere
/// else records nothing and the assertion would pass vacuously.
#[test]
fn iso_packing_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = Harness::builder(ScenarioSource::path(scene_path(COLLISION_SPRITE_SCENE)))
        .agents(2_048)
        .seed(17)
        .build()
        .expect("collision scene");

    // Warm-up outside the scope: whatever the first pack grows, it grows now.
    h.step_exact(3);
    h.runtime_mut().pack_groups();
    let alive = h.alive_count();
    let packed: usize = h
        .runtime()
        .draw_groups()
        .iter()
        .map(|g| g.instances.len())
        .sum();
    assert!(
        packed > 0 && packed < alive,
        "this case must exercise *both* sides of the cull: {packed} of {alive} agents \
         packed"
    );

    let guard = MeasureGuard::enter();
    for _ in 0..8 {
        h.runtime_mut().pack_groups();
        std::hint::black_box(h.runtime().draw_groups()[0].instances.len());
        std::hint::black_box(h.runtime().ring_instances().len());
    }
    assert_eq!(
        guard.allocations(),
        0,
        "packing an isometric frame with the cull allocated"
    );
    guard.assert_zero();
    drop(guard);

    // The pack is still the same frame after eight repeats — a cull that
    // dropped a different set each time would be a different defect.
    let again: usize = h
        .runtime()
        .draw_groups()
        .iter()
        .map(|g| g.instances.len())
        .sum();
    assert_eq!(
        again, packed,
        "re-packing an unchanged sim changed the frame"
    );
}

/// The hitbox overlay does not get to spend the frame budget it was added
/// under: packing a ring per agent reuses a buffer reserved at load.
///
/// Both states are measured. Rings *off* is the cheap half and would pass on
/// its own even if the visible path reallocated every frame, so measuring only
/// one of them would leave the interesting case uncovered.
///
/// This lives here, not in `render_correctness.rs`, because the counting
/// allocator is installed in *this* test binary — a `MeasureGuard` anywhere
/// else records nothing and the assertion would pass vacuously.
#[test]
fn ring_packing_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = Harness::builder(ScenarioSource::path(scene_path(COLLISION_SPRITE_SCENE)))
        .agents(512)
        .build()
        .expect("collision scene");
    assert!(
        h.sim().collision().enabled(),
        "the scene must have a body, or there are no rings to measure"
    );
    assert!(
        h.runtime().hitboxes_visible(),
        "rings are on by default; this test must measure the visible path"
    );

    // Warm-up outside the scope: whatever the first pack grows, it grows now.
    h.step_exact(2);
    h.runtime_mut().pack_groups();
    let packed = h.runtime().ring_instances().len();
    // Not `alive_count()`: this scene's map diamond is larger than the view, so
    // the packer culls. What matters here is that the warm-up really packed
    // rings — an empty warm-up would leave the growth for the measured scope.
    assert!(
        packed > 0 && packed <= h.alive_count(),
        "warm-up packed {packed} rings for {} agents",
        h.alive_count()
    );

    let guard = MeasureGuard::enter();
    for _ in 0..8 {
        h.runtime_mut().pack_groups();
        std::hint::black_box(h.runtime().ring_instances().len());
    }
    assert_eq!(guard.allocations(), 0, "packing rings allocated");
    guard.assert_zero();
    drop(guard);

    // …and with the overlay hidden. `clear()` must keep the capacity, so
    // toggling back on does not re-grow the buffer inside a later frame.
    h.runtime_mut().apply_action(InputAction::ToggleHitboxes);
    assert!(!h.runtime().hitboxes_visible());
    h.runtime_mut().pack_groups();

    let guard = MeasureGuard::enter();
    for _ in 0..8 {
        h.runtime_mut().pack_groups();
        std::hint::black_box(h.runtime().ring_instances().len());
    }
    assert_eq!(
        guard.allocations(),
        0,
        "packing with rings hidden allocated"
    );
    guard.assert_zero();
    drop(guard);

    // Toggling back on inside a measured scope must not allocate either.
    h.runtime_mut().apply_action(InputAction::ToggleHitboxes);
    let guard = MeasureGuard::enter();
    h.runtime_mut().pack_groups();
    std::hint::black_box(h.runtime().ring_instances().len());
    assert_eq!(guard.allocations(), 0, "re-showing the rings allocated");
    guard.assert_zero();
    drop(guard);
    assert_eq!(h.runtime().ring_instances().len(), packed);
}

/// Packing an RTS frame is under the same zero-allocation contract as the
/// horde packer: `RtsFrame::new` reserves every buffer at its ceiling and
/// `clear` keeps the capacity, so no frame may grow one.
///
/// Measured with the ghost *and* a drag live, because those are the two paths
/// that push into the prop group — a measurement of the world layer alone
/// would leave the UI group's reservation untested.
///
/// This lives here, not in `rts_pack.rs`, because the counting allocator is
/// installed in *this* test binary — a `MeasureGuard` anywhere else records
/// nothing and the assertion would pass vacuously.
#[test]
fn pack_frame_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    assert!(h.world_mut().set_rally(hq, Some(Cell { x: 180, y: 176 })));
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    let cursor = [960.0, 540.0];
    let drag = Some(DragBox {
        a: [10.0, 10.0],
        b: [110.0, 60.0],
    });

    // Warm-up outside the scope: whatever the first pack would grow, it grows
    // now. `RtsFrame::new` itself allocates — that is construction, not a
    // frame.
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), cursor, drag, &mut frame);
    let packed = frame.instance_count();
    assert_eq!(
        packed,
        17 + 1 + 1 + 65 + 4,
        "the warm-up must exercise all three layers: world, ring, UI"
    );

    let guard = MeasureGuard::enter();
    for _ in 0..600 {
        pack_frame(h.world(), cursor, drag, &mut frame);
        std::hint::black_box(frame.instance_count());
    }
    assert_eq!(guard.allocations(), 0, "packing an RTS frame allocated");
    guard.assert_zero();
    drop(guard);

    assert_eq!(
        frame.instance_count(),
        packed,
        "re-packing an unchanged world changed the frame"
    );
}

/// The HUD is under the same zero-allocation contract as the rest of the
/// frame: it runs every frame, and `fmt_u32` / `fmt_ratio` write into stack
/// buffers rather than a `String`.
///
/// Measured with a selection, a rally point and a non-empty production queue
/// live — those are the three paths that push extra text beyond the top bar
/// and the always-drawn build menu, so a measurement of the empty-selection
/// case alone would leave them untested.
///
/// This lives here, not in `rts_hud.rs`, because the counting allocator is
/// installed in *this* test binary — a `MeasureGuard` anywhere else records
/// nothing and the assertion would pass vacuously.
#[test]
fn pack_hud_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let hq = h.world().start_hq().expect("hq");
    h.world_mut().selection_mut().insert(hq);
    assert!(h.world_mut().set_rally(hq, Some(Cell { x: 180, y: 176 })));
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    let cursor = [960.0, 540.0];

    // Warm-up outside the scope: whatever the first pack would grow, it grows
    // now.
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), cursor, None, &mut frame);
    pack_hud(h.world(), &mut frame);
    let packed = frame.instance_count();
    assert!(
        packed > 17,
        "the warm-up must have packed more than the bare world"
    );

    let guard = MeasureGuard::enter();
    for _ in 0..600 {
        pack_frame(h.world(), cursor, None, &mut frame);
        pack_hud(h.world(), &mut frame);
        std::hint::black_box(frame.instance_count());
    }
    assert_eq!(guard.allocations(), 0, "packing the HUD allocated");
    guard.assert_zero();
    drop(guard);

    assert_eq!(
        frame.instance_count(),
        packed,
        "re-packing an unchanged world changed the frame"
    );
}

#[test]
fn spatial_rebuild_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let n = 512;
    let mut grid = SpatialGrid::new(64, 64, 1.0, n);
    let xs: Vec<f32> = (0..n).map(|i| (i % 64) as f32 + 0.5).collect();
    let ys: Vec<f32> = (0..n).map(|i| (i / 64) as f32 + 0.5).collect();
    grid.rebuild(&xs, &ys); // warm-up: any lazy growth happens here

    let guard = MeasureGuard::enter();
    for _ in 0..8 {
        grid.rebuild(&xs, &ys);
        std::hint::black_box(grid.len());
    }
    assert_eq!(guard.allocations(), 0);
    guard.assert_zero();
}

/// The whole tick, not just the index: rebuilding the grid and accumulating the
/// repulsion sums must reuse the buffers the sim reserved at construction.
#[test]
fn a_collision_tick_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = Harness::fixture(FIXTURE_DENSE_V1).build().expect("dense");
    assert!(h.sim().collision().enabled(), "fixture must have a body");
    h.step_exact(2); // warm-up outside the measured scope

    let guard = MeasureGuard::enter();
    h.step_exact(10);
    std::hint::black_box(h.tick_index());
    assert_eq!(guard.allocations(), 0);
    guard.assert_zero();
}

/// The pool does not get to weaken the invariant it was allowed to exist under.
///
/// A measure scope is armed per-thread, so an *unarmed* worker's allocations
/// would simply be invisible here and this test would pass without proving
/// anything. Each worker therefore arms itself with `alloc_guard::arm_worker`
/// around its chunk: its allocations land in the same process-wide counter this
/// scope reads, and the zero below covers the workers as well as the ticking
/// thread. That is why introducing threads makes the guard stronger rather than
/// weaker.
#[test]
fn a_threaded_collision_tick_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let spec = GridSpec::new(32, 32, Cell { x: 31, y: 16 })
        .with_spawns(vec![Cell { x: 1, y: 16 }])
        .with_collision(128, 256)
        .with_agents(256)
        .with_separation_threads(4);
    let mut h = Harness::grid(spec)
        .build()
        .expect("threaded collision grid");
    assert!(h.sim().collision().enabled(), "the grid must have a body");
    assert_eq!(
        h.sim().worker_thread_count(),
        3,
        "the pass must actually be running on workers, or this test measures \
         the inline path and proves nothing about them"
    );
    h.step_exact(2); // warm-up outside the measured scope

    let guard = MeasureGuard::enter();
    h.step_exact(10);
    std::hint::black_box(h.tick_index());
    assert_eq!(guard.allocations(), 0);
    guard.assert_zero();
}

/// The RTS movement sweep is under the same invariant the horde tick is.
///
/// The pool is warmed *outside* the scope on purpose: a flow-field **miss**
/// rebuilds into reused scratch and may grow that scratch once, and that miss
/// is the single bounded exception this plan grants. Everything measured below
/// — the live-slot sweep, the field sample, the admissible step, the animation
/// advance — is exempt from nothing.
#[test]
fn movement_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(workers.len(), 6);
    let dest = Cell { x: 200, y: 200 };
    // Warm-up outside the scope: this acquire is the miss that builds the
    // field and settles the scratch heap.
    assert_eq!(h.world_mut().order_move_group(&workers, dest), 6);
    h.step_exact(2);
    assert!(
        workers
            .iter()
            .all(|id| matches!(h.world().order_of(*id), Some(Order::Move { .. }))),
        "every worker must still be walking, or this measures an idle sweep"
    );

    let guard = MeasureGuard::enter();
    h.step_exact(600);
    std::hint::black_box(h.tick_index());
    assert_eq!(guard.allocations(), 0, "the RTS movement sweep allocated");
    guard.assert_zero();
    drop(guard);

    // …and the run really did walk: a stalled sweep allocates nothing either.
    let slot = h.world().entities().slot(workers[0]).expect("worker slot");
    assert_ne!(
        h.world().entities().position(slot),
        [162.5, 178.5],
        "the measured ticks moved nobody"
    );
}

/// The gather loop is under the same zero-allocation contract as movement.
///
/// The pool is warmed *outside* the scope on purpose — the two `nav.acquire`
/// calls each worker's round trip makes (to the node, then to the drop-off)
/// are misses the first time and settle the scratch heap then. Once every
/// destination has been visited once, every further tick is field hits and
/// bookkeeping, and the same bounded-miss exception `movement_allocates_nothing`
/// documents is the only thing exempt.
#[test]
fn the_gather_loop_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(workers.len(), 6);
    let crystal_nodes = h.ids_of_kind(EntityKind::Node(ResourceKind::Crystal));
    let (first_three, rest) = workers.split_at(3);
    assert_eq!(
        h.world_mut()
            .order_gather_group(first_three, crystal_nodes[0]),
        3
    );
    assert_eq!(h.world_mut().order_gather_group(rest, crystal_nodes[1]), 3);

    // Warm-up outside the scope: run one full round trip per worker so every
    // field this test will ever need (to each node, then to the HQ) has
    // already been built and the scratch heap has already settled. Ticked one
    // at a time and watched, not a single fixed-count snapshot: T10's
    // ring-based approach cell shifted the round-trip's cadence, so a lucky
    // exact tick count is not reliable evidence of a completed cycle any more.
    let mut saw_returning = false;
    for _ in 0..2_000 {
        h.step_exact(1);
        if workers.iter().any(|id| {
            matches!(
                h.world().order_of(*id),
                Some(Order::Gather {
                    phase: GatherPhase::Returning { .. },
                    ..
                })
            )
        }) {
            saw_returning = true;
            break;
        }
    }
    assert!(
        saw_returning,
        "warm-up must have driven at least one worker into a return trip, \
         or this measures a sweep that never exercised the drop-off field"
    );

    let guard = MeasureGuard::enter();
    h.step_exact(2_000);
    std::hint::black_box(h.tick_index());
    assert_eq!(guard.allocations(), 0, "the gather loop allocated");
    guard.assert_zero();
    drop(guard);

    // ...and the run really did gather: a stalled loop allocates nothing
    // either.
    assert!(
        h.world().resources().crystal > 300,
        "the measured ticks banked nothing"
    );
}

/// Selection is under the same zero-allocation contract as the rest of the
/// per-frame path: a box select is bounded and preallocated, not a fresh
/// `Vec` per drag.
#[test]
fn selection_operations_allocate_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let view = Camera::new(320, 320, 4.0, [1920.0, 1080.0], [166.0, 172.0]).iso_view();
    let a = view.project(162.5, 178.5);
    let b = view.project(167.5, 178.5);

    // T3's radius-aware initial spawn scatters the scene's six workers well
    // outside this box (they cannot share a cell one apart at a 3-cell body
    // radius); re-park them in a row first — this case means to measure box
    // select's own allocation behaviour, not depend on the scenario's default
    // spawn layout.
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    for (i, id) in workers.iter().enumerate() {
        let slot = h.world().entities().slot(*id).expect("live worker");
        h.world_mut()
            .entities_mut()
            .set_position(slot, [162.5 + i as f32, 178.5]);
    }

    // Warm-up outside the scope: whatever the selection/scratch buffers grow
    // to on their first use happens now.
    let warm = h.world_mut().box_select_into_selection(&view, a, b);
    assert_eq!(warm, 6, "the box must actually pick the six spawn workers");

    let guard = MeasureGuard::enter();
    for _ in 0..100 {
        let n = h.world_mut().box_select_into_selection(&view, a, b);
        std::hint::black_box(n);
    }
    assert_eq!(guard.allocations(), 0, "box select allocated");
    guard.assert_zero();
}

/// Construction is under the same zero-allocation contract as the rest of the
/// per-tick path — including the one bounded exception: a finished building's
/// footprint stamp (`FieldPool::set_blocked`) invalidates every cached field,
/// and the misses that follow must land in the same reused scratch heap the
/// pool already warmed, not a fresh allocation.
#[test]
fn construction_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let workers = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker));
    assert_eq!(workers.len(), 6);

    // Three obstacle-, node- and HQ-free Depot footprints, mutually
    // non-overlapping, each paid from the scene's exact starting 300 crystal.
    let sites: Vec<_> = [
        Cell { x: 180, y: 176 },
        Cell { x: 198, y: 176 },
        Cell { x: 210, y: 176 },
    ]
    .into_iter()
    .enumerate()
    .map(|(i, min)| {
        assert!(h.world_mut().begin_placement(BuildingKind::Depot));
        let builder = workers[i * 2];
        let site = h
            .world_mut()
            .confirm_placement(min, builder)
            .expect("confirm");
        assert!(h.world_mut().order_build(workers[i * 2 + 1], site));
        site
    })
    .collect();
    assert_eq!(h.world().resources().crystal, 0);

    // Warm-up outside the scope: run every builder's walk-in and let
    // attendance (and the fields that requires) settle before measuring.
    let mut attending = 0;
    for _ in 0..600 {
        h.step_exact(1);
        attending = sites
            .iter()
            .filter(|&&s| {
                let slot = h.world().entities().slot(s);
                slot.is_some_and(|slot| h.world().entities().progress(slot) > 0)
            })
            .count();
        if attending == sites.len() {
            break;
        }
    }
    assert_eq!(
        attending,
        sites.len(),
        "every site must have started attending"
    );

    let guard = MeasureGuard::enter();
    h.step_exact(600);
    std::hint::black_box(h.tick_index());
    assert_eq!(guard.allocations(), 0, "the construction sweep allocated");
    guard.assert_zero();
    drop(guard);

    // ...and the run really did build something: at least one site finished
    // (which is what exercises the bounded-miss exception above).
    assert!(
        sites.iter().any(|&s| !h.world().is_site(s)),
        "the measured ticks finished nothing"
    );
}

/// Production queues, completion spawns and supply recount reuse preallocated
/// storage. Two active producers ensure both unit kinds traverse the hot path.
#[test]
fn production_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    h.world_mut().resources_mut().crystal = 10_000;
    h.world_mut().resources_mut().gas = 10_000;
    let barracks = h
        .world_mut()
        .entities_mut()
        .spawn(
            EntityKind::Building(BuildingKind::Barracks),
            OWNER_PLAYER,
            [203.0, 181.0],
        )
        .expect("spawn finished Barracks");
    let hq = h.world().start_hq().expect("hq");
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert!(h.world_mut().enqueue_unit(hq, UnitKind::Worker).is_ok());
    assert!(
        h.world_mut()
            .enqueue_unit(barracks, UnitKind::Soldier)
            .is_ok()
    );

    let guard = MeasureGuard::enter();
    h.step_exact(600);
    std::hint::black_box(h.tick_index());
    assert_eq!(guard.allocations(), 0, "the production sweep allocated");
    guard.assert_zero();
    drop(guard);

    assert_eq!(
        h.ids_of_kind(EntityKind::Unit(UnitKind::Worker)).len(),
        8,
        "both queued Workers must complete"
    );
    assert_eq!(
        h.ids_of_kind(EntityKind::Unit(UnitKind::Soldier)).len(),
        1,
        "queued Soldier must complete"
    );
}

/// T3: `FieldPool::from_blocked_mask` reserves its scratch heap to the true
/// worst case (`8 * cells + 1`), so the very first field build after load — a
/// guaranteed cold miss, since nothing has ever called `acquire` on a freshly
/// seeded world — allocates nothing at all. Previously only the *second*
/// rebuild at a grid size was guaranteed not to grow the heap; this is the
/// stronger, ticket-mandated bound: not even the first one may.
#[test]
fn cold_field_acquire_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let worker = h.ids_of_kind(EntityKind::Unit(UnitKind::Worker))[0];

    let guard = MeasureGuard::enter();
    let ok = h.world_mut().order_move(worker, Cell { x: 50, y: 50 });
    std::hint::black_box(ok);
    assert_eq!(
        guard.allocations(),
        0,
        "the first cold field acquire after load allocated"
    );
    guard.assert_zero();
    drop(guard);

    assert!(
        ok,
        "the destination must have been legal, or this measured nothing"
    );
}
