//! T29 deterministic system test harness contract.
//!
//! Every game-system test drives one seeded headless entry point. The three
//! properties asserted here are what make the harness usable as a foundation:
//! a run is reproducible (`same_seed_same_state_hash`), it is sensitive to its
//! seed (`different_seed_differs`), and it advances by an exact tick count
//! (`tick_count_is_exact`) with no wall-clock input (`no_wall_clock_dependence`)
//! and no GPU (`harness_headless_without_gpu`).

use std::process::Command;
use std::time::Duration;

use mmd_engine::scenario::ScenarioError;
use mmd_engine::sim::{SPEED_CELLS_PER_SEC, TICK_DT};
use mmd_engine::testkit::{
    ALL_FIXTURES, FIXTURE_CORRIDOR_V1, FIXTURE_SMALL_V1, Harness, ScenarioSource,
};

/// Canonical reproducibility run: one fixture, one agent count, one seed, one
/// tick count. Shared by the in-process and cross-process halves of
/// `same_seed_same_state_hash` so both provably measure the same thing.
const CANON_AGENTS: u32 = 64;
const CANON_SEED: u64 = 42;
const CANON_TICKS: u64 = 500;

/// Env flag + test name used to re-enter this binary as a child process.
const CHILD_ENV: &str = "MMD_HARNESS_CHILD_HASH";
const CHILD_TEST: &str = "print_state_hash_for_child_process";
const HASH_PREFIX: &str = "MMD_STATE_HASH=";

fn canonical_hash() -> String {
    let mut h = Harness::fixture(FIXTURE_SMALL_V1)
        .agents(CANON_AGENTS)
        .seed(CANON_SEED)
        .build()
        .expect("build canonical harness");
    h.step_exact(CANON_TICKS);
    h.state_hash_hex()
}

/// Child entry point for `same_seed_same_state_hash`. This is a *fixture*, not
/// a check — it asserts nothing and is `#[ignore]`d so a normal run does not
/// count it as a passing test. The parent runs it with `--ignored` and proves
/// it executed by asserting on its output. Do not "fix" it into a real test.
#[test]
#[ignore = "child entry point driven by same_seed_same_state_hash"]
fn print_state_hash_for_child_process() {
    assert!(
        std::env::var_os(CHILD_ENV).is_some(),
        "child entry point must only run via the parent, which sets {CHILD_ENV}"
    );
    println!("{HASH_PREFIX}{}", canonical_hash());
}

/// Run the canonical scenario in a genuinely separate process.
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
    // A filter that matches nothing also exits 0, so prove a test actually ran
    // rather than inferring it from the exit status.
    assert!(
        stdout.contains("1 passed"),
        "child ran no test — filter or test name drifted:\n{stdout}"
    );
    assert_eq!(
        stdout.matches(HASH_PREFIX).count(),
        1,
        "expected exactly one hash marker from the child:\n{stdout}"
    );

    // `--nocapture` interleaves the marker with libtest's own progress line,
    // so locate the marker inside the line rather than at its start.
    let tail = stdout
        .find(HASH_PREFIX)
        .map(|i| &stdout[i + HASH_PREFIX.len()..])
        .unwrap_or_else(|| panic!("child printed no `{HASH_PREFIX}` marker:\n{stdout}"));
    let hash: String = tail.chars().take_while(char::is_ascii_hexdigit).collect();
    assert_eq!(hash.len(), 64, "child hash malformed: {hash:?}");
    hash
}

#[test]
fn same_seed_same_state_hash() {
    let first = canonical_hash();
    let second = canonical_hash();
    assert_eq!(
        first, second,
        "same seed + tick count must reproduce the state hash within a process"
    );

    // "Across processes on the same host" is a strictly stronger claim than a
    // double run: it also rules out process-lifetime state (address-dependent
    // ordering, lazily-initialized globals, hash seeds).
    let child = hash_from_child_process();
    assert_eq!(
        first, child,
        "same seed must reproduce the state hash in a separate process"
    );
}

