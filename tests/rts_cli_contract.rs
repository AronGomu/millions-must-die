//! T14: the `rts` subcommand's stdout contract, CLI-level bindings, and
//! command semantics — mouse and keyboard, driven from `--inject-input`.
//!
//! Same discipline as `tests/cli_contract.rs`: drives the real binary as a
//! subprocess, forcing `SDL_VIDEODRIVER=offscreen` unless a case specifically
//! wants a window. `run` and `bench` are untouched by this file.
//!
//! # Geometry
//!
//! `assets/scenarios/rts_prototype_v1.ron` fixes the scene these tests click
//! on: `hq_cell: (160, 160)`, `HQ_FOOTPRINT_CELLS = 12` so the HQ's centre —
//! and the camera's start position, per `RtsWorld::from_scenario` — is cell
//! `(166, 166)`. `cell_size_px: 4` makes `tile_w = 8`, `tile_h = 4`. Camera
//! start centres that cell on screen, so the projection's `origin` is fixed
//! at `(960, -124)` for every run that never pans. [`screen_of`] reproduces
//! `mmd_engine::render::iso_project` against that fixed origin so a test can
//! name a *cell* and get the *pixel* `click_select`/`pick_at` actually read
//! (they unproject the click, not the sprite's drawn position).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Serializes every subprocess this file spawns against the real GPU.
///
/// Several cases here run thousands of frames (`a_right_click_on_a_node_starts_gathering`,
/// `a_left_click_places_the_ghost`), far more sustained GPU work per process
/// than `tests/cli_contract.rs`'s small-scene cases. Run fully parallel (the
/// default `cargo test` behaviour), enough of those overlap on one physical
/// device to trip `VK_ERROR_DEVICE_LOST` — observed directly on this host.
/// One process on the device at a time trades this file's own wall time for
/// determinism; it does not reach across test binaries.
static GPU_LOCK: Mutex<()> = Mutex::new(());

fn gpu_guard() -> MutexGuard<'static, ()> {
    GPU_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Exit code for a failure the user can act on.
const EXIT_ERROR: i32 = 1;
/// Exit code clap uses for a usage error.
const EXIT_USAGE: i32 = 2;
/// Exit code the app reserves for "this host has no usable GPU device".
const EXIT_NO_GPU: i32 = 3;

/// Env var that turns every GPU skip in this file into a hard failure.
const REQUIRE_GPU_ENV: &str = "MMD_REQUIRE_GPU";

// ---------------------------------------------------------------------------
// Scene geometry — derived from assets/scenarios/rts_prototype_v1.ron
// ---------------------------------------------------------------------------

/// Camera projection origin at the scene's start (HQ-centred, no pan).
const ORIGIN: [f32; 2] = [960.0, -124.0];
const TILE_W: f32 = 8.0;
const TILE_H: f32 = 4.0;

/// The screen pixel `iso_project(cx, cy, ..)` produces at the fixed start
/// origin — and so the pixel a plain (unpanned) `click`/`drag` must name to
/// land on cell-space point `(cx, cy)`.
fn screen_of(cx: f32, cy: f32) -> [f32; 2] {
    [
        ORIGIN[0] + (cx - cy) * TILE_W * 0.5,
        ORIGIN[1] + (cx + cy) * TILE_H * 0.5,
    ]
}

fn fmt_xy(p: [f32; 2]) -> String {
    format!("{},{}", p[0], p[1])
}

/// One worker's spawn point. `spawn_cells` names six cells one apart
/// (`162..167 @ y=178`), but T3's radius-aware initial spawn relocates every
/// worker but the first (a 3-cell-radius body cannot share a cell that close
/// with another) — the first cell is still legal on its own, so it is the one
/// entry this helper can still name directly.
fn worker_screen() -> [f32; 2] {
    screen_of(162.5, 178.5)
}

/// A drag rectangle in screen space covering every one of the six
/// (T3-relocated) starting workers' projected positions:
/// `(162.5, 178.5)`, `(168.5, 178.5)`, `(164.5, 184.5)`, `(170.5, 184.5)`,
/// `(174.5, 178.5)`, `(175.5, 172.5)` — the same box
/// `crates/mmd-engine/tests/rts_acceptance.rs`'s `DRAG_A`/`DRAG_B` use, for
/// the same reason.
fn spawn_group_drag() -> (String, String) {
    (fmt_xy([850.0, 520.0]), fmt_xy([1000.0, 600.0]))
}

/// A point inside the HQ's own footprint that picks the HQ.
///
/// Not [`hq_screen`]: T3's radius-aware initial spawn happens to leave one
/// relocated worker (whose rendered sprite quad is far larger than its own
/// body — `RTS_SPRITE_SIZE_PX` is 12 cells across) with a screen-space hit
/// region that reaches back over the HQ's own screen centre and outranks it
/// on pick depth. The HQ's own footprint corner is clear of every worker's
/// hit region and still resolves to `Pick::Building`.
fn hq_click_screen() -> [f32; 2] {
    screen_of(160.5, 160.5)
}

/// The nearest crystal node (`crystal_nodes[0] = (140, 150)`).
fn crystal_node_screen() -> [f32; 2] {
    screen_of(140.5, 150.5)
}

/// A corner of that node's rendered 48x48 sprite quad, not its centre.
///
/// `RTS_SPRITE_SIZE_PX` is `[48.0, 48.0]`, anchored bottom-centre
/// (`mmd_engine::rts::sprite_screen_rect`): x spans the ground point ±24 px,
/// y spans it `-48..0` px. `(-20, -4)` sits just inside the bottom-left
/// corner, and on the side away from the scenario's next-nearest crystal
/// node (`146, 146`) so the corner cannot also land on that neighbour's quad.
fn crystal_node_corner_screen() -> [f32; 2] {
    let g = crystal_node_screen();
    [g[0] - 20.0, g[1] - 4.0]
}

/// A cell well clear of the HQ footprint (`160..172`), both resource nodes,
/// and — empirically, see `a_left_click_places_the_ghost` — the scenario's
/// obstacle mask: `(180, 150)`, a Depot-sized (8-cell) footprint centred
/// there.
fn clear_build_site_screen() -> [f32; 2] {
    screen_of(180.5, 150.5)
}

/// 30 cells south of the HQ: far enough that a move order visibly changes
/// the state hash inside the frame budgets these tests use.
fn far_move_target_screen() -> [f32; 2] {
    screen_of(166.0, 196.0)
}

// ---------------------------------------------------------------------------
// Invocation helper — mirrors tests/cli_contract.rs
// ---------------------------------------------------------------------------

struct Cli {
    args: Vec<String>,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Cli {
    fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }

    fn exit_line(&self) -> &str {
        self.stdout
            .lines()
            .find(|l| l.starts_with("rts: clean exit"))
            .unwrap_or_else(|| panic!("{self}\nno `rts: clean exit` line"))
    }

