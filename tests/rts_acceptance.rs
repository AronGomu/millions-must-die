//! T15: the shipped binary, driven by the tracked acceptance script, reaches
//! the same phase-1 milestones the engine-level test asserts by world state.
//!
//! `crates/mmd-engine/tests/rts_acceptance.rs` owns the *meaning* of the run —
//! which system produced which milestone. This file owns the separate fact
//! that `millions_must_die rts --inject-input-file <the tracked script>`
//! reproduces it end to end, through clap, the file parser, the input layer,
//! the renderer and the exit line. A failure here and a failure there mean
//! different things, which is why both exist.
//!
//! Same discipline as `tests/rts_cli_contract.rs`: the real binary as a
//! subprocess, `SDL_VIDEODRIVER=offscreen`, and every spawn serialized behind
//! one lock so several thousand-frame GPU runs cannot overlap on one device.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// The tracked script the merge gate runs, relative to the crate root.
const SCRIPT: &str = "assets/scenarios/rts_acceptance_v1.script";
/// Frame budget the merge gate gives it. The script quits before this.
const FRAMES: &str = "1600";

/// Serializes every subprocess this file spawns against the real GPU — the
/// same reason `tests/rts_cli_contract.rs` does it: enough thousand-frame runs
/// overlapping on one physical device trips `VK_ERROR_DEVICE_LOST`.
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

    fn exit_field(&self, key: &str) -> &str {
        let line = self.exit_line();
        line.split_whitespace()
            .find_map(|tok| tok.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("{self}\nexit line has no `{key}=`"))
    }

    fn exit_u32(&self, key: &str) -> u32 {
        let raw = self.exit_field(key);
        raw.parse()
            .unwrap_or_else(|e| panic!("{self}\n`{key}={raw}` is not a number: {e}"))
    }

    /// A `key=<x>,<y>` field of the exit line, parsed as a pair of floats.
    fn exit_pair(&self, key: &str) -> [f32; 2] {
        let raw = self.exit_field(key);
        let mut parts = raw.split(',').map(|n| {
            n.parse::<f32>()
                .unwrap_or_else(|e| panic!("{self}\n`{key}={raw}` is not a pair of numbers: {e}"))
        });
        let (x, y) = (parts.next(), parts.next());
        match (x, y) {
            (Some(x), Some(y)) => [x, y],
            _ => panic!("{self}\n`{key}={raw}` is not a pair"),
        }
    }

    fn final_hash(&self) -> String {
        let hash = self.exit_field("hash").to_string();
        assert_eq!(hash.len(), 64, "{self}\nstate hash is not 32 bytes of hex");
        hash
    }

    /// The `T17` joined observation of one run, in exit-line field order.
    ///
    /// Read as one struct rather than field by field so
    /// `phase1_1_run_is_cross_process_deterministic` compares *all* of it
    /// between two processes — a counter added later is compared for free,
    /// instead of being silently unproven.
    fn phase1_1(&self) -> Phase11 {
        Phase11 {
            hash: self.final_hash(),
            tick: self.exit_u32("tick"),
            frames: self.exit_u32("frames"),
            body_overlaps: self.exit_u32("body_overlaps"),
            ui_page: self.exit_field("ui_page").to_string(),
            music_starts: self.exit_u32("music_starts"),
            voice_select: self.exit_u32("voice_select"),
            voice_order: self.exit_u32("voice_order"),
            voice_reject: self.exit_u32("voice_reject"),
            sfx_ui: self.exit_u32("sfx_ui"),
            keyboard_pan: self.exit_u32("keyboard_pan"),
            camera: self.exit_pair("camera"),
        }
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
        for crash in ["panicked at", "stack backtrace"] {
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
}

/// Everything `T17` added to the exit line, plus the identity fields a
/// cross-process comparison needs.
#[derive(Debug, PartialEq)]
struct Phase11 {
    hash: String,
    tick: u32,
    frames: u32,
    body_overlaps: u32,
    ui_page: String,
    music_starts: u32,
    voice_select: u32,
    voice_order: u32,
    voice_reject: u32,
    sfx_ui: u32,
    keyboard_pan: u32,
    camera: [f32; 2],
}

impl std::fmt::Display for Cli {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "--- millions_must_die {}\n--- exit {:?}\n--- stdout (tail)\n{}--- stderr\n{}---",
            self.args.join(" "),
            self.code,
            tail(&self.stdout),
            self.stderr
        )
    }
}