#[test]
fn different_seed_differs() {
    let mut a = Harness::fixture(FIXTURE_SMALL_V1)
        .agents(CANON_AGENTS)
        .seed(42)
        .build()
        .expect("seed 42");
    let mut b = Harness::fixture(FIXTURE_SMALL_V1)
        .agents(CANON_AGENTS)
        .seed(43)
        .build()
        .expect("seed 43");
    let canonical = Harness::fixture(FIXTURE_SMALL_V1)
        .agents(CANON_AGENTS)
        .seed(0)
        .build()
        .expect("seed 0");

    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "seed must change the initial state, or it is not wired to anything"
    );
    assert_ne!(
        a.agents().x,
        canonical.agents().x,
        "a seeded run must differ from the canonical round-robin placement"
    );

    // Seeding is defined as moving start positions only. If it ever permuted
    // the animation channels it would silently break the T5 even-distribution
    // contract while every hash assertion still passed.
    assert_eq!(
        a.agents().atlas,
        canonical.agents().atlas,
        "seed must not touch atlas assignment"
    );
    assert_eq!(a.agents().dir, canonical.agents().dir);
    assert_eq!(a.agents().frame, canonical.agents().frame);

    // And it must place agents *on spawn cells*, not at arbitrary coordinates.
    let spawns: std::collections::HashSet<(u32, u32)> = a
        .scenario()
        .spawn_cells()
        .iter()
        .map(|c| (c.x, c.y))
        .collect();
    let v = a.agents();
    for i in 0..v.x.len() {
        let cell = (v.x[i].floor() as u32, v.y[i].floor() as u32);
        assert!(
            spawns.contains(&cell),
            "seeded agent {i} placed off-spawn at ({}, {})",
            v.x[i],
            v.y[i]
        );
    }

    a.step_exact(CANON_TICKS);
    b.step_exact(CANON_TICKS);
    assert_ne!(
        a.state_hash_hex(),
        b.state_hash_hex(),
        "distinct seeds must produce distinct trajectories"
    );
}

#[test]
fn tick_count_is_exact() {
    let mut h = Harness::fixture(FIXTURE_SMALL_V1)
        .agents(CANON_AGENTS)
        .seed(CANON_SEED)
        .build()
        .expect("harness");
    assert_eq!(h.tick_index(), 0);
    assert_eq!(h.ticks_applied(), 0);

    let applied = h.step(CANON_TICKS);
    assert_eq!(
        applied, CANON_TICKS,
        "step must apply exactly what it claims"
    );
    assert_eq!(h.tick_index(), CANON_TICKS, "sim tick index must match");
    assert_eq!(h.ticks_applied(), CANON_TICKS);

    // Split runs accumulate exactly, with no off-by-one at the boundary.
    let applied = h.step(7);
    assert_eq!(applied, 7);
    assert_eq!(h.tick_index(), CANON_TICKS + 7);
    assert_eq!(h.ticks_applied(), CANON_TICKS + 7);

    // A paused harness applies zero ticks and reports it honestly.
    h.set_paused(true);
    let applied = h.step(10);
    assert_eq!(applied, 0, "paused harness must apply no ticks");
    assert_eq!(h.tick_index(), CANON_TICKS + 7, "paused must not advance");

    // step_exact refuses to silently under-apply.
    h.set_paused(false);
    h.step_exact(3);
    assert_eq!(h.tick_index(), CANON_TICKS + 10);

    // The render path keeps its own tick accounting; it must agree.
    let before = h.tick_index();
    h.render_frame();
    assert_eq!(
        h.tick_index(),
        before + 1,
        "render_frame must apply one tick"
    );
    assert_eq!(h.ticks_applied(), before + 1);
    h.set_paused(true);
    h.render_frame();
    assert_eq!(
        h.tick_index(),
        before + 1,
        "paused render_frame must not tick"
    );
    assert_eq!(
        h.ticks_applied(),
        before + 1,
        "paused render_frame must not count a tick"
    );
}

#[test]
fn no_wall_clock_dependence() {
    let baseline = canonical_hash();

    // Same run, but stretched over real time with uneven pauses. Any dependence
    // on elapsed time, frame pacing, or a monotonic clock would show up here.
    let mut h = Harness::fixture(FIXTURE_SMALL_V1)
        .agents(CANON_AGENTS)
        .seed(CANON_SEED)
        .build()
        .expect("delayed harness");
    let chunks = [200u64, 1, 149, 150];
    assert_eq!(chunks.iter().sum::<u64>(), CANON_TICKS);
    let started = std::time::Instant::now();
    for (i, chunk) in chunks.iter().enumerate() {
        std::thread::sleep(Duration::from_millis(if i % 2 == 0 { 25 } else { 5 }));
        h.step_exact(*chunk);
    }
    let stretched = started.elapsed();

    // Without this the test silently degenerates into a second copy of
    // `same_seed_same_state_hash` if the delays are ever removed or hoisted.
    assert!(
        stretched >= Duration::from_millis(50),
        "delay was never injected between ticks ({stretched:?}) — this proves nothing"
    );
    assert_eq!(h.tick_index(), CANON_TICKS);
    assert_eq!(
        h.state_hash_hex(),
        baseline,
        "artificial delay changed the state hash — the sim reads a clock"
    );
}