    fn frame0_line(&self) -> &str {
        self.stdout
            .lines()
            .find(|l| l.starts_with("rts: frame0"))
            .unwrap_or_else(|| panic!("{self}\nno `rts: frame0` line"))
    }

    fn field<'a>(&self, line: &'a str, key: &str) -> &'a str {
        line.split_whitespace()
            .find_map(|tok| tok.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("{self}\nline `{line}` has no `{key}=`"))
    }

    fn exit_field(&self, key: &str) -> &str {
        self.field(self.exit_line(), key)
    }

    fn final_hash(&self) -> String {
        let hash = self.exit_field("hash").to_string();
        assert_eq!(hash.len(), 64, "{self}\nstate hash is not 32 bytes of hex");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "{self}\nstate hash is not hex"
        );
        hash
    }

    fn assert_success(&self) -> &Self {
        assert_eq!(
            self.code,
            Some(0),
            "{self}\nexpected a clean exit 0 (a `None` code means the process died on a signal)"
        );
        self
    }

    fn assert_actionable_failure(&self) -> &Self {
        assert_eq!(
            self.code,
            Some(EXIT_ERROR),
            "{self}\nactionable failures exit {EXIT_ERROR}; {EXIT_NO_GPU} is reserved for a \
             missing GPU device"
        );
        let combined = self.combined();
        for crash in ["panicked at", "stack backtrace", "RUST_BACKTRACE"] {
            assert!(
                !combined.contains(crash),
                "{self}\nfailure surfaced `{crash}` — a stack trace is not UX"
            );
        }
        self
    }

    fn assert_says(&self, needles: &[&str]) -> &Self {
        let combined = self.combined();
        for needle in needles {
            assert!(
                combined.contains(needle),
                "{self}\nmessage does not name `{needle}`"
            );
        }
        self
    }

    fn hud_lines(&self) -> usize {
        self.stdout
            .lines()
            .filter(|l| l.starts_with("rts: hud "))
            .count()
    }
}

impl std::fmt::Display for Cli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "--- millions_must_die {}\n--- exit {:?}\n--- stdout\n{}--- stderr\n{}---",
            self.args.join(" "),
            self.code,
            self.stdout,
            self.stderr
        )
    }
}

fn app_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_millions_must_die"))
}

/// Invoke the app with `args`. `offscreen` forces SDL's offscreen video
/// driver so a test never opens a window on the developer's desktop.
fn invoke(args: &[&str], offscreen: bool) -> Cli {
    let mut cmd = app_bin();
    cmd.args(args);
    if offscreen {
        cmd.env("SDL_VIDEODRIVER", "offscreen");
    }
    // The app also reads these; a developer's shell must not steer a test.
    cmd.env_remove("MMD_RTS_FRAMES");
    cmd.env_remove("MMD_RTS_ONCE");
    run_to_completion(cmd, args.join(" "))
}

const RUN_DEADLINE: Duration = Duration::from_secs(120);