/// The last few stdout lines. The acceptance run prints one HUD line per frame
/// for its final stretch, and a full dump would bury the exit line.
fn tail(out: &str) -> String {
    let lines: Vec<&str> = out.lines().collect();
    let start = lines.len().saturating_sub(12);
    lines[start..]
        .iter()
        .map(|l| format!("{l}\n"))
        .collect::<String>()
}

fn app_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_millions_must_die"))
}

/// Invoke `rts` with `extra` args, forcing SDL's offscreen video driver so a
/// test never opens a window on the developer's desktop.
fn rts(extra: &[&str]) -> Cli {
    let mut args = vec!["rts"];
    args.extend_from_slice(extra);
    let mut cmd = app_bin();
    cmd.args(&args);
    cmd.env("SDL_VIDEODRIVER", "offscreen");
    // The app also reads these; a developer's shell must not steer a test.
    cmd.env_remove("MMD_RTS_FRAMES");
    cmd.env_remove("MMD_RTS_ONCE");
    run_to_completion(cmd, args.join(" "))
}

/// The tracked acceptance run itself — the exact command the merge gate lists.
fn acceptance_run() -> Cli {
    rts(&["--frames", FRAMES, "--inject-input-file", SCRIPT])
}

/// One acceptance run, shared by every case that only reads its exit line.
///
/// Deliberately shared rather than one process per case. `cargo test
/// --workspace` runs test *binaries* in parallel, and this file's lock only
/// serializes its own; eight independent thousand-frame runs of the real GPU
/// alongside `rts_cli_contract`, `gpu_smoke`, `gpu_golden` and
/// `render_correctness` reproduced `VK_ERROR_DEVICE_LOST` on this host. The
/// assertions below are unchanged — each still reads a real run's real exit
/// line — and `the_acceptance_run_is_deterministic` still spends a second
/// process, which is what keeps "the same script gives the same hash" a
/// cross-process claim rather than a cached one.
///
/// `None` means this host has no GPU device, exactly as [`or_skip`] decides.
fn shared_acceptance_run(case: &str) -> Option<&'static Cli> {
    static RUN: OnceLock<Option<Cli>> = OnceLock::new();
    RUN.get_or_init(|| or_skip("the tracked acceptance run", acceptance_run()))
        .as_ref()
        .or_else(|| {
            eprintln!("SKIP {case}: no GPU device on this host");
            None
        })
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

    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
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
// The acceptance run
// ---------------------------------------------------------------------------

#[test]
fn the_tracked_script_runs_clean() {
    let Some(cli) = shared_acceptance_run("the_tracked_script_runs_clean") else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.stdout
            .lines()
            .filter(|l| l.starts_with("rts: clean exit"))
            .count(),
        1,
        "{cli}\nexpected exactly one clean-exit line"
    );
    assert_eq!(
        cli.exit_field("quit"),
        "true",
        "{cli}\nthe script's `esc` never landed"
    );
}

#[test]
fn the_acceptance_run_builds_two_buildings() {
    let Some(cli) = shared_acceptance_run("the_acceptance_run_builds_two_buildings") else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.exit_u32("buildings"),
        3,
        "{cli}\nexpected the starting HQ plus a finished Depot and Barracks"
    );
}

#[test]
fn the_acceptance_run_produces_a_soldier() {
    let Some(cli) = shared_acceptance_run("the_acceptance_run_produces_a_soldier") else {
        return;
    };
    cli.assert_success();
    let units = cli.exit_u32("units");
    assert!(
        units >= 8,
        "{cli}\n{units} units: the six starting workers, the HQ's Worker and the \
         Barracks' Soldier are eight"
    );
    // A Soldier costs 2 supply and a Worker 1, so seven workers alone would
    // read 7 — the count is what makes `units` above a Soldier rather than a
    // second Worker.
    let supply = cli.exit_field("supply");
    let used: u32 = supply
        .split('/')
        .next()
        .and_then(|u| u.parse().ok())
        .unwrap_or_else(|| panic!("{cli}\n`supply={supply}` is not used/cap"));
    assert!(
        used > units,
        "{cli}\nsupply used {used} for {units} units — nothing here costs the two \
         supply a Soldier does"
    );
}

#[test]
fn the_acceptance_run_earns_crystal() {
    let Some(cli) = shared_acceptance_run("the_acceptance_run_earns_crystal") else {
        return;
    };
    cli.assert_success();
    // The scene starts with 300 crystal. This run buys a Depot (100), a Worker
    // (50), a Barracks (150) and a Soldier (50) — 350, more than it started
    // with. Both purchases landing *and* a positive balance is only possible if
    // the gather loop banked something.
    assert_eq!(
        cli.exit_u32("buildings"),
        3,
        "{cli}\nthe run did not buy both buildings, so its balance proves no income"
    );
    assert!(
        cli.exit_u32("units") >= 8,
        "{cli}\nthe run did not buy both units, so its balance proves no income"
    );
    assert!(
        cli.exit_u32("crystal") > 0,
        "{cli}\ncrystal is 0 after spending 350 of a starting 300 — that cannot happen"
    );
}