#[test]
fn harness_headless_without_gpu() {
    // No GpuContext, no SpriteRenderer, no window: a sim-only harness must
    // build, step, and expose full state on a machine with no device at all.
    let mut h = Harness::fixture(FIXTURE_CORRIDOR_V1)
        .seed(9)
        .build()
        .expect("headless harness");

    assert_eq!(
        h.alive_count(),
        h.scenario().hard_agent_count() as usize,
        "harness must honour the scenario's population"
    );
    h.step_exact(120);

    let agents = h.agents();
    assert!(
        [
            agents.y.len(),
            agents.atlas.len(),
            agents.dir.len(),
            agents.frame.len()
        ]
        .iter()
        .all(|&n| n == agents.x.len()),
        "SoA channels desynced"
    );
    for i in 0..agents.x.len() {
        assert!(
            agents.x[i].is_finite() && agents.y[i].is_finite(),
            "agent {i} left the numeric domain"
        );
    }

    // The CPU half of the render path also works with no device: packing SoA
    // state into draw groups is pure arithmetic.
    let groups = h.render_frame_groups();
    let packed: usize = groups.iter().map(|g| g.instances.len()).sum();
    assert_eq!(
        packed,
        h.alive_count(),
        "every agent must pack to an instance"
    );
}

#[test]
fn harness_exposes_spawn_and_recycle_counters() {
    let mut h = Harness::fixture(FIXTURE_SMALL_V1)
        .agents(CANON_AGENTS)
        .seed(CANON_SEED)
        .build()
        .expect("harness");
    assert_eq!(h.recycled_count(), 0, "no recycles before the first tick");

    // Ground truth independent of the counter itself: a recycle is the only way
    // an agent can move further in one tick than its speed allows. Comparing
    // the counter against observed teleports means a counter stuck at 0 (or
    // inflated) fails, where `spawned == alive + recycled` could not — that is
    // an identity of the accessors and holds for any sim behaviour.
    let step_len = SPEED_CELLS_PER_SEC * TICK_DT;
    let mut prev: Vec<(f32, f32)> = (0..h.alive_count())
        .map(|i| (h.agents().x[i], h.agents().y[i]))
        .collect();
    let mut observed_recycles = 0u64;
    for _ in 0..CANON_TICKS {
        h.step_exact(1);
        let v = h.agents();
        for (i, slot) in prev.iter_mut().enumerate() {
            let moved = ((v.x[i] - slot.0).powi(2) + (v.y[i] - slot.1).powi(2)).sqrt();
            if moved > step_len * 2.0 {
                observed_recycles += 1;
            }
            *slot = (v.x[i], v.y[i]);
        }
    }

    assert!(
        observed_recycles > 0,
        "agents should reach the destination and recycle within {CANON_TICKS} ticks"
    );
    assert_eq!(
        h.recycled_count(),
        observed_recycles,
        "recycle counter must match independently observed respawns"
    );
    assert_eq!(
        h.spawned_count(),
        CANON_AGENTS as u64 + observed_recycles,
        "total spawn events = initial seeding + observed recycles"
    );
    assert_eq!(
        h.alive_count(),
        CANON_AGENTS as usize,
        "recycling must not change the population"
    );
}

#[test]
fn gate_scene_harness_matches_direct_runtime() {
    // The harness is only a valid substitute for hand-built tests if its
    // default (unseeded) run is bit-identical to constructing the runtime
    // directly. Seed 0 is defined as "canonical order".
    use mmd_engine::runtime::Runtime;

    let mut direct = Runtime::load(
        mmd_engine::workspace_root().join("assets/scenarios/technical_prototype_v1.ron"),
        Some(512),
    )
    .expect("direct runtime");
    let mut h = Harness::gate_scene()
        .agents(512)
        .seed(0)
        .build()
        .expect("gate harness");

    assert_eq!(
        h.state_hash(),
        direct.state_hash(),
        "initial state must match"
    );
    for _ in 0..30 {
        direct.tick_and_render();
    }
    h.step_exact(30);
    assert_eq!(
        h.state_hash(),
        direct.state_hash(),
        "harness stepping must not diverge from Runtime::tick_and_render"
    );
    assert_eq!(h.scenario().version(), "technical_prototype_v1");
}