fn run_to_completion(mut cmd: Command, label: String) -> Cli {
    let _guard = gpu_guard();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = tmp_dir("proc");
    let out_path = dir.join(format!("{id}.out"));
    let err_path = dir.join(format!("{id}.err"));
    let out_file = File::create(&out_path).expect("create stdout capture");
    let err_file = File::create(&err_path).expect("create stderr capture");
    let mut child = cmd
        .stdout(out_file)
        .stderr(err_file)
        .spawn()
        .expect("spawn millions_must_die");

    let started = Instant::now();
    let status = loop {
        match child.try_wait().expect("poll millions_must_die") {
            Some(status) => break Some(status),
            None if started.elapsed() >= RUN_DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };

    let read = |p: &std::path::Path| std::fs::read_to_string(p).unwrap_or_default();
    let cli = Cli {
        args: vec![label],
        code: status.and_then(|s| s.code()),
        stdout: read(&out_path),
        stderr: read(&err_path),
    };
    assert!(
        status.is_some(),
        "{cli}\nthe run did not terminate within {RUN_DEADLINE:?} and was killed — \
         a frame budget stopped being honoured"
    );
    cli
}

/// Run `rts` with `extra` args, offscreen.
fn rts(extra: &[&str]) -> Cli {
    let mut args = vec!["rts"];
    args.extend_from_slice(extra);
    invoke(&args, true)
}

fn gpu_is_required() -> bool {
    std::env::var_os(REQUIRE_GPU_ENV).is_some_and(|v| v != "0" && !v.is_empty())
}

/// Classify a completed run: `None` when this host simply has no GPU device.
fn or_skip(case: &str, cli: Cli) -> Option<Cli> {
    if cli.code == Some(EXIT_NO_GPU) {
        assert!(
            !gpu_is_required(),
            "{cli}\n{case}: skipped (no GPU device) but {REQUIRE_GPU_ENV} is set — \
             this host is declared to have a GPU, so a skip is a missed verification"
        );
        assert!(
            cli.combined()
                .contains("GPU device unavailable on this host"),
            "{cli}\n{case}: exit {EXIT_NO_GPU} must explain itself"
        );
        eprintln!("SKIP {case}: no GPU device on this host");
        return None;
    }
    Some(cli)
}

fn tmp_dir(case: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(case);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

// ---------------------------------------------------------------------------
// 1. Lifecycle — start, tick, exit
// ---------------------------------------------------------------------------

#[test]
fn rts_runs_headless_and_exits_clean() {
    let Some(cli) = or_skip(
        "rts_runs_headless_and_exits_clean",
        rts(&["--frames", "30"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("mode"), "offscreen", "{cli}");
    assert_eq!(
        cli.stdout
            .lines()
            .filter(|l| l.starts_with("rts: clean exit"))
            .count(),
        1,
        "{cli}\nexpected exactly one clean-exit line"
    );
}

#[test]
fn frame0_line_reports_three_world_groups() {
    let Some(cli) = or_skip(
        "frame0_line_reports_three_world_groups",
        rts(&["--frames", "1"]),
    ) else {
        return;
    };
    cli.assert_success();
    let line = cli.frame0_line();
    let world = cli.field(line, "world");
    let ui = cli.field(line, "ui");
    assert_eq!(
        world
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .count(),
        3,
        "{cli}\n`world=` must carry exactly 3 numbers: {world}"
    );
    assert_eq!(
        ui.trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .count(),
        5,
        "{cli}\n`ui=` must carry exactly 5 numbers: {ui}"
    );
}

#[test]
fn frame0_hash_is_sixty_four_hex() {
    let Some(cli) = or_skip("frame0_hash_is_sixty_four_hex", rts(&["--frames", "1"])) else {
        return;
    };
    cli.assert_success();
    let hash = cli.field(cli.frame0_line(), "hash");
    assert_eq!(hash.len(), 64, "{cli}");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{cli}");
}

#[test]
fn the_run_is_deterministic() {
    let Some(a) = or_skip("the_run_is_deterministic", rts(&["--frames", "50"])) else {
        return;
    };
    let Some(b) = or_skip("the_run_is_deterministic", rts(&["--frames", "50"])) else {
        return;
    };
    a.assert_success();
    b.assert_success();
    assert_eq!(a.final_hash(), b.final_hash(), "{a}\n{b}");
}

#[test]
fn frames_zero_is_rejected() {
    let cli = rts(&["--frames", "0"]);
    cli.assert_actionable_failure().assert_says(&["--frames 0"]);
}

#[test]
fn env_frame_budget_is_honoured() {
    let mut cmd = app_bin();
    cmd.args(["rts"])
        .env("SDL_VIDEODRIVER", "offscreen")
        .env("MMD_RTS_FRAMES", "5")
        .env_remove("MMD_RTS_ONCE");
    let Some(cli) = or_skip(
        "env_frame_budget_is_honoured",
        run_to_completion(cmd, "MMD_RTS_FRAMES=5 rts".to_string()),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("frames"), "5", "{cli}");
}

#[test]
fn a_malformed_env_budget_is_rejected() {
    let mut cmd = app_bin();
    cmd.args(["rts"])
        .env("SDL_VIDEODRIVER", "offscreen")
        .env("MMD_RTS_FRAMES", "abc")
        .env_remove("MMD_RTS_ONCE");
    run_to_completion(cmd, "MMD_RTS_FRAMES=abc rts".to_string())
        .assert_actionable_failure()
        .assert_says(&["MMD_RTS_FRAMES"]);
}

#[test]
fn run_and_rts_budgets_are_independent() {
    let mut cmd = app_bin();
    cmd.args(["rts", "--frames", "3"])
        .env("SDL_VIDEODRIVER", "offscreen")
        .env("MMD_RUN_FRAMES", "7")
        .env_remove("MMD_RTS_FRAMES")
        .env_remove("MMD_RTS_ONCE");
    let Some(cli) = or_skip(
        "run_and_rts_budgets_are_independent",
        run_to_completion(cmd, "MMD_RUN_FRAMES=7 rts --frames 3".to_string()),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("frames"), "3", "{cli}");
    assert!(
        !cli.combined().contains("MMD_RUN_FRAMES"),
        "{cli}\n`rts` must never mention `run`'s env var"
    );

    // The `--frames 3` case above never even reaches the env-var fallback
    // (an explicit flag short-circuits it), so it cannot by itself catch
    // `rts` reading the wrong env var. Drop `--frames` and set both env vars
    // to conflicting budgets: only reading `MMD_RTS_FRAMES` (not `run`'s)
    // explains `frames=3` here.
    let mut cmd2 = app_bin();
    cmd2.args(["rts"])
        .env("SDL_VIDEODRIVER", "offscreen")
        .env("MMD_RUN_FRAMES", "7")
        .env("MMD_RTS_FRAMES", "3")
        .env_remove("MMD_RTS_ONCE");
    let Some(cli2) = or_skip(
        "run_and_rts_budgets_are_independent",
        run_to_completion(cmd2, "MMD_RUN_FRAMES=7 MMD_RTS_FRAMES=3 rts".to_string()),
    ) else {
        return;
    };
    cli2.assert_success();
    assert_eq!(
        cli2.exit_field("frames"),
        "3",
        "{cli2}\n`rts` must read its own `MMD_RTS_FRAMES`, not `run`'s `MMD_RUN_FRAMES`"
    );
}

#[test]
fn a_missing_scenario_is_actionable() {
    let dir = tmp_dir("missing_scenario");
    let path = dir.join("nope.ron");
    let cli = rts(&["--scenario", path.to_str().unwrap()]);
    cli.assert_actionable_failure()
        .assert_says(&["scenario", &format!("{}: No such file", path.display())]);
}

#[test]
fn a_phase0_scenario_is_refused() {
    let cli = rts(&["--scenario", "assets/scenarios/technical_prototype_v1.ron"]);
    cli.assert_actionable_failure()
        .assert_says(&["carries no rts block"]);
}

// ---------------------------------------------------------------------------
// 2. Pause and overlay
// ---------------------------------------------------------------------------

#[test]
fn pause_freezes_the_tick() {
    let Some(cli) = or_skip(
        "pause_freezes_the_tick",
        rts(&["--frames", "20", "--inject-input", "2:key:space"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("tick"), "1", "{cli}");
    assert_eq!(cli.exit_field("frames"), "20", "{cli}");
    assert_eq!(cli.exit_field("paused"), "true", "{cli}");
}

#[test]
fn the_overlay_prints_one_line_per_frame() {
    let Some(cli) = or_skip(
        "the_overlay_prints_one_line_per_frame",
        rts(&["--frames", "10", "--inject-input", "1:key:f1"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.hud_lines(), 10, "{cli}");
}

#[test]
fn the_overlay_is_off_by_default() {
    let Some(cli) = or_skip("the_overlay_is_off_by_default", rts(&["--frames", "10"])) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.hud_lines(), 0, "{cli}");
}

#[test]
fn a_quit_on_frame_one_renders_nothing() {
    let Some(cli) = or_skip(
        "a_quit_on_frame_one_renders_nothing",
        rts(&["--inject-input", "1:quit"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("frames"), "0", "{cli}");
    assert_eq!(cli.exit_field("quit"), "true", "{cli}");
    assert!(
        !cli.stdout.contains("rts: frame0"),
        "{cli}\na run that quit before frame 1 must not report a first frame"
    );
}

#[test]
fn an_unfired_entry_fails_the_run() {
    let Some(cli) = or_skip(
        "an_unfired_entry_fails_the_run",
        rts(&["--frames", "3", "--inject-input", "50:key:esc"]),
    ) else {
        return;
    };
    cli.assert_actionable_failure().assert_says(&["50:esc"]);
}

// ---------------------------------------------------------------------------
// 3. Selection, orders, building, production
// ---------------------------------------------------------------------------

#[test]
fn a_click_selects_a_worker() {
    let p = fmt_xy(worker_screen());
    let Some(cli) = or_skip(
        "a_click_selects_a_worker",
        rts(&[
            "--frames",
            "5",
            "--inject-input",
            &format!("1:move:{p};2:lclick:{p}"),
        ]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("selected"), "1", "{cli}");
}

#[test]
fn script_coordinates_remain_logical() {
    // T8 routes *live* SDL mouse events through `DisplayViewport::map_pointer`
    // before they reach `RtsCommand`. Scripted `--inject-input` commands never
    // go near that path — `RtsScript` builds `RtsCommand`s straight from the
    // coordinates a script names, offscreen driver or not — so the same fixed
    // 1920x1080 logical coordinate this file has always clicked with must
    // still select the same worker after T8 lands.
    let p = fmt_xy(worker_screen());
    let Some(cli) = or_skip(
        "script_coordinates_remain_logical",
        rts(&[
            "--frames",
            "5",
            "--inject-input",
            &format!("1:move:{p};2:lclick:{p}"),
        ]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("selected"), "1", "{cli}");
}

#[test]
fn a_drag_selects_the_group() {
    let (a, b) = spawn_group_drag();
    let Some(cli) = or_skip(
        "a_drag_selects_the_group",
        rts(&[
            "--frames",
            "5",
            "--inject-input",
            &format!("1:drag:{a},{b}"),
        ]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("selected"), "6", "{cli}");
}

#[test]
fn a_right_click_moves_the_selection() {
    let (a, b) = spawn_group_drag();
    let dest = fmt_xy(far_move_target_screen());
    let script = format!("1:drag:{a},{b};5:rclick:{dest}");
    let Some(moved) = or_skip(
        "a_right_click_moves_the_selection",
        rts(&["--frames", "900", "--inject-input", &script]),
    ) else {
        return;
    };
    let Some(control) = or_skip(
        "a_right_click_moves_the_selection",
        rts(&["--frames", "900"]),
    ) else {
        return;
    };
    moved.assert_success();
    control.assert_success();
    assert_ne!(
        moved.final_hash(),
        control.final_hash(),
        "{moved}\n{control}\na move order did not change the state hash"
    );
}

#[test]
fn a_right_click_on_a_node_starts_gathering() {
    let (a, b) = spawn_group_drag();
    let node = fmt_xy(crystal_node_corner_screen());
    let script = format!("1:drag:{a},{b};5:rclick:{node}");
    let Some(cli) = or_skip(
        "a_right_click_on_a_node_starts_gathering",
        rts(&["--frames", "3000", "--inject-input", &script]),
    ) else {
        return;
    };
    cli.assert_success();
    let crystal: u32 = cli.exit_field("crystal").parse().expect("{cli}");
    assert!(
        crystal > 300,
        "{cli}\ncrystal did not rise above the starting 300"
    );
}

#[test]
fn arrow_keys_pan_the_camera() {
    let Some(panned) = or_skip(
        "arrow_keys_pan_the_camera",
        rts(&["--frames", "60", "--inject-input", "1:pan:right"]),
    ) else {
        return;
    };
    let Some(still) = or_skip("arrow_keys_pan_the_camera", rts(&["--frames", "60"])) else {
        return;
    };
    panned.assert_success();
    still.assert_success();
    assert_ne!(panned.final_hash(), still.final_hash(), "{panned}\n{still}");
}

/// Two 100-frame runs with the *same total* ticks of active rightward
/// panning (9) — one held from frame 1, one from frame 50 — must reach the
/// identical final camera centre, and so the identical state hash: nothing
/// else in either run ever moves. If key-up failed to zero `pan_dir`, the
/// first run would keep drifting for 91 more frames and the hashes would
/// diverge.
#[test]
fn pan_stops_on_key_up() {
    let Some(early) = or_skip(
        "pan_stops_on_key_up",
        rts(&[
            "--frames",
            "100",
            "--inject-input",
            "1:pan:right;10:panup:right",
        ]),
    ) else {
        return;
    };
    let Some(late) = or_skip(
        "pan_stops_on_key_up",
        rts(&[
            "--frames",
            "100",
            "--inject-input",
            "50:pan:right;59:panup:right",
        ]),
    ) else {
        return;
    };
    early.assert_success();
    late.assert_success();
    assert_eq!(
        early.final_hash(),
        late.final_hash(),
        "{early}\n{late}\npanning did not stop cleanly at key-up"
    );
}

#[test]
fn w_opens_the_depot_ghost() {
    let Some(cli) = or_skip(
        "w_opens_the_depot_ghost",
        rts(&["--frames", "5", "--inject-input", "1:key:f1;1:key:w"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert!(
        cli.stdout
            .lines()
            .any(|l| l.starts_with("rts: hud ") && l.contains("ghost=depot")),
        "{cli}\nno HUD line reports `ghost=depot`"
    );
}

#[test]
fn x_cancels_the_ghost() {
    let Some(cli) = or_skip(
        "x_cancels_the_ghost",
        rts(&[
            "--frames",
            "5",
            "--inject-input",
            "1:key:f1;1:key:w;3:key:x",
        ]),
    ) else {
        return;
    };
    cli.assert_success();
    let last_hud = cli
        .stdout
        .lines()
        .rfind(|l| l.starts_with("rts: hud "))
        .unwrap_or_else(|| panic!("{cli}\nno HUD lines"));
    assert!(
        last_hud.contains("ghost=none"),
        "{cli}\nlast HUD line does not report `ghost=none`: {last_hud}"
    );
}

#[test]
fn a_left_click_places_the_ghost() {
    let p = fmt_xy(clear_build_site_screen());
    let script = format!("1:key:w;2:move:{p};3:lclick:{p}");
    let Some(cli) = or_skip(
        "a_left_click_places_the_ghost",
        rts(&["--frames", "2500", "--inject-input", &script]),
    ) else {
        return;
    };
    cli.assert_success();
    let buildings: u32 = cli.exit_field("buildings").parse().expect("{cli}");
    assert_eq!(buildings, 2, "{cli}\nexpected the HQ plus one new Depot");
}

#[test]
fn a_right_click_cancels_the_ghost_without_ordering() {
    let p = fmt_xy(clear_build_site_screen());
    let cancel_script = format!("1:key:w;3:rclick:{p}");
    let x_script = "1:key:w;3:key:x";
    let Some(cancelled) = or_skip(
        "a_right_click_cancels_the_ghost_without_ordering",
        rts(&["--frames", "5", "--inject-input", &cancel_script]),
    ) else {
        return;
    };
    let Some(x_baseline) = or_skip(
        "a_right_click_cancels_the_ghost_without_ordering",
        rts(&["--frames", "5", "--inject-input", x_script]),
    ) else {
        return;
    };
    cancelled.assert_success();
    x_baseline.assert_success();
    assert_eq!(
        cancelled.exit_field("buildings"),
        "1",
        "{cancelled}\na right click while a ghost was pending must not have built anything"
    );
    assert_eq!(
        cancelled.final_hash(),
        x_baseline.final_hash(),
        "{cancelled}\n{x_baseline}\ncancelling via right click must match cancelling via `x`"
    );
}

#[test]
fn a_produces_a_worker_at_the_hq() {
    let p = fmt_xy(hq_click_screen());
    let script = format!("1:move:{p};2:lclick:{p};3:key:a");
    let Some(cli) = or_skip(
        "a_produces_a_worker_at_the_hq",
        rts(&["--frames", "400", "--inject-input", &script]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.exit_field("units"),
        "7",
        "{cli}\nexpected 6 + 1 workers"
    );
    assert_eq!(cli.exit_field("crystal"), "250", "{cli}\nexpected 300 - 50");
}

#[test]
fn the_exit_line_reports_every_counter() {
    let Some(cli) = or_skip(
        "the_exit_line_reports_every_counter",
        rts(&["--frames", "5"]),
    ) else {
        return;
    };
    cli.assert_success();
    let line = cli.exit_line();
    for key in [
        "crystal",
        "gas",
        "supply",
        "units",
        "buildings",
        "nodes",
        "selected",
    ] {
        cli.field(line, key);
    }
}

// ---------------------------------------------------------------------------
// 3b. Window/focus offscreen isolation (T10)
// ---------------------------------------------------------------------------

/// Hard isolation constraint (T10): an offscreen run must never build a
/// window adapter, so it can never print a window-claim/release line or
/// touch the window-mode/grab machinery `src/rts_window.rs` owns.
#[test]
fn offscreen_never_builds_window_adapter() {
    let Some(cli) = or_skip(
        "offscreen_never_builds_window_adapter",
        rts(&["--frames", "3"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("mode"), "offscreen", "{cli}");
    assert!(
        !cli.combined().contains("rts: window "),
        "{cli}\noffscreen run must never claim/build a real window"
    );
    assert!(
        !cli.combined().contains("released window"),
        "{cli}\noffscreen run has nothing to release"
    );
}

/// A `SDL_VIDEODRIVER=offscreen` run never opens a real window, so it can
/// never receive a `FocusGained`/`FocusLost` event — `paused` must stay
/// `false` and no focus/grab bookkeeping in `src/rts_window.rs` can fire.
/// Real focus-loss/gain behaviour on a live window is manual/platform
/// evidence only (ADR 018); this is the offscreen-isolation half of that
/// contract, which is the half a headless test host can prove.
#[test]
fn focus_state_never_diverges_from_default_offscreen() {
    let Some(cli) = or_skip(
        "focus_state_never_diverges_from_default_offscreen",
        rts(&["--frames", "5"]),
    ) else {
        return;
    };
    cli.assert_success();
    let line = cli.exit_line();
    assert_eq!(
        cli.field(line, "paused"),
        "false",
        "{cli}\nno real window exists offscreen, so a focus-loss pause request can never fire"
    );
    for needle in ["focus-gain", "focus-loss", "grab"] {
        assert!(
            !cli.combined().contains(needle),
            "{cli}\noffscreen run must never touch window-focus/grab machinery (`{needle}`)"
        );
    }
}

// ---------------------------------------------------------------------------
// 4. Settings isolation
// ---------------------------------------------------------------------------

/// Hard isolation constraint (T7): an offscreen run must never resolve,
/// read, or write the real per-user settings path. `SDL_GetPrefPath`
/// creates its directory as a side effect of being *called* at all
/// (`src/rts_settings.rs`'s module doc), so this proves the call itself
/// never happens under `SDL_VIDEODRIVER=offscreen` — not just that no file
/// happens to land in it.
#[test]
fn offscreen_settings_run_does_not_touch_settings() {
    let data_home = tmp_dir("offscreen_settings_run_does_not_touch_settings");
    let mut cmd = app_bin();
    cmd.args(["rts", "--frames", "3"]);
    cmd.env("SDL_VIDEODRIVER", "offscreen");
    // Both env vars SDL's `SDL_GetPrefPath` may consult on this platform,
    // pointed at one throwaway sentinel directory the assertion below owns.
    cmd.env("XDG_DATA_HOME", &data_home);
    cmd.env("HOME", &data_home);
    cmd.env_remove("MMD_RTS_FRAMES");
    cmd.env_remove("MMD_RTS_ONCE");
    let cli = run_to_completion(
        cmd,
        "XDG_DATA_HOME=<sentinel> HOME=<sentinel> rts --frames 3 (offscreen settings isolation)"
            .to_string(),
    );
    cli.assert_success();

    assert!(
        !cli.combined().contains("rts: settings warning="),
        "{cli}\noffscreen run must never look up settings, so it can never warn about them"
    );

    let entries: Vec<_> = std::fs::read_dir(&data_home)
        .expect("read sentinel pref dir")
        .collect();
    assert!(
        entries.is_empty(),
        "{cli}\noffscreen run created something under the sentinel pref dir: {entries:?}"
    );
}

// ---------------------------------------------------------------------------
// 5. Failure and CLI surface
// ---------------------------------------------------------------------------

/// `VK_DRIVER_FILES` pointed at a nonexistent ICD directory reliably yields
/// [`EXIT_NO_GPU`] on this host (verified directly: the process prints "GPU
/// device unavailable on this host" and exits 3). This is a stronger result
/// than a prior ticket's note about this env var on `run`; recorded here as
/// what was actually observed, not assumed.
#[test]
fn no_gpu_exits_with_code_three() {
    let mut cmd = app_bin();
    cmd.args(["rts", "--frames", "1"])
        .env("SDL_VIDEODRIVER", "offscreen")
        .env("VK_DRIVER_FILES", "/nonexistent");
    let cli = run_to_completion(
        cmd,
        "VK_DRIVER_FILES=/nonexistent rts --frames 1".to_string(),
    );
    assert_eq!(
        cli.code,
        Some(EXIT_NO_GPU),
        "{cli}\nexpected exit {EXIT_NO_GPU}; observed exit {:?} instead — a nonexistent \
         Vulkan ICD directory did not force the no-GPU path on this host",
        cli.code
    );
    cli.assert_says(&["GPU device unavailable on this host"]);
}

#[test]
fn usage_error_exits_with_code_two() {
    let cli = invoke(&["rts", "--nope"], true);
    assert_eq!(cli.code, Some(EXIT_USAGE), "{cli}");
    assert!(
        cli.combined().to_ascii_lowercase().contains("usage:"),
        "{cli}\nexpected a usage line"
    );
}

/// T31's shutdown-order bug (the window must be released from the device
/// before it drops) applies to this command's window path exactly as it did
/// to `run`'s. Only the clean-quit path is exercised here — the same
/// division of labour `tests/cli_contract.rs::quit_exits_clean_and_releases_window`
/// documents: the device-level claim/release contract is
/// `crates/mmd-engine/tests/render_correctness.rs`'s job, and a present
/// failure cannot be forced from this CLI, so this proves the release call
/// was reached, not that skipping it re-crashes this host.
#[test]
fn the_window_is_released_before_it_drops() {
    let Some(cli) = or_skip(
        "the_window_is_released_before_it_drops",
        invoke(
            &["rts", "--frames", "600", "--inject-input", "4:quit"],
            false,
        ),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("quit"), "true", "{cli}");
    assert_eq!(cli.exit_field("frames"), "3", "{cli}");
    match cli.exit_field("mode") {
        "window" => assert!(
            cli.stdout.contains("rts: released window"),
            "{cli}\nthe window was never released from the device before shutdown"
        ),
        "offscreen" => {
            assert!(
                cli.stderr.contains("window create failed")
                    || cli.stderr.contains("claim_window failed"),
                "{cli}\nfell back to offscreen without naming a reason"
            );
            eprintln!("SKIP: no window on this host");
        }
        other => panic!("{cli}\nunknown run mode `{other}`"),
    }
}

// ---------------------------------------------------------------------------
// 6. HUD routing and the minimap (T12)
// ---------------------------------------------------------------------------

/// A point inside the top-right settings gear — HUD chrome, no world
/// meaning.
fn gear_click_screen() -> [f32; 2] {
    [1888.0, 24.0]
}

/// The first (`BuildHq`) command-grid cell.
fn command_slot0_screen() -> [f32; 2] {
    [1706.0, 866.0]
}

/// A minimap point well off-centre, so a valid click visibly moves the
/// camera rather than landing near its own start position.
fn minimap_click_screen() -> [f32; 2] {
    [296.0, 960.0]
}

/// A gap in the bottom HUD panel: clear of the minimap, selection and
/// command cards.
fn hud_background_gap_screen() -> [f32; 2] {
    [410.0, 900.0]
}

#[test]
fn hud_command_grid_click_shares_the_keyboard_executor() {
    let p = fmt_xy(worker_screen());
    let slot = fmt_xy(command_slot0_screen());
    let mouse_script = format!("1:move:{p};2:lclick:{p};4:lclick:{slot}");
    let key_script = format!("1:move:{p};2:lclick:{p};4:key:q");

    let Some(mouse) = or_skip(
        "hud_command_grid_click_shares_the_keyboard_executor",
        rts(&["--frames", "10", "--inject-input", &mouse_script]),
    ) else {
        return;
    };
    let Some(key) = or_skip(
        "hud_command_grid_click_shares_the_keyboard_executor",
        rts(&["--frames", "10", "--inject-input", &key_script]),
    ) else {
        return;
    };
    mouse.assert_success();
    key.assert_success();
    assert_eq!(
        mouse.final_hash(),
        key.final_hash(),
        "{mouse}\n{key}\na command-grid click on BuildHq must match pressing `q`"
    );
}

#[test]
fn hud_disabled_command_slot_is_consumed_without_action() {
    // Default selection is empty, so every command_slots() entry is
    // disabled: a click on slot 0 must not touch world state at all.
    let slot = fmt_xy(command_slot0_screen());
    let script = format!("2:lclick:{slot}");
    let Some(clicked) = or_skip(
        "hud_disabled_command_slot_is_consumed_without_action",
        rts(&["--frames", "5", "--inject-input", &script]),
    ) else {
        return;
    };
    let Some(baseline) = or_skip(
        "hud_disabled_command_slot_is_consumed_without_action",
        rts(&["--frames", "5"]),
    ) else {
        return;
    };
    clicked.assert_success();
    baseline.assert_success();
    assert_eq!(
        clicked.final_hash(),
        baseline.final_hash(),
        "{clicked}\n{baseline}\na disabled command slot must never mutate world state"
    );
}

#[test]
fn hud_rally_arms_and_waits_for_the_next_world_click() {
    let hq = fmt_xy(hq_click_screen());
    let gear = fmt_xy(gear_click_screen());
    let target = fmt_xy(clear_build_site_screen());

    // Select the HQ, arm rally, detour through the gear/pause menu (`T13`:
    // the gear now actually opens it, pausing the sim for the frame it is
    // open — Escape closes it again before the world click lands), then
    // click a world cell — rally must still land there. The detour skips
    // exactly one tick (the paused frame the menu was open), so `via_gear`
    // gets one extra frame budget to reach the same tick count as `direct`.
    let via_gear =
        format!("1:move:{hq};2:lclick:{hq};3:key:r;4:lclick:{gear};5:key:esc;6:lclick:{target}");
    // Same, without the intervening HUD/menu detour.
    let direct = format!("1:move:{hq};2:lclick:{hq};3:key:r;5:lclick:{target}");
    // Arm rally, detour through the menu, but no world click ever arrives.
    let armed_only = format!("1:move:{hq};2:lclick:{hq};3:key:r;4:lclick:{gear};5:key:esc");

    let Some(via_gear) = or_skip(
        "hud_rally_arms_and_waits_for_the_next_world_click",
        rts(&["--frames", "11", "--inject-input", &via_gear]),
    ) else {
        return;
    };
    let Some(direct) = or_skip(
        "hud_rally_arms_and_waits_for_the_next_world_click",
        rts(&["--frames", "10", "--inject-input", &direct]),
    ) else {
        return;
    };
    let Some(armed_only) = or_skip(
        "hud_rally_arms_and_waits_for_the_next_world_click",
        rts(&["--frames", "11", "--inject-input", &armed_only]),
    ) else {
        return;
    };
    via_gear.assert_success();
    direct.assert_success();
    armed_only.assert_success();

    assert_eq!(
        via_gear.exit_field("tick"),
        direct.exit_field("tick"),
        "{via_gear}\n{direct}\nequal tick counts, or the hash compare below proves nothing"
    );
    assert_eq!(
        via_gear.final_hash(),
        direct.final_hash(),
        "{via_gear}\n{direct}\na gear/menu detour while rally is armed must not lose it"
    );
    assert_ne!(
        via_gear.final_hash(),
        armed_only.final_hash(),
        "{via_gear}\n{armed_only}\nrally must still be waiting until an actual world click arrives"
    );
}

#[test]
fn hud_minimap_click_recentres_the_camera() {
    let p = fmt_xy(minimap_click_screen());
    let script = format!("2:lclick:{p}");
    let Some(clicked) = or_skip(
        "hud_minimap_click_recentres_the_camera",
        rts(&["--frames", "5", "--inject-input", &script]),
    ) else {
        return;
    };
    let Some(baseline) = or_skip(
        "hud_minimap_click_recentres_the_camera",
        rts(&["--frames", "5"]),
    ) else {
        return;
    };
    clicked.assert_success();
    baseline.assert_success();
    assert_ne!(
        clicked.exit_field("camera"),
        baseline.exit_field("camera"),
        "{clicked}\n{baseline}\na valid minimap click must move the camera"
    );
    assert_eq!(
        clicked.exit_field("selected"),
        baseline.exit_field("selected"),
        "{clicked}\na minimap click must not also change the selection"
    );
}

#[test]
fn hud_background_click_never_reaches_the_world() {
    let p = fmt_xy(hud_background_gap_screen());
    let script = format!("2:lclick:{p};3:rclick:{p}");
    let Some(clicked) = or_skip(
        "hud_background_click_never_reaches_the_world",
        rts(&["--frames", "5", "--inject-input", &script]),
    ) else {
        return;
    };
    let Some(baseline) = or_skip(
        "hud_background_click_never_reaches_the_world",
        rts(&["--frames", "5"]),
    ) else {
        return;
    };
    clicked.assert_success();
    baseline.assert_success();
    assert_eq!(
        clicked.final_hash(),
        baseline.final_hash(),
        "{clicked}\n{baseline}\na click/right-click on a HUD background gap must never order the world"
    );
}

// ---------------------------------------------------------------------------
// 7. Paused menu / settings FSM (T13)
// ---------------------------------------------------------------------------

/// Inside the pause menu's own `Settings` button.
fn pause_menu_settings_screen() -> [f32; 2] {
    [960.0, 540.0]
}

/// Inside the settings panel's `Back` control.
fn settings_back_screen() -> [f32; 2] {
    [632.0, 928.0]
}

#[test]
fn menu_escape_opens_the_pause_menu_without_quitting() {
    let Some(cli) = or_skip(
        "menu_escape_opens_the_pause_menu_without_quitting",
        rts(&["--frames", "5", "--inject-input", "2:key:esc"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.exit_field("quit"),
        "false",
        "{cli}\nEscape must never quit — T13 repurposes it into the paused menu"
    );
    assert_eq!(
        cli.exit_field("paused"),
        "true",
        "{cli}\nopening the menu pauses the sim"
    );
    assert_eq!(
        cli.exit_field("frames"),
        "5",
        "{cli}\na paused run still renders every budgeted frame"
    );
}

#[test]
fn menu_escape_nesting_backs_out_to_gameplay() {
    // Escape opens the menu, a second Escape closes it again.
    let Some(cli) = or_skip(
        "menu_escape_nesting_backs_out_to_gameplay",
        rts(&["--frames", "5", "--inject-input", "2:key:esc;3:key:esc"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("quit"), "false", "{cli}");
    assert_eq!(
        cli.exit_field("paused"),
        "false",
        "{cli}\na closed menu with no manual pause must resume the sim"
    );
}

#[test]
fn menu_manual_pause_survives_the_menu() {
    // Space pauses manually; opening and closing the menu must not resume
    // the sim — the manual pause reason is independent of the menu one.
    let Some(cli) = or_skip(
        "menu_manual_pause_survives_the_menu",
        rts(&[
            "--frames",
            "6",
            "--inject-input",
            "2:key:space;3:key:esc;4:key:esc",
        ]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.exit_field("paused"),
        "true",
        "{cli}\nthe manual pause must survive an opened-then-closed menu"
    );
}

#[test]
fn menu_gear_click_opens_the_menu_exactly_like_escape() {
    let gear = fmt_xy(gear_click_screen());
    let script = format!("2:lclick:{gear}");
    let Some(cli) = or_skip(
        "menu_gear_click_opens_the_menu_exactly_like_escape",
        rts(&["--frames", "5", "--inject-input", &script]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("quit"), "false", "{cli}");
    assert_eq!(
        cli.exit_field("paused"),
        "true",
        "{cli}\nthe gear opens the paused menu"
    );
}

#[test]
fn menu_settings_button_navigates_and_back_returns() {
    // Open the menu, click Settings, click Back, then Escape closes the
    // (now top-level) menu — quit must never fire and the sim must resume.
    let settings_btn = fmt_xy(pause_menu_settings_screen());
    let back_btn = fmt_xy(settings_back_screen());
    let script = format!("2:key:esc;3:lclick:{settings_btn};4:lclick:{back_btn};5:key:esc");
    let Some(cli) = or_skip(
        "menu_settings_button_navigates_and_back_returns",
        rts(&["--frames", "6", "--inject-input", &script]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("quit"), "false", "{cli}");
    assert_eq!(
        cli.exit_field("paused"),
        "false",
        "{cli}\nSettings -> Back -> Escape must fully close the menu"
    );
}

#[test]
fn menu_modal_consumes_clicks_outside_its_own_controls() {
    // While the menu is open, a click far outside the pause menu's own
    // rect (e.g. dead centre of the HUD's command panel) must not select,
    // place, or otherwise touch the world — it is consumed by the modal.
    let hq = fmt_xy(hq_click_screen());
    let outside = fmt_xy([50.0, 50.0]);
    let with_menu = format!("1:move:{hq};2:lclick:{hq};3:key:esc;4:lclick:{outside}");
    let no_menu_click = format!("1:move:{hq};2:lclick:{hq};3:key:esc");

    let Some(with_menu) = or_skip(
        "menu_modal_consumes_clicks_outside_its_own_controls",
        rts(&["--frames", "5", "--inject-input", &with_menu]),
    ) else {
        return;
    };
    let Some(no_menu_click) = or_skip(
        "menu_modal_consumes_clicks_outside_its_own_controls",
        rts(&["--frames", "5", "--inject-input", &no_menu_click]),
    ) else {
        return;
    };
    with_menu.assert_success();
    no_menu_click.assert_success();
    assert_eq!(
        with_menu.exit_field("selected"),
        no_menu_click.exit_field("selected"),
        "{with_menu}\n{no_menu_click}\na click outside the modal's own rect must not reach \
         selection"
    );
}

// ---------------------------------------------------------------------------
// 8. Deterministic audio events and buses (T15)
// ---------------------------------------------------------------------------

impl Cli {
    /// The `rts: audio ...` line — the semantic audio trace's counters.
    fn audio_line(&self) -> &str {
        self.stdout
            .lines()
            .find(|l| l.starts_with("rts: audio "))
            .unwrap_or_else(|| panic!("{self}\nno `rts: audio` line"))
    }

    fn audio_field(&self, key: &str) -> u32 {
        let raw = self.field(self.audio_line(), key);
        raw.parse()
            .unwrap_or_else(|_| panic!("{self}\n`{key}={raw}` is not a count"))
    }
}

/// One `StartMusic` per session, exact default gains, and silence for a run
/// that never acts — the whole no-input baseline.
#[test]
fn audio_events_start_music_once_with_exact_default_gains() {
    let Some(cli) = or_skip(
        "audio_events_start_music_once_with_exact_default_gains",
        rts(&["--frames", "10"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.stdout
            .lines()
            .filter(|l| l.starts_with("rts: audio "))
            .count(),
        1,
        "{cli}\nexactly one audio line per run"
    );
    assert_eq!(cli.audio_field("music"), 1, "{cli}\nmusic starts once");
    assert_eq!(cli.audio_field("voice"), 0, "{cli}");
    assert_eq!(cli.audio_field("cues"), 0, "{cli}");
    assert_eq!(cli.audio_field("reject"), 0, "{cli}");
    assert_eq!(cli.audio_field("ui"), 0, "{cli}");
    assert_eq!(
        cli.field(cli.audio_line(), "gains"),
        "2800/5600/4800",
        "{cli}\ndefault 80/35/70/60 must resolve to exact basis points"
    );
}

/// The menu and a focus-losing pause never restart or stop music.
#[test]
fn audio_events_music_survives_the_pause_menu() {
    let Some(cli) = or_skip(
        "audio_events_music_survives_the_pause_menu",
        rts(&[
            "--frames",
            "8",
            "--inject-input",
            "2:key:esc;4:key:space;6:key:esc",
        ]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.audio_field("music"),
        1,
        "{cli}\nstill exactly one start"
    );
}

/// A selection voices only the units it newly selected; re-selecting the
/// same units again is silence.
#[test]
fn audio_events_voice_new_selection_only_once() {
    let (a, b) = spawn_group_drag();
    let script = format!("1:drag:{a},{b};4:drag:{a},{b}");
    let Some(cli) = or_skip(
        "audio_events_voice_new_selection_only_once",
        rts(&["--frames", "8", "--inject-input", &script]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("selected"), "6", "{cli}");
    assert_eq!(
        cli.audio_field("voice"),
        1,
        "{cli}\nthe second, identical drag selects nothing new"
    );
    assert_eq!(
        cli.audio_field("cues"),
        6,
        "{cli}\nsix newly selected workers, under the eight-cue cap"
    );
    assert_eq!(cli.audio_field("reject"), 0, "{cli}");
}

/// An accepted context order voices one capped batch and no reject; the
/// same click with nothing selected is fully silent.
#[test]
fn audio_events_order_batch_is_capped_and_rejectless() {
    let (a, b) = spawn_group_drag();
    let dest = fmt_xy(far_move_target_screen());
    let script = format!("1:drag:{a},{b};4:rclick:{dest}");
    let Some(ordered) = or_skip(
        "audio_events_order_batch_is_capped_and_rejectless",
        rts(&["--frames", "8", "--inject-input", &script]),
    ) else {
        return;
    };
    let Some(unselected) = or_skip(
        "audio_events_order_batch_is_capped_and_rejectless",
        rts(&[
            "--frames",
            "8",
            "--inject-input",
            &format!("4:rclick:{dest}"),
        ]),
    ) else {
        return;
    };
    ordered.assert_success();
    unselected.assert_success();

    assert_eq!(
        ordered.audio_field("voice"),
        2,
        "{ordered}\none selection batch plus one order batch"
    );
    assert_eq!(
        ordered.audio_field("cues"),
        12,
        "{ordered}\nsix selected plus six ordered, both under the cap"
    );
    assert_eq!(
        ordered.audio_field("reject"),
        0,
        "{ordered}\nevery selected worker accepted the move"
    );

    assert_eq!(
        unselected.audio_field("voice"),
        0,
        "{unselected}\nan empty selection voices nothing"
    );
    assert_eq!(
        unselected.audio_field("reject"),
        0,
        "{unselected}\nan empty selection is not a rejection"
    );
}

/// An enabled command card is one UI SFX; the same command from its
/// keyboard hotkey is silent, and a disabled card is silent too.
#[test]
fn audio_events_only_pointer_ui_actions_click() {
    let p = fmt_xy(worker_screen());
    let slot = fmt_xy(command_slot0_screen());
    let card = format!("1:move:{p};2:lclick:{p};4:lclick:{slot}");
    let hotkey = format!("1:move:{p};2:lclick:{p};4:key:q");
    let disabled = format!("4:lclick:{slot}");

    let Some(card) = or_skip(
        "audio_events_only_pointer_ui_actions_click",
        rts(&["--frames", "8", "--inject-input", &card]),
    ) else {
        return;
    };
    let Some(hotkey) = or_skip(
        "audio_events_only_pointer_ui_actions_click",
        rts(&["--frames", "8", "--inject-input", &hotkey]),
    ) else {
        return;
    };
    let Some(disabled) = or_skip(
        "audio_events_only_pointer_ui_actions_click",
        rts(&["--frames", "8", "--inject-input", &disabled]),
    ) else {
        return;
    };
    card.assert_success();
    hotkey.assert_success();
    disabled.assert_success();

    assert_eq!(
        card.audio_field("ui"),
        1,
        "{card}\nan enabled command card clicks exactly once"
    );
    assert_eq!(
        hotkey.audio_field("ui"),
        0,
        "{hotkey}\na keyboard hotkey never makes a pointer-click sound"
    );
    assert_eq!(
        disabled.audio_field("ui"),
        0,
        "{disabled}\na disabled card is consumed silently"
    );
    // The two paths still drive the same world (T12's shared executor).
    assert_eq!(card.final_hash(), hotkey.final_hash(), "{card}\n{hotkey}");
}

/// The gear and a valid minimap recentre each click once; a minimap click
/// outside the map diamond is consumed silently.
#[test]
fn audio_events_map_gear_and_minimap_sources() {
    let gear = fmt_xy(gear_click_screen());
    let minimap = fmt_xy(minimap_click_screen());
    // One pixel inside the minimap's map box (`MINIMAP_MAP` starts at
    // `[32, 872]`), which is a *corner* of that box and so outside the map
    // diamond drawn in it.
    let corner = fmt_xy([33.0, 873.0]);
    let script = format!("2:lclick:{minimap};3:lclick:{corner};4:lclick:{gear}");
    let Some(cli) = or_skip(
        "audio_events_map_gear_and_minimap_sources",
        rts(&["--frames", "8", "--inject-input", &script]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.audio_field("ui"),
        2,
        "{cli}\nminimap recentre + gear, but not the off-diamond corner"
    );
    assert_eq!(
        cli.exit_field("paused"),
        "true",
        "{cli}\nthe gear opened the menu"
    );
}

/// Audio derivation is a read-only observer of the shared action path: the
/// same scripted actions must leave exactly the same world state hash as a
/// run whose events go nowhere. Proven here against the *emitting* run's own
/// exit hash, which the unit test `audio_events_do_not_change_world_hash`
/// pins against a null sink in-process.
#[test]
fn audio_events_keep_the_world_hash_stable() {
    let (a, b) = spawn_group_drag();
    let dest = fmt_xy(far_move_target_screen());
    let script = format!("1:drag:{a},{b};4:rclick:{dest}");
    let Some(first) = or_skip(
        "audio_events_keep_the_world_hash_stable",
        rts(&["--frames", "60", "--inject-input", &script]),
    ) else {
        return;
    };
    let Some(second) = or_skip(
        "audio_events_keep_the_world_hash_stable",
        rts(&["--frames", "60", "--inject-input", &script]),
    ) else {
        return;
    };
    first.assert_success();
    second.assert_success();
    assert_eq!(
        first.final_hash(),
        second.final_hash(),
        "{first}\n{second}\nemitting audio must stay deterministic"
    );
    assert_eq!(
        first.audio_line(),
        second.audio_line(),
        "{first}\n{second}\nthe same script must hear exactly the same events"
    );
}