/// The scene starts with 100 gas and this run spends 25 on the Barracks and
/// 25 on the Soldier. Ending above 50 is therefore only possible if a worker
/// actually worked a gas node: an engine where gas gathering is broken lands
/// exactly on 50.
#[test]
fn the_acceptance_run_earns_gas() {
    let Some(cli) = shared_acceptance_run("the_acceptance_run_earns_gas") else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.exit_u32("buildings"),
        3,
        "{cli}\nthe run did not buy the Barracks, so its gas balance proves no income"
    );
    assert!(
        cli.exit_u32("units") >= 8,
        "{cli}\nthe run did not buy the Soldier, so its gas balance proves no income"
    );
    let gas = cli.exit_u32("gas");
    assert!(
        gas > 50,
        "{cli}\ngas is {gas} after spending 50 of a starting 100 — nothing was \
         ever gathered from a gas node"
    );
}

/// The script holds an arrow key for 300 frames. The camera must be somewhere
/// else at the end of the run than the base it opened on.
#[test]
fn the_acceptance_run_pans_the_camera() {
    let Some(cli) = shared_acceptance_run("the_acceptance_run_pans_the_camera") else {
        return;
    };
    cli.assert_success();
    let camera = cli.exit_pair("camera");
    // The scene opens centred on the HQ's footprint centre, (166, 166), and
    // the script's held `right` is a *screen*-space direction: on this
    // projection it moves the centre toward a larger `cell.x - cell.y`.
    assert!(
        camera[0] > 166.0 && camera[1] < 166.0,
        "{cli}\nthe camera centre is {camera:?}; the run opened on (166, 166) and \
         held the right arrow for 300 frames without moving"
    );
}

#[test]
fn the_acceptance_run_raises_the_supply_cap() {
    let Some(cli) = shared_acceptance_run("the_acceptance_run_raises_the_supply_cap") else {
        return;
    };
    cli.assert_success();
    let supply = cli.exit_field("supply");
    let cap = supply
        .split('/')
        .nth(1)
        .unwrap_or_else(|| panic!("{cli}\n`supply={supply}` is not used/cap"));
    assert_eq!(
        cap, "20",
        "{cli}\nsupply cap {cap}: the starting HQ grants 10 and the Depot this run \
         finishes grants another 10"
    );
}

#[test]
fn the_acceptance_run_is_deterministic() {
    let Some(a) = shared_acceptance_run("the_acceptance_run_is_deterministic") else {
        return;
    };
    // A second, independent process: a shared run compared with itself would
    // prove only that a `String` equals itself.
    let Some(b) = or_skip("the_acceptance_run_is_deterministic", acceptance_run()) else {
        return;
    };
    a.assert_success();
    b.assert_success();
    assert_eq!(
        a.final_hash(),
        b.final_hash(),
        "{a}\n{b}\nthe acceptance run is not reproducible"
    );
}

// ---------------------------------------------------------------------------
// T17: the joined phase-1.1 claim
// ---------------------------------------------------------------------------

/// Hard bodies, end to end: the shipped binary reports its own
/// `RtsWorld::body_overlap_count` at exit, and a run that drives selection,
/// group orders, construction and production through the real input path
/// must never leave a single penetrating pair behind.
#[test]
fn acceptance_never_has_body_penetration() {
    let Some(cli) = shared_acceptance_run("acceptance_never_has_body_penetration") else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.exit_u32("body_overlaps"),
        0,
        "{cli}\nlive unit bodies penetrate each other at the end of the run"
    );
}