#[test]
fn fixture_scenarios_are_hash_verified() {
    // Fixtures obey the same tracked-hash contract as the gate scene: a
    // tampered fixture must fail to load rather than silently drift.
    for name in ALL_FIXTURES {
        let name = *name;
        let path = mmd_engine::testkit::fixture_path(name);
        assert!(path.is_file(), "missing tracked fixture {}", path.display());
        let sha = path.with_extension("sha256");
        assert!(sha.is_file(), "missing tracked sidecar {}", sha.display());

        let scenario = mmd_engine::scenario::Scenario::load_verified(&path)
            .unwrap_or_else(|e| panic!("fixture {name} must load: {e}"));
        assert_eq!(scenario.version(), name, "version id must match file stem");

        // The explicit-path source resolves to the same verified scenario.
        let by_path = Harness::builder(ScenarioSource::path(&path))
            .build()
            .expect("path source");
        assert_eq!(by_path.scenario().version(), name);

        let mut bytes = std::fs::read(&path).expect("read fixture");
        let expected = std::fs::read_to_string(&sha).expect("read sidecar");
        let idx = bytes
            .iter()
            .position(|b| *b == b'0')
            .expect("digit to flip");
        bytes[idx] = b'1';
        let err = mmd_engine::scenario::Scenario::from_verified_bytes(&bytes, expected.trim())
            .expect_err("tampered fixture must be rejected");
        assert!(
            matches!(err, ScenarioError::HashMismatch { .. }),
            "expected HashMismatch for {name}, got {err:?}"
        );
    }
}

#[test]
fn fixture_scenarios_stay_small() {
    // *Enforcement* of the fixture caps is pinned by the negative tests in
    // `scenario_contract.rs` (asserting a loaded scenario is under a cap only
    // restates the committed data). This asserts the committed fixtures are
    // genuinely the fast ones the harness promises — far below the hard cap.
    for name in ALL_FIXTURES {
        let name = *name;
        let h = Harness::builder(ScenarioSource::fixture(name))
            .build()
            .expect("fixture harness");
        let s = h.scenario();
        let cells = s.width() * s.height();
        assert!(
            cells <= 4_096,
            "{name}: {cells} cells is too big for a fast fixture"
        );
        assert!(
            (1..=512).contains(&s.hard_agent_count()),
            "{name}: {} agents is outside the tens/hundreds the harness promises",
            s.hard_agent_count()
        );
        assert!(
            s.obstacle_count() > 0,
            "{name}: fixtures must exercise obstacles"
        );
    }
}

#[test]
fn inline_grid_source_uses_the_same_entry_point() {
    // Surgical unit-scale tests build a synthetic grid through the same
    // Harness entry point rather than a parallel construction path.
    use mmd_engine::scenario::Cell;
    use mmd_engine::testkit::GridSpec;

    let spec = GridSpec::new(16, 3, Cell { x: 15, y: 1 }).with_spawns(vec![Cell { x: 1, y: 1 }]);
    let mut h = Harness::grid(spec).agents(1).build().expect("inline grid");

    assert_eq!(h.alive_count(), 1);
    assert_eq!(h.scenario().width(), 16);
    let (x0, y0) = (h.agents().x[0], h.agents().y[0]);
    assert!((x0 - 1.5).abs() < 1e-6 && (y0 - 1.5).abs() < 1e-6);
    h.step_exact(1);
    // Same tolerance as the movement contract in `simulation.rs`, so this is
    // an API-shape test that does not silently weaken the numeric contract.
    assert!(
        (h.agents().x[0] - (x0 + SPEED_CELLS_PER_SEC * TICK_DT)).abs() < 1e-5,
        "inline grid must obey the same movement contract"
    );
}

#[test]
fn source_is_reusable_for_future_systems() {
    // Phase-1 systems (camera, selection, workers) need the runtime itself and
    // a deterministic seed stream, not just a hash. Both are reachable.
    let mut h = Harness::fixture(FIXTURE_SMALL_V1)
        .seed(7)
        .build()
        .expect("harness");
    assert_eq!(h.seed(), 7);

    let mut stream = h.rng("camera");
    let a = stream.next_u64();
    let b = stream.next_u64();
    assert_ne!(a, b, "seed stream must advance");
    assert_eq!(
        h.rng("camera").next_u64(),
        a,
        "a labelled stream must be re-derivable from the harness seed"
    );
    assert_ne!(
        h.rng("selection").next_u64(),
        a,
        "two systems must not share one stream, or their draws correlate"
    );

    // Runtime access keeps input/pause/overlay behaviour available to tests
    // that grow later, without a harness redesign.
    h.runtime_mut()
        .apply_action(mmd_engine::runtime::InputAction::ToggleOverlay);
    assert!(h.runtime().overlay_visible());
    assert!(h.flow_field().width() > 0, "nav state must be reachable");
}
