//! T32: the playable surface. `run` starts, ticks, pauses, overlays, quits
//! cleanly, and fails loudly on bad input.
//!
//! Everything here drives the **real binary** as a subprocess. That is the
//! point: `crates/mmd-engine/tests/runtime_frame.rs` already proves
//! [`Runtime`] pauses when told to, but only a process run proves the app
//! *wires* the flag, honours the frame budget, prints the documented contract,
//! releases the GPU window, and reports a failure as an actionable message
//! instead of a panic.
//!
//! # The stdout contract under test
//!
//! ```text
//! run: backend=<b> adapter=<a> view=<w>x<h> agents=<n> scenario=<path> (engine <v>)
//! run: frame0 tick=<t> hash=<64 hex> groups=[<n>,..] sim=<f>ms upload=<f>ms
//! run: offscreen draw ok (backend=<b>)
//! run: window <w>x<h> claimed; ...                      (windowed runs only)
//! <overlay HUD lines>                                   (while overlay is on)
//! run: released window                                  (windowed runs only)
//! run: clean exit mode=<offscreen|window> backend=<b> tick=<t> frames=<n> \
//!      hash=<64 hex> quit=<bool> paused=<bool> overlay=<bool>
//! ```
//!
//! Exit codes: `0` success, `1` actionable failure, `2` clap usage, `3` this
//! host has no GPU device.
//!
//! # Timing
//!
//! No assertion in this file reads a duration. Phase-0 acceptance is
//! behavioural (`docs/05-testing.md`); the `sim=`/`upload=` fields exist for a
//! human reading the HUD and are deliberately never asserted on.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// Exit code for a failure the user can act on.
const EXIT_ERROR: i32 = 1;

/// Exit code clap uses for a usage error.
const EXIT_USAGE: i32 = 2;

/// Exit code the app reserves for "this host has no usable GPU device".
const EXIT_NO_GPU: i32 = 3;

/// Env var that turns every GPU skip in this file into a hard failure.
///
/// Same contract as `crates/mmd-engine/tests/render_correctness.rs`: a skipped
/// run and a verified run both print `ok`, so on a host that is supposed to
/// have a GPU the gate is run with `MMD_REQUIRE_GPU=1` and a skip becomes a
/// failure naming what was not verified.
const REQUIRE_GPU_ENV: &str = "MMD_REQUIRE_GPU";

/// A small scene: these tests assert lifecycle, not scale. The gate scene
/// is exercised by the documented `run --agents 5000 --frames 300` smoke.
const SMALL: &str = "64";

// ---------------------------------------------------------------------------
// Invocation helper
// ---------------------------------------------------------------------------