/// Every build and produce command in the tracked script is a command-card
/// click, not a hotkey. The card only enables a slot for a selection it
/// actually fits, so four accepted clicks (`sfx_ui` counts them) plus the
/// finished Depot, Barracks, Worker and Soldier is the proof that the
/// pointer path — selection, hit test, enable check, execute — works.
#[test]
fn acceptance_uses_command_card_for_build_and_produce() {
    let Some(cli) = shared_acceptance_run("acceptance_uses_command_card_for_build_and_produce")
    else {
        return;
    };
    cli.assert_success();

    let script = std::fs::read_to_string(script_path()).expect("read the tracked script");
    // Whole tokens, not a substring search: `key:esc` contains `key:e`.
    for line in script.lines() {
        for entry in line.split('#').next().unwrap_or("").split(';') {
            let mut parts = entry.trim().split(':');
            let (_frame, kind, arg) = (parts.next(), parts.next(), parts.next());
            if kind == Some("key") && matches!(arg, Some("w" | "e" | "a" | "s")) {
                panic!(
                    "the tracked script still uses the `key:{}` build/produce hotkey; \
                     phase 1.1 must go through the command card",
                    arg.unwrap_or_default()
                );
            }
        }
    }
    for slot in ["1728,888", "1800,888", "1872,888"] {
        assert!(
            script.contains(slot),
            "the tracked script never clicks command-grid centre {slot}"
        );
    }

    assert_eq!(
        cli.exit_u32("buildings"),
        3,
        "{cli}\nthe card's Depot and Barracks slots did not finish both buildings"
    );
    assert!(
        cli.exit_u32("units") >= 8,
        "{cli}\nthe card's Worker and Soldier slots did not produce both units"
    );
}

/// Gear -> SETTINGS -> a slider -> Escape -> Escape. The menu pauses the sim
/// while it is open, the settings edit lands (78 cells/s, a legal step), and
/// closing it puts the run back on `gameplay` with the simulation ticking
/// again.
#[test]
fn acceptance_menu_settings_round_trip_resumes() {
    let Some(cli) = shared_acceptance_run("acceptance_menu_settings_round_trip_resumes") else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.exit_field("ui_page"),
        "gameplay",
        "{cli}\nthe run ended inside a menu; both Escapes must back all the way out"
    );
    assert_eq!(
        cli.exit_field("paused"),
        "false",
        "{cli}\nthe sim is still paused after the menu closed"
    );
    assert_eq!(
        cli.exit_u32("keyboard_pan"),
        78,
        "{cli}\nthe settings slider click did not land on the 78 cells/s step"
    );
    // The menu really paused: the run rendered strictly more frames than it
    // stepped ticks, and every one of those missing ticks is a menu frame.
    let (frames, tick) = (cli.exit_u32("frames"), cli.exit_u32("tick"));
    assert!(
        tick < frames,
        "{cli}\ntick {tick} of {frames} frames: the menu never paused the sim"
    );
    assert!(
        tick > 0,
        "{cli}\nthe sim never advanced at all, so 'it resumed' proves nothing"
    );
}

/// The exact audio counters the tracked script owes, cue by cue.
///
/// These are hard equalities on purpose: an inequality would still pass when
/// a whole class of feedback silently stopped being derived.
#[test]
fn acceptance_audio_counts_are_exact() {
    let Some(cli) = shared_acceptance_run("acceptance_audio_counts_are_exact") else {
        return;
    };
    cli.assert_success();
    // One music start per session; nothing (menu, pause) ever restarts it.
    assert_eq!(cli.exit_u32("music_starts"), 1, "{cli}");
    // The six boxed starting workers plus the two later newly selected ones.
    // A re-select is silence, which is why narrowing the box to one of its
    // own members does not count.
    assert_eq!(cli.exit_u32("voice_select"), 8, "{cli}");
    // Six crystal gathers, one gas gather, two build receipts.
    assert_eq!(cli.exit_u32("voice_order"), 9, "{cli}");
    // The one deliberate off-map order.
    assert_eq!(cli.exit_u32("voice_reject"), 1, "{cli}");
    // Four command-card clicks, the minimap, the gear, SETTINGS, the slider.
    assert_eq!(cli.exit_u32("sfx_ui"), 8, "{cli}");

    // The `rts: audio` line, pinned the same way.
    //
    // This used to compare those fields against the exit-line fields above and
    // call it a "sink-side cross-check". It was neither: both lines are
    // printed from the one `session.audio_counters`, so the comparison was a
    // tally checked against itself and would have passed with every number
    // wrong together. The sink's *own* tally (`FakeAudioSink::counters`) is
    // never printed, so there is no second observer to compare with from out
    // here. Hard equalities against the script's own owed numbers are what a
    // CLI-level test can actually assert — and they are strictly stronger than
    // the tautology they replace.
    let audio = cli
        .stdout
        .lines()
        .find(|l| l.starts_with("rts: audio "))
        .unwrap_or_else(|| panic!("{cli}\nno `rts: audio` line"));
    let field = |key: &str| -> u32 {
        audio
            .split_whitespace()
            .find_map(|tok| tok.strip_prefix(&format!("{key}=")))
            .and_then(|v| v.parse().ok())
            .unwrap_or_else(|| panic!("{cli}\n`rts: audio` has no numeric `{key}=`"))
    };
    assert_eq!(field("music"), 1, "{cli}");
    assert_eq!(field("reject"), 1, "{cli}");
    assert_eq!(field("ui"), 8, "{cli}");
    // Voice *batches*: one per action that voiced anything, which is fewer
    // than the cues inside them.
    assert_eq!(field("voice"), 7, "{cli}");
    // Every individual unit cue: the 8 selects plus the 9 accepted orders.
    assert_eq!(field("cues"), 17, "{cli}");
    assert_eq!(
        field("cues"),
        cli.exit_u32("voice_select") + cli.exit_u32("voice_order"),
        "{cli}\nthe split select/order counters do not add up to the cue total"
    );
}

/// Two independent processes, same script: identical state hash *and*
/// identical joined counters.
///
/// The hash alone would not cover the audio/UI/body observation, which is
/// derived outside the world state on purpose (ADR 020) — so a run could be
/// hash-stable while its feedback drifted.
#[test]
fn phase1_1_run_is_cross_process_deterministic() {
    let Some(a) = shared_acceptance_run("phase1_1_run_is_cross_process_deterministic") else {
        return;
    };
    let Some(b) = or_skip(
        "phase1_1_run_is_cross_process_deterministic",
        acceptance_run(),
    ) else {
        return;
    };
    a.assert_success();
    b.assert_success();
    assert_eq!(
        a.phase1_1(),
        b.phase1_1(),
        "{a}\n{b}\ntwo processes running the same script disagreed"
    );
}

/// The tracked script, resolved against the crate root.
fn script_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(SCRIPT)
}

/// A scripted entry that never fires makes the run exit 1, so a clean exit is
/// itself the proof that every line of the tracked script landed.
#[test]
fn the_acceptance_run_fires_every_entry() {
    let Some(cli) = shared_acceptance_run("the_acceptance_run_fires_every_entry") else {
        return;
    };
    cli.assert_success();
    assert!(
        !cli.combined().contains("never fired"),
        "{cli}\nan entry of the tracked script never fired"
    );

    // Anti-vacuity: the same script under a budget that ends before its last
    // entry must fail, or the clean exit above proves nothing.
    let Some(short) = or_skip(
        "the_acceptance_run_fires_every_entry",
        rts(&["--frames", "30", "--inject-input-file", SCRIPT]),
    ) else {
        return;
    };
    short
        .assert_actionable_failure()
        .assert_says(&["never fired"]);
}

// ---------------------------------------------------------------------------
// The flag itself
// ---------------------------------------------------------------------------

#[test]
fn inject_input_and_file_are_mutually_exclusive() {
    let cli = rts(&[
        "--frames",
        "3",
        "--inject-input",
        "1:key:esc",
        "--inject-input-file",
        SCRIPT,
    ]);
    assert_eq!(
        cli.code,
        Some(EXIT_USAGE),
        "{cli}\ngiving both script flags must be a usage error, not a silent \
         precedence rule"
    );
}

#[test]
fn a_missing_script_file_is_actionable() {
    let cli = rts(&[
        "--frames",
        "3",
        "--inject-input-file",
        "/nope/missing.script",
    ]);
    cli.assert_actionable_failure()
        .assert_says(&["--inject-input-file", "/nope/missing.script"]);
}

/// A commented, multi-line script and the one-line `--inject-input` spec it is
/// equivalent to must produce the identical run.
#[test]
fn comments_and_blank_lines_are_ignored() {
    let spec = "1:move:960,540;4:key:f1;8:key:space;12:key:esc";
    let commented = "\
# the same run, written for a human
1:move:960,540      # look at the HQ

# pause it, then quit
4:key:f1
8:key:space
12:key:esc          # trailing comment
";
    let path = tmp_dir("commented_script").join("equivalent.script");
    std::fs::write(&path, commented).expect("write temp script");

    let Some(from_flag) = or_skip(
        "comments_and_blank_lines_are_ignored",
        rts(&["--frames", "20", "--inject-input", spec]),
    ) else {
        return;
    };
    let Some(from_file) = or_skip(
        "comments_and_blank_lines_are_ignored",
        rts(&[
            "--frames",
            "20",
            "--inject-input-file",
            path.to_str().expect("utf-8 temp path"),
        ]),
    ) else {
        return;
    };
    from_flag.assert_success();
    from_file.assert_success();
    assert_eq!(
        from_flag.final_hash(),
        from_file.final_hash(),
        "{from_flag}\n{from_file}\ncomments or newlines changed what the script did"
    );
}