/// One completed run of the app binary.
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

    /// The line beginning with `run: clean exit`. Panics when absent — a run
    /// that claims success without it has broken the contract.
    fn exit_line(&self) -> &str {
        self.stdout
            .lines()
            .find(|l| l.starts_with("run: clean exit"))
            .unwrap_or_else(|| panic!("{self}\nno `run: clean exit` line"))
    }

    fn frame0_line(&self) -> &str {
        self.stdout
            .lines()
            .find(|l| l.starts_with("run: frame0"))
            .unwrap_or_else(|| panic!("{self}\nno `run: frame0` line"))
    }

    /// Value of `key=` in `line`. Contract lines carry no spaces inside a
    /// value, so whitespace splitting is exact.
    fn field<'a>(&self, line: &'a str, key: &str) -> &'a str {
        line.split_whitespace()
            .find_map(|tok| tok.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("{self}\nline `{line}` has no `{key}=`"))
    }

    fn exit_field(&self, key: &str) -> &str {
        self.field(self.exit_line(), key)
    }

    /// Final state hash from the exit line.
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

    /// A failure must exit with the *actionable* code and read as a message,
    /// not a crash.
    ///
    /// The code is asserted exactly, not merely as "non-zero": [`EXIT_NO_GPU`]
    /// is also non-zero, and it is the code [`or_skip`] treats as "skip this
    /// host". A regression that classified a bad scenario as a missing device
    /// would keep every failure case here green *and* silently skip every case
    /// that needs a GPU.
    fn assert_actionable_failure(&self) -> &Self {
        assert_eq!(
            self.code,
            Some(EXIT_ERROR),
            "{self}\nactionable failures exit {EXIT_ERROR}; {EXIT_NO_GPU} is reserved for a \
             missing GPU device and would make GPU cases skip instead of fail"
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

    /// Assert the message names every fragment a user needs to act on.
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

    fn overlay_lines(&self) -> usize {
        self.stdout
            .lines()
            .filter(|l| l.starts_with("backend=") && l.contains("agents="))
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
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_millions_must_die"));
    // Whole-file rule: no run this test binary spawns may appear on or focus
    // the developer's desktop. The two real-driver cases still build, claim,
    // present to and release a real window — it is never shown.
    cmd.env("MMD_WINDOW_HIDDEN", "1");
    cmd
}

/// Invoke the app with `args`. `offscreen` forces SDL's offscreen video driver
/// so a test never builds a window at all; the two cases that need a real
/// window opt out and get a hidden one instead (see [`app_bin`]).
fn invoke(args: &[&str], offscreen: bool) -> Cli {
    let mut cmd = app_bin();
    cmd.args(args);
    if offscreen {
        cmd.env("SDL_VIDEODRIVER", "offscreen");
    }
    // The app also reads these; a developer's shell must not steer a test.
    cmd.env_remove("MMD_RUN_FRAMES");
    cmd.env_remove("MMD_RUN_ONCE");
    run_to_completion(cmd, args.join(" "))
}

/// How long a run may take before the test treats it as never-terminating.
///
/// Generous: the largest case here is the gate scene at the live agent ceiling
/// for two frames in a debug build. The bound exists for the failure mode
/// where a frame budget stops being honoured — without it the process runs
/// forever and the suite *hangs* rather than failing, which in CI is
/// indistinguishable from a slow machine and blocks the gate instead of
/// reporting it.
const RUN_DEADLINE: Duration = Duration::from_secs(120);

/// Run `cmd` to completion or kill it at [`RUN_DEADLINE`].
///
/// Output goes to files rather than pipes: a killed child with a full pipe
/// would deadlock a parent that is waiting to read it.
fn run_to_completion(mut cmd: Command, label: String) -> Cli {
    // `cargo test` runs these cases in parallel threads of one process, so the
    // capture files must be unique per *invocation* — a shared name silently
    // hands one case another's output.
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
        // A killed child reports no exit code, same as a signal death — the
        // assertion below names which one happened.
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

/// Run `run` with a small scene, offscreen.
fn run_small(extra: &[&str]) -> Cli {
    let mut args = vec!["run", "--agents", SMALL];
    args.extend_from_slice(extra);
    invoke(&args, true)
}

fn gpu_is_required() -> bool {
    std::env::var_os(REQUIRE_GPU_ENV).is_some_and(|v| v != "0" && !v.is_empty())
}

/// Classify a completed run: `None` when this host simply has no GPU device.
///
/// Only exit code [`EXIT_NO_GPU`] skips. Every other failure is a real defect
/// and stays loud — widening this would hole the gate exactly the way
/// `RenderError::is_device_unavailable` warns about.
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

/// Accept a run that fell back to offscreen only when the app said why.
///
/// A window needs a *display*, which is a different capability from the GPU
/// device `MMD_REQUIRE_GPU` speaks about — so a display-less host is allowed to
/// miss the windowed path. What is not allowed is the window path quietly
/// vanishing: `or_skip` already proved a device exists, so the only legitimate
/// reasons left are window creation or the claim failing, and the app names
/// both on stderr.
fn assert_no_window_is_expected(cli: &Cli) {
    assert!(
        cli.stderr.contains("window create failed") || cli.stderr.contains("claim_window failed"),
        "{cli}\nfell back to offscreen without naming a reason — indistinguishable \
         from the windowed path silently disappearing"
    );
    let headed =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    assert!(
        !(headed && gpu_is_required()),
        "{cli}\nthis host has a display and declares a GPU, so the windowed path \
         must have been exercised"
    );
    eprintln!("SKIP: no window on this host");
}

// ---------------------------------------------------------------------------
// Scenario fixtures written to the test temp dir
// ---------------------------------------------------------------------------

fn tmp_dir(case: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(case);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Write `bytes` as a scenario at `<dir>/<name>.ron`, with a sidecar hash that
/// is either the true digest or a deliberately wrong one.
fn write_scenario(dir: &Path, name: &str, bytes: &[u8], honest_hash: bool) -> PathBuf {
    let ron = dir.join(format!("{name}.ron"));
    std::fs::write(&ron, bytes).expect("write scenario");
    let digest = if honest_hash {
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    } else {
        "0".repeat(64)
    };
    std::fs::write(ron.with_extension("sha256"), digest).expect("write sidecar");
    ron
}

fn gate_scenario_bytes() -> Vec<u8> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/scenarios/technical_prototype_v1.ron");
    std::fs::read(path).expect("read gate scenario")
}

// ---------------------------------------------------------------------------
// 1. Lifecycle — start, tick, exit
// ---------------------------------------------------------------------------

/// `--frames N` renders exactly N frames and exits 0.
///
/// `1` is in the set on purpose: the first frame is rendered before any loop
/// is entered, so a budget checked at the wrong end of the loop overshoots by
/// exactly one and only `--frames 1` shows it. The other two budgets are there
/// because a single one is also satisfied by a constant.
#[test]
fn run_exits_after_n_frames() {
    let mut after_one_frame = String::new();
    for n in ["1", "6", "9"] {
        let Some(cli) = or_skip("run_exits_after_n_frames", run_small(&["--frames", n])) else {
            return;
        };
        cli.assert_success();
        assert_eq!(
            cli.exit_field("frames"),
            n,
            "{cli}\nframe budget not honoured"
        );
        assert_eq!(
            cli.exit_field("tick"),
            n,
            "{cli}\nan unpaused run must tick once per frame"
        );
        assert_eq!(cli.exit_field("quit"), "false", "{cli}");
        assert_eq!(cli.field(cli.frame0_line(), "tick"), "1", "{cli}");

        if n == "1" {
            // A one-frame run ends on the state frame 1 produced.
            after_one_frame = cli.final_hash();
            continue;
        }

        // The reported state moved between the first frame and the last.
        // `Simulation::state_hash` digests the tick index, so this asserts the
        // hash is recomputed per frame rather than that agents moved — agent
        // motion is `crates/mmd-engine/tests/simulation.rs`.
        let first = cli.field(cli.frame0_line(), "hash");
        assert_ne!(
            first,
            cli.final_hash(),
            "{cli}\nthe exit line repeats the opening state hash"
        );
        // ...and the `frame0` hash is the state *after* frame 1, not the state
        // the run was loaded with: the one-frame run above ended exactly there.
        assert_eq!(
            first, after_one_frame,
            "{cli}\nthe frame0 hash is not the state frame 1 produced"
        );
    }
}

/// The same frame budget holds on the windowed path.
///
/// Every other budget case forces `SDL_VIDEODRIVER=offscreen` and so exercises
/// a different loop. The windowed loop owns the interactive command in the
/// merge gate, and it had the off-by-one the offscreen loop did not.
#[test]
fn windowed_run_honours_the_frame_budget() {
    for n in ["1", "5"] {
        let Some(cli) = or_skip(
            "windowed_run_honours_the_frame_budget",
            invoke(&["run", "--agents", SMALL, "--frames", n], false),
        ) else {
            return;
        };
        cli.assert_success();
        if cli.exit_field("mode") != "window" {
            assert_no_window_is_expected(&cli);
            return;
        }
        assert_eq!(
            cli.exit_field("frames"),
            n,
            "{cli}\nthe windowed loop overshot its budget"
        );
        assert_eq!(cli.exit_field("tick"), n, "{cli}");
    }
}

/// A quit scheduled for frame 1 ends the run before anything is rendered.
///
/// This is the one path that can print `clean exit` having drawn nothing, so
/// what it must *not* claim matters as much as what it reports.
#[test]
fn quit_before_the_first_frame_renders_nothing() {
    let Some(cli) = or_skip(
        "quit_before_the_first_frame_renders_nothing",
        run_small(&["--frames", "5", "--inject-input", "1:esc"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("frames"), "0", "{cli}");
    assert_eq!(cli.exit_field("tick"), "0", "{cli}");
    assert_eq!(cli.exit_field("quit"), "true", "{cli}");
    assert!(
        !cli.stdout.contains("run: frame0"),
        "{cli}\na run that quit before frame 1 must not report a first frame"
    );
    assert!(
        !cli.stdout.contains("offscreen draw ok"),
        "{cli}\nno frame was rendered, so nothing was drawn"
    );
}

/// A multi-entry script fires every entry, each on its own frame.
///
/// Single-entry scripts leave the comma-separated parse and the same-frame
/// sweep untested: a parser that kept only the first entry would pass every
/// other case in this file.
#[test]
fn multiple_scripted_presses_each_fire() {
    let Some(cli) = or_skip(
        "multiple_scripted_presses_each_fire",
        run_small(&["--frames", "8", "--inject-input", "2:f1,5:space"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("overlay"), "true", "{cli}\nF1 never fired");
    assert_eq!(cli.exit_field("paused"), "true", "{cli}\nSpace never fired");
    assert_eq!(
        cli.exit_field("tick"),
        "4",
        "{cli}\nthe pause landed on the wrong frame"
    );
    assert_eq!(
        cli.overlay_lines(),
        7,
        "{cli}\nthe HUD must run from frame 2 to frame 8"
    );
}

/// A quit cancels the presses queued behind it on the same frame.
///
/// Those presses would otherwise mutate a run that is already over, and the
/// exit line would report state from a frame nobody rendered. Cancelled means
/// unfired, and an unfired press is a failure — which is exactly how this is
/// observable from outside.
#[test]
fn quit_cancels_the_rest_of_its_frame() {
    let Some(cli) = or_skip(
        "quit_cancels_the_rest_of_its_frame",
        run_small(&["--frames", "9", "--inject-input", "3:esc,3:space"]),
    ) else {
        return;
    };
    cli.assert_actionable_failure()
        .assert_says(&["never fired", "3:space"]);
}

/// The frame budget can also come from the environment, and a budget that does
/// not parse is a typo rather than "run forever".
#[test]
fn frame_budget_from_the_environment() {
    let with_env = |value: &str| {
        let mut cmd = app_bin();
        cmd.args(["run", "--agents", SMALL])
            .env("SDL_VIDEODRIVER", "offscreen")
            .env("MMD_RUN_FRAMES", value)
            .env_remove("MMD_RUN_ONCE");
        run_to_completion(cmd, format!("MMD_RUN_FRAMES={value} run --agents {SMALL}"))
    };

    let Some(cli) = or_skip("frame_budget_from_the_environment", with_env("4")) else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.exit_field("frames"),
        "4",
        "{cli}\nMMD_RUN_FRAMES ignored"
    );

    // Blaming `--frames` for an env var the caller never typed sends them
    // looking in the wrong place.
    with_env("abc")
        .assert_actionable_failure()
        .assert_says(&["MMD_RUN_FRAMES", "\"abc\""]);
    with_env("0")
        .assert_actionable_failure()
        .assert_says(&["MMD_RUN_FRAMES 0"]);
}

/// With no budget at all and no window, the run still terminates.
///
/// An offscreen run has no way to quit by hand, so the default is what stands
/// between a headless invocation and a process that never returns.
#[test]
fn headless_run_without_a_budget_still_terminates() {
    let Some(cli) = or_skip(
        "headless_run_without_a_budget_still_terminates",
        run_small(&[]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("mode"), "offscreen", "{cli}");
    assert_eq!(
        cli.exit_field("frames"),
        "3",
        "{cli}\nthe headless default frame count changed"
    );
}

/// The agent-count override reaches the simulation, and the locked gate scene
/// is what a bare `run` loads.
#[test]
fn agent_count_override_is_respected() {
    let Some(cli) = or_skip(
        "agent_count_override_is_respected",
        run_small(&["--frames", "2"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert!(
        cli.stdout.contains(&format!("agents={SMALL}")),
        "{cli}\n--agents override not applied"
    );

    let Some(dflt) = or_skip(
        "agent_count_override_is_respected",
        invoke(&["run", "--frames", "2"], true),
    ) else {
        return;
    };
    dflt.assert_success();
    assert!(
        dflt.stdout.contains("agents=5000"),
        "{dflt}\nbare `run` must load the locked gate scene"
    );
}

// ---------------------------------------------------------------------------
// 2. Pause and overlay — scripted headless input
// ---------------------------------------------------------------------------

/// Pause stops simulation progress: the state hash is frozen at the value it
/// had when Space was pressed, for every remaining frame.
#[test]
fn pause_freezes_state() {
    // Control: two frames, no input.
    let Some(control) = or_skip("pause_freezes_state", run_small(&["--frames", "2"])) else {
        return;
    };
    control.assert_success();

    // Space before frame 3, then six more frames that must change nothing.
    let Some(paused) = or_skip(
        "pause_freezes_state",
        run_small(&["--frames", "8", "--inject-input", "3:space"]),
    ) else {
        return;
    };
    paused.assert_success();
    assert_eq!(
        paused.exit_field("frames"),
        "8",
        "{paused}\npause must not shorten the run"
    );
    assert_eq!(
        paused.exit_field("paused"),
        "true",
        "{paused}\nSpace did not reach the runtime"
    );
    assert_eq!(
        paused.exit_field("tick"),
        "2",
        "{paused}\nthe simulation kept ticking while paused"
    );
    assert_eq!(
        paused.final_hash(),
        control.final_hash(),
        "{paused}\n{control}\npaused state drifted from the frame it was frozen at"
    );
}

/// A run paused from its very first frame is a legitimate outcome, not a
/// stalled simulation.
///
/// The app refuses to report a clean exit when ticks and frames drift apart —
/// that guard is what catches a loop that stopped simulating. A run paused
/// before frame 1 has *zero* ticks against N frames and must be exempt, or the
/// app fails a run that did exactly what it was told.
#[test]
fn pause_from_the_first_frame_is_not_a_stalled_run() {
    let Some(cli) = or_skip(
        "pause_from_the_first_frame_is_not_a_stalled_run",
        run_small(&["--frames", "4", "--inject-input", "1:space"]),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(cli.exit_field("frames"), "4", "{cli}");
    assert_eq!(cli.exit_field("paused"), "true", "{cli}");
    assert_eq!(
        cli.exit_field("tick"),
        "0",
        "{cli}\na run paused before its first frame must never tick"
    );
    // Frozen from before the first tick: the closing hash is the opening one.
    assert_eq!(
        cli.field(cli.frame0_line(), "hash"),
        cli.final_hash(),
        "{cli}\nstate moved while paused"
    );
}

/// The overlay toggle is display-only: it changes what is printed, never the
/// simulation.
#[test]
fn overlay_toggle_is_inert() {
    let Some(plain) = or_skip("overlay_toggle_is_inert", run_small(&["--frames", "6"])) else {
        return;
    };
    plain.assert_success();

    let Some(shown) = or_skip(
        "overlay_toggle_is_inert",
        run_small(&["--frames", "6", "--inject-input", "2:f1"]),
    ) else {
        return;
    };
    shown.assert_success();

    // The toggle really fired...
    assert_eq!(shown.exit_field("overlay"), "true", "{shown}");
    assert_eq!(plain.exit_field("overlay"), "false", "{plain}");
    assert_eq!(
        plain.overlay_lines(),
        0,
        "{plain}\nHUD printed without the toggle"
    );
    assert_eq!(
        shown.overlay_lines(),
        5,
        "{shown}\nHUD must print for frames 2..=6 once F1 is on"
    );

    // ...and changed nothing about the run.
    assert_eq!(
        shown.exit_field("tick"),
        plain.exit_field("tick"),
        "{shown}"
    );
    assert_eq!(
        shown.final_hash(),
        plain.final_hash(),
        "{shown}\n{plain}\nthe overlay toggle altered simulation state"
    );
}

/// `H` toggles the hitbox overlay from a script, and changes nothing else.
///
/// The overlay defaults to **on**, so the interesting direction is turning it
/// off. Without the `hitboxes=` field on the exit line this test could not
/// exist: a scripted `h` press would produce byte-identical stdout whether the
/// binding worked or was dropped on the floor, and the ticket's claim that
/// `--inject-input N:h` drives the toggle headlessly would be unfalsifiable.
#[test]
fn hitbox_toggle_is_scriptable() {
    let Some(plain) = or_skip("hitbox_toggle_is_scriptable", run_small(&["--frames", "6"])) else {
        return;
    };
    plain.assert_success();

    let Some(hidden) = or_skip(
        "hitbox_toggle_is_scriptable",
        run_small(&["--frames", "6", "--inject-input", "2:h"]),
    ) else {
        return;
    };
    hidden.assert_success();

    // The toggle really fired, and the default really is on.
    assert_eq!(
        plain.exit_field("hitboxes"),
        "true",
        "{plain}\nhitbox rings must default to on"
    );
    assert_eq!(
        hidden.exit_field("hitboxes"),
        "false",
        "{hidden}\n`--inject-input 2:h` did not reach the hitbox toggle"
    );

    // ...and changed nothing about the run. A render overlay that moved the
    // simulation would be a far worse defect than one that failed to toggle.
    assert_eq!(
        hidden.exit_field("tick"),
        plain.exit_field("tick"),
        "{hidden}"
    );
    assert_eq!(
        hidden.final_hash(),
        plain.final_hash(),
        "{hidden}\n{plain}\nthe hitbox toggle altered simulation state"
    );
    assert_eq!(
        hidden.exit_field("overlay"),
        plain.exit_field("overlay"),
        "{hidden}\nthe hitbox toggle moved the HUD overlay flag"
    );
    assert_eq!(
        hidden.exit_field("paused"),
        plain.exit_field("paused"),
        "{hidden}\nthe hitbox toggle moved the pause flag"
    );
}

/// Quit ends the run early, releases the window from the GPU device before
/// dropping it, and exits 0 — no panic, no signal.
///
/// T31 found a real use-after-free here: destroying a claimed window left the
/// device holding a dangling swapchain and the process died on SIGSEGV *after*
/// its work was done. `code == Some(0)` is the guard — a signal death reports
/// `None`, never a code.
///
/// Division of labour, recorded because it bounds what this test proves:
/// `crates/mmd-engine/tests/render_correctness.rs` owns the claim/release
/// contract at the device level. This case owns the *app's* shutdown order —
/// that the quit path reaches the release site (asserted below) and that the
/// process then exits by returning rather than by dying. Deleting the release
/// call while keeping the report does not reproduce the crash on this host, so
/// treat the marker as evidence the path ran, not as a memory-safety proof.
#[test]
fn quit_exits_clean_and_releases_window() {
    // No offscreen override: this is the one case that wants a real window.
    let Some(cli) = or_skip(
        "quit_exits_clean_and_releases_window",
        invoke(
            &[
                "run",
                "--agents",
                SMALL,
                "--frames",
                "600",
                "--inject-input",
                "4:esc",
            ],
            false,
        ),
    ) else {
        return;
    };
    cli.assert_success();
    assert_eq!(
        cli.exit_field("quit"),
        "true",
        "{cli}\nEsc did not end the run"
    );
    assert_eq!(
        cli.exit_field("frames"),
        "3",
        "{cli}\nquit must stop before the frame it was scheduled on, \
         well short of the 600-frame budget"
    );

    match cli.exit_field("mode") {
        "window" => assert!(
            cli.stdout.contains("run: released window"),
            "{cli}\nthe window was never released from the device before shutdown"
        ),
        "offscreen" => assert_no_window_is_expected(&cli),
        other => panic!("{cli}\nunknown run mode `{other}`"),
    }
}

/// A scripted press that never happens is a silently vacuous test. The run
/// refuses to report success in that case.
#[test]
fn injection_that_never_fires_is_an_error() {
    let Some(cli) = or_skip(
        "injection_that_never_fires_is_an_error",
        run_small(&["--frames", "3", "--inject-input", "9:space"]),
    ) else {
        return;
    };
    cli.assert_actionable_failure()
        .assert_says(&["never fired", "9:space"]);
}

// ---------------------------------------------------------------------------
// 3. Failure paths — no GPU needed, these fail before the device is opened
// ---------------------------------------------------------------------------

/// A missing scenario names the file it looked for.
#[test]
fn missing_scenario_fails_clean() {
    let dir = tmp_dir("missing_scenario");
    let path = dir.join("nope.ron");
    let cli = run_small(&["--scenario", path.to_str().unwrap()]);
    // The path must come from the *loader's* own error, not only from a
    // wrapper the app adds: `{path}: No such file` can only be produced by the
    // read that failed, so it pins which file was actually missing.
    cli.assert_actionable_failure()
        .assert_says(&["scenario", &format!("{}: No such file", path.display())]);
}

/// A present scenario whose sidecar is missing names the *sidecar*, not just
/// the scenario — otherwise the message sends the user to the wrong file.
#[test]
fn missing_sidecar_names_the_sidecar() {
    let dir = tmp_dir("missing_sidecar");
    let path = dir.join("orphan.ron");
    std::fs::write(&path, gate_scenario_bytes()).expect("write scenario");
    let _ = std::fs::remove_file(path.with_extension("sha256"));
    let cli = run_small(&["--scenario", path.to_str().unwrap()]);
    cli.assert_actionable_failure()
        .assert_says(&[path.with_extension("sha256").to_str().unwrap()]);
}

/// Corrupt RON with an honest sidecar: the parse failure is reported against
/// the file, as a message.
#[test]
fn bad_scenario_fails_clean() {
    let dir = tmp_dir("bad_scenario");
    let path = write_scenario(&dir, "corrupt", b"(this is not a scenario", true);
    let cli = run_small(&["--scenario", path.to_str().unwrap()]);
    cli.assert_actionable_failure().assert_says(&[
        "scenario",
        path.to_str().unwrap(),
        "parse error",
    ]);
}

/// A scenario whose bytes no longer match its tracked hash is refused before
/// anything is built from it.
#[test]
fn drifted_scenario_fails_clean() {
    let dir = tmp_dir("drifted_scenario");
    let path = write_scenario(&dir, "drifted", &gate_scenario_bytes(), false);
    let cli = run_small(&["--scenario", path.to_str().unwrap()]);
    cli.assert_actionable_failure().assert_says(&[
        "scenario",
        path.to_str().unwrap(),
        "hash mismatch",
    ]);
}

/// A scenario that parses but breaks the contract is refused with the reason.
#[test]
fn invalid_scenario_fails_clean() {
    let dir = tmp_dir("invalid_scenario");
    let bytes = gate_scenario_bytes();
    let text = String::from_utf8(bytes).expect("scenario is utf-8");
    let broken = text.replace("seed: 5570183490285100849,", "seed: 0,");
    assert_ne!(broken, text, "the seed line moved; fix this fixture edit");
    let path = write_scenario(&dir, "invalid", broken.as_bytes(), true);
    let cli = run_small(&["--scenario", path.to_str().unwrap()]);
    cli.assert_actionable_failure().assert_says(&[
        "scenario",
        path.to_str().unwrap(),
        "seed must be nonzero",
    ]);
}

/// Agent counts outside the scenario's contract are rejected with the reason
/// and the bound, before any GPU work happens.
#[test]
fn absurd_agent_count_rejected() {
    let zero = invoke(&["run", "--agents", "0", "--frames", "2"], true);
    zero.assert_actionable_failure()
        .assert_says(&["--agents 0", "> 0"]);

    // The number the user needs is the bound, not the word "cap": the gate
    // scenario's stretch cap is 5000, and the message must name the range.
    let huge = invoke(&["run", "--agents", "4000000000", "--frames", "2"], true);
    huge.assert_actionable_failure().assert_says(&[
        "--agents 4000000000",
        "stretch cap of 5000",
        "1..=5000",
    ]);

    // One over the ceiling is the boundary a user actually hits, and it must be
    // refused by the same bound rather than sliding through to GPU work.
    let over = invoke(&["run", "--agents", "5001", "--frames", "1"], true);
    over.assert_actionable_failure().assert_says(&[
        "--agents 5001",
        "stretch cap of 5000",
        "1..=5000",
    ]);
}

/// `--frames 0` cannot mean what it says — the first frame is unconditional.
#[test]
fn zero_frames_rejected() {
    let cli = run_small(&["--frames", "0"]);
    cli.assert_actionable_failure().assert_says(&["--frames 0"]);
}

/// Every malformed injection script is refused by name, before the device is
/// opened.
#[test]
fn bad_input_script_rejected() {
    for (spec, needle) in [
        ("", "empty entry"),
        ("space", "not FRAME:KEY"),
        ("x:space", "not a number"),
        ("0:space", "1-based"),
        ("2:f9", "unknown key"),
        ("2:space,", "empty entry"),
    ] {
        let cli = run_small(&["--frames", "2", "--inject-input", spec]);
        cli.assert_actionable_failure().assert_says(&[needle]);
    }
}

// ---------------------------------------------------------------------------
// 4. CLI surface
// ---------------------------------------------------------------------------

#[test]
fn app_help_lists_run_and_bench() {
    let cli = invoke(&["--help"], true);
    cli.assert_success();
    for sub in ["run", "bench"] {
        // Match the subcommand *listing*, not the word anywhere: every flag
        // description in this help text contains "run".
        assert!(
            cli.stdout
                .lines()
                .any(|l| l.trim_start().starts_with(&format!("{sub} "))),
            "{cli}\nhelp does not list the `{sub}` subcommand"
        );
    }
}

#[test]
fn unknown_subcommand_fails() {
    let cli = invoke(&["wat"], true);
    assert_eq!(
        cli.code,
        Some(EXIT_USAGE),
        "{cli}\nclap usage errors exit {EXIT_USAGE}, distinct from an actionable \
         run failure ({EXIT_ERROR}) and from a missing device ({EXIT_NO_GPU})"
    );
    assert!(
        cli.combined().to_ascii_lowercase().contains("usage:"),
        "{cli}\nexpected a usage line"
    );
}

#[test]
fn workspace_metadata_has_expected_members() {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata = String::from_utf8_lossy(&output.stdout);
    for name in ["millions_must_die", "mmd-engine", "mmd-lab", "xtask"] {
        assert!(
            metadata.contains(&format!("\"name\":\"{name}\"")),
            "workspace metadata missing package {name}"
        );
    }
}
