//! Interactive `run` entry: a moving flow-field horde (T8).
//!
//! Deliberately no population figure here or in the window title below. The
//! count is a runtime input (`--agents`, bounded by the scenario's stretch cap
//! and by `scenario::MAX_LIVE_AGENTS`), so any number baked into a string
//! becomes wrong the moment the cap or the scene moves — which is exactly what
//! happened to the population figure this line and the window title used to
//! carry: it named a count the CLI now refuses.
//!
//! # stdout contract
//!
//! ```text
//! run: backend=<b> adapter=<a> view=<w>x<h> agents=<n> scenario=<path> (engine <v>)
//! run: frame0 tick=<t> hash=<64 hex> groups=[<n>,..] sim=<f>ms upload=<f>ms   (a)
//! run: offscreen draw ok (backend=<b>)                                        (a)
//! run: window <w>x<h> claimed; Esc quit, F1 overlay, Space pause, H hitboxes  (b)
//! <two overlay HUD lines per frame>                                           (c)
//! run: released window                                                        (b)
//! run: clean exit mode=<offscreen|window> backend=<b> tick=<t> frames=<n> \
//!      hash=<64 hex> quit=<bool> paused=<bool> overlay=<bool> hitboxes=<bool>
//! ```
//!
//! - (a) absent when a scripted quit lands on frame 1: nothing was rendered,
//!   so there is no first frame to report.
//! - (b) printed whenever a window was *claimed* — including the fallback that
//!   claims a window, fails to present, releases it, and then finishes
//!   offscreen. `mode=` reports where the run ended, not whether it held a
//!   window, so these two can legitimately disagree.
//! - (c) while the overlay is toggled on, from [`overlay::format_overlay`].
//!
//! The `frame0` and `clean exit` lines — the two a test parses — are strictly
//! `key=value` separated by single spaces, with no spaces inside a value. The
//! rest is prose for a human.
//!
//! `hash` is the simulation state hash: on the `frame0` line after the first
//! frame, on the exit line after the last. It is digested at those two moments
//! and nowhere else — the digest walks every agent, and no frame in between has
//! a consumer for one. A run that quits before frame 1 prints neither a
//! `frame0` line nor a first digest, and pays for exactly one.
//! The `sim=`/`upload=` fields are for
//! a human — phase-0 acceptance is behavioural and nothing gates on a duration
//! (`docs/05-testing.md`).
//!
//! An exit line is printed only for a genuinely clean exit. A failure prints
//! nothing further and returns a [`RunError`] instead.
//!
//! # exit codes
//!
//! | code | meaning |
//! | ---- | ------- |
//! | 0 | ran to the frame budget, or quit, and shut down cleanly |
//! | 1 | actionable failure — bad flag value, bad scenario, render failure |
//! | 2 | clap usage error |
//! | [`EXIT_NO_GPU`] | this host has no usable GPU device |
//!
//! Code 3 exists so a headless shell can tell "no device here" apart from a
//! real defect without scraping the message.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mmd_engine::render::{
    ATLAS_COUNT, DrawGroup, RenderError, SpriteInstance, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH,
};
use mmd_engine::runtime::{BoundKey, InputAction, Runtime, RuntimeError, action_for_key};
use mmd_engine::scenario::ScenarioError;
use mmd_engine::workspace_root;
use sdl3::event::Event;

use crate::input;
use crate::overlay;

/// Fixed camera window matches gate resolution.
const WINDOW_W: u32 = VIEW_WIDTH;
const WINDOW_H: u32 = VIEW_HEIGHT;

/// Frames rendered when neither `--frames` nor `MMD_RUN_FRAMES` is given and
/// no window could be opened (there is no way to quit such a run by hand).
const HEADLESS_DEFAULT_FRAMES: u64 = 3;

/// Exit code for an actionable failure the user can fix.
pub const EXIT_ERROR: u8 = 1;

/// Exit code reserved for "this host has no usable GPU device".
///
/// Distinct from [`EXIT_ERROR`] so a headless CI shell — or a CLI contract
/// test — can skip on a missing device without also swallowing a real defect.
/// The classification is [`RenderError::is_device_unavailable`], which is
/// deliberately narrow: a rejected software adapter or a drifted atlas is a
/// failure, not a headless host.
pub const EXIT_NO_GPU: u8 = 3;

/// Why a run stopped.
#[derive(Debug)]
pub enum RunError {
    /// No GPU device at all on this host.
    NoGpu(String),
    /// Anything the user can act on: bad flag value, bad scenario, a render
    /// failure, or a run that could not prove what it claimed.
    Failed(String),
}

impl RunError {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::NoGpu(_) => EXIT_NO_GPU,
            Self::Failed(_) => EXIT_ERROR,
        }
    }

    /// Split a render failure into "this host has no GPU" and everything else.
    fn from_render(e: RenderError) -> Self {
        if e.is_device_unavailable() {
            Self::NoGpu(e.to_string())
        } else {
            Self::Failed(e.to_string())
        }
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Both variants are already a finished sentence — the renderer's own
        // message opens with "GPU device unavailable on this host", so
        // re-prefixing it would stutter. The variant carries the exit code,
        // not the wording.
        let (Self::NoGpu(detail) | Self::Failed(detail)) = self;
        write!(f, "{detail}")
    }
}

impl std::error::Error for RunError {}

/// CLI options for interactive / headless smoke.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub agents: Option<u32>,
    pub scenario: Option<PathBuf>,
    /// Auto-exit after N frames (CI/smoke). `None` = interactive until quit.
    pub frames: Option<u64>,
    /// Scripted headless key presses, `FRAME:KEY[,FRAME:KEY...]`.
    pub inject_input: Option<String>,
}

// ---------------------------------------------------------------------------
// Scripted headless input
// ---------------------------------------------------------------------------

/// One scheduled key press. `frame` is 1-based and names the frame the press
/// lands *before*, so `3:space` pauses after two frames have been rendered.
#[derive(Debug, Clone, Copy)]
struct Scheduled {
    frame: u64,
    key: BoundKey,
    name: &'static str,
    fired: bool,
}

/// Scripted keyboard input for runs with no keyboard.
///
/// Without this there is no way to reach pause, overlay, or quit except by
/// hand, which means the three behaviours the app is *for* have no automated
/// coverage. Presses resolve through [`input::bound_key_from_name`] into the
/// same [`action_for_key`] mapping the live SDL path uses.
#[derive(Debug, Default)]
struct InputScript {
    keys: Vec<Scheduled>,
}

impl InputScript {
    /// Parse `FRAME:KEY[,FRAME:KEY...]`. Every rejection names the offending
    /// entry — a script is typed by hand and a silent misparse would make the
    /// run prove nothing.
    fn parse(spec: &str) -> Result<Self, String> {
        let mut keys = Vec::new();
        for raw in spec.split(',') {
            let entry = raw.trim();
            if entry.is_empty() {
                return Err(format!(
                    "--inject-input {spec:?} has an empty entry; expected FRAME:KEY[,FRAME:KEY...]"
                ));
            }
            let (frame, key) = entry.split_once(':').ok_or_else(|| {
                format!("--inject-input entry {entry:?} is not FRAME:KEY (e.g. 4:space)")
            })?;
            let frame: u64 = frame.trim().parse().map_err(|_| {
                format!(
                    "--inject-input entry {entry:?}: frame {:?} is not a number",
                    frame.trim()
                )
            })?;
            if frame == 0 {
                return Err(format!(
                    "--inject-input entry {entry:?}: frames are 1-based, so the earliest \
                     press is frame 1"
                ));
            }
            let name = key.trim();
            let key = input::bound_key_from_name(name).ok_or_else(|| {
                format!(
                    "--inject-input entry {entry:?}: unknown key {name:?} (valid: {})",
                    input::key_names()
                )
            })?;
            keys.push(Scheduled {
                frame,
                key,
                name: input::name_of_bound_key(key),
                fired: false,
            });
        }
        Ok(Self { keys })
    }

    /// Apply every press scheduled for `frame`. Returns `true` when one of
    /// them was Quit, in which case `frame` is never rendered.
    ///
    /// Quit stops the sweep: a press queued behind it on the same frame would
    /// otherwise mutate a run that is already over, and the exit line would
    /// report state from a frame nobody saw. Those presses stay unfired, which
    /// [`Self::unfired`] then reports.
    fn apply(&mut self, frame: u64, runtime: &mut Runtime) -> bool {
        for scheduled in self
            .keys
            .iter_mut()
            .filter(|k| !k.fired && k.frame == frame)
        {
            scheduled.fired = true;
            match action_for_key(scheduled.key) {
                InputAction::Quit => return true,
                other => runtime.apply_action(other),
            }
        }
        false
    }

    /// Entries the run ended without reaching, in script order.
    fn unfired(&self) -> Vec<String> {
        self.keys
            .iter()
            .filter(|k| !k.fired)
            .map(|k| format!("{}:{}", k.frame, k.name))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

/// Everything the exit line reports, accumulated as the run proceeds.
///
/// Carries no state hash: the two lines that print one digest the runtime at
/// the moment they print it, so there is nothing to keep in step here.
struct RunState {
    /// Frames actually rendered (frame 1 is the offscreen proof frame).
    frames: u64,
    quit: bool,
    /// Ticks the frames rendered so far should have produced — one per
    /// unpaused frame.
    expected_ticks: u64,
}

/// Run full prototype: scenario → sim → instances → 4 draws.
pub fn run(opts: RunOptions) -> Result<(), RunError> {
    let root = workspace_root_or_cwd();
    let scenario_path = opts
        .scenario
        .clone()
        .unwrap_or_else(|| root.join("assets/scenarios/technical_prototype_v1.ron"));

    // Flag validation first: a typo must not cost a device init.
    let auto_frames = resolve_frames(&opts)?;
    let mut script = match opts.inject_input.as_deref() {
        Some(spec) => InputScript::parse(spec).map_err(RunError::Failed)?,
        None => InputScript::default(),
    };

    let mut runtime =
        Runtime::load(&scenario_path, opts.agents).map_err(|e| load_error(&scenario_path, e))?;
    let mut renderer = SpriteRenderer::new(&root, true).map_err(RunError::from_render)?;
    // The camera is fixed, so the depth normalisation is a per-scene constant:
    // set it once from the same projection the packers use, and the uniform the
    // GPU reads can never describe a different map than the CPU packed.
    let iso = runtime.iso_view();
    renderer.set_depth_params(iso.depth_scale, iso.depth_bias);
    let backend = renderer.backend().to_string();

    println!(
        "run: backend={} adapter={} view={}x{} agents={} scenario={} (engine {})",
        backend,
        renderer.ctx.adapter,
        VIEW_WIDTH,
        VIEW_HEIGHT,
        runtime.agent_count(),
        scenario_path.display(),
        mmd_engine::version()
    );

    let mut state = RunState {
        frames: 0,
        quit: false,
        expected_ticks: 0,
    };

    // Frame 1 is the headless proof: nonempty groups and a real offscreen draw
    // before any window exists. It goes through the same body as every other
    // frame — a parallel first-frame body is how a scripted press, an overlay
    // toggle, or a quit ends up honoured on frame 2 but not frame 1.
    let frame0 = step_frame(
        &mut runtime,
        &mut script,
        &mut state,
        &backend,
        |groups, rings| renderer.draw_offscreen_with_rings(groups, rings),
    )
    .map_err(RunError::from_render)?;

    let Some(frame0) = frame0 else {
        // A quit scheduled for frame 1: nothing rendered, nothing claimed.
        return finish(&mut script, &state, &runtime, &backend, "offscreen");
    };
    println!(
        "run: frame0 tick={} hash={} groups={:?} sim={:.3}ms upload={:.3}ms",
        frame0.tick,
        // First of the run's two digests. Taken here rather than inside the
        // frame body: nothing has touched the simulation since frame 1 packed,
        // so this is the state frame 1 produced.
        hex::encode(runtime.state_hash()),
        frame0.group_lens,
        frame0.sim_ms,
        frame0.upload_ms
    );
    if frame0.group_lens.contains(&0) {
        return Err(RunError::Failed(
            "empty instance group after first frame".into(),
        ));
    }
    println!("run: offscreen draw ok (backend={backend})");

    // Visible path when the display can present. Offscreen driver → tick loop only.
    let offscreen_driver = std::env::var_os("SDL_VIDEODRIVER")
        .map(|v| v == "offscreen")
        .unwrap_or(false);

    // Acquired before the window is claimed: every `?` between a claim and the
    // matching `release_window` would drop a still-claimed window, leaving the
    // device with a dangling swapchain.
    let mut pump = renderer
        .ctx
        .sdl
        .event_pump()
        .map_err(|e| RunError::Failed(format!("SDL event pump unavailable: {e}")))?;

    let window = if offscreen_driver {
        None
    } else {
        match renderer
            .ctx
            .video
            .window("millions_must_die — flow-field horde", WINDOW_W, WINDOW_H)
            .position_centered()
            .build()
        {
            Ok(w) => match renderer.ctx.claim_window(&w) {
                Ok(()) => Some(w),
                Err(e) => {
                    eprintln!("run: claim_window failed ({e}); offscreen-only");
                    None
                }
            },
            Err(e) => {
                eprintln!("run: window create failed ({e}); offscreen-only");
                None
            }
        }
    };

    let Some(window) = window else {
        run_offscreen(
            &mut runtime,
            &mut renderer,
            &mut script,
            &mut state,
            &backend,
            auto_frames,
        )?;
        return finish(&mut script, &state, &runtime, &backend, "offscreen");
    };

    println!(
        "run: window {}x{} claimed; Esc quit, F1 overlay, Space pause, H hitboxes",
        WINDOW_W, WINDOW_H
    );

    // Present the frame-1 groups (still frame-1 contents — no further tick yet).
    // Rings come along: a first frame drawn without them would show the overlay
    // switching itself on at frame 2.
    if let Err(e) = renderer.draw_to_swapchain_with_rings(
        &window,
        runtime.draw_groups(),
        runtime.ring_instances(),
    ) {
        eprintln!("run: present failed ({e}); offscreen-only");
        // Release before `window` drops: a still-claimed window leaves the
        // device holding a dangling swapchain.
        release_window(&renderer, window);
        run_offscreen(
            &mut runtime,
            &mut renderer,
            &mut script,
            &mut state,
            &backend,
            auto_frames,
        )?;
        return finish(&mut script, &state, &runtime, &backend, "offscreen");
    }

    let mut present_error: Option<RenderError> = None;

    'running: loop {
        // Budget checked at the *top*: frame 1 is already rendered by the time
        // this loop is entered, so a tail check always overshoots by one and
        // `--frames 1` would render two.
        if auto_frames.is_some_and(|limit| state.frames >= limit) {
            break;
        }

        for event in pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    state.quit = true;
                    break 'running;
                }
                Event::KeyDown {
                    keycode: Some(kc), ..
                } => {
                    if let Some(action) = input::action_from_keycode(kc) {
                        match action {
                            InputAction::Quit => {
                                state.quit = true;
                                break 'running;
                            }
                            other => runtime.apply_action(other),
                        }
                    }
                }
                _ => {}
            }
        }

        let frame_start = Instant::now();
        // Errors leave the loop rather than returning through `?`, so the
        // window is always released from the device before it is dropped.
        match step_frame(
            &mut runtime,
            &mut script,
            &mut state,
            &backend,
            |groups, rings| renderer.draw_to_swapchain_with_rings(&window, groups, rings),
        ) {
            Ok(None) => break 'running,
            Ok(Some(_)) => {}
            Err(e) => {
                present_error = Some(e);
                break 'running;
            }
        }

        // Soft pace toward 60 Hz when running interactively without frame cap.
        if auto_frames.is_none() {
            let elapsed = frame_start.elapsed();
            if elapsed < Duration::from_millis(16) {
                std::thread::sleep(Duration::from_millis(16) - elapsed);
            }
        }
    }

    // Shutdown order: release the window from the device, drop the window, then
    // let `renderer` drop. Destroying a claimed window first would leave the
    // device with a dangling swapchain (T31: a real SIGSEGV on shutdown).
    release_window(&renderer, window);

    if let Some(e) = present_error {
        return Err(RunError::from_render(e));
    }

    finish(&mut script, &state, &runtime, &backend, "window")
}

/// Tick to the frame budget with no window: same frame body, offscreen draws.
fn run_offscreen(
    runtime: &mut Runtime,
    renderer: &mut SpriteRenderer,
    script: &mut InputScript,
    state: &mut RunState,
    backend: &str,
    auto_frames: Option<u64>,
) -> Result<(), RunError> {
    let target = auto_frames.unwrap_or(HEADLESS_DEFAULT_FRAMES);
    while state.frames < target {
        match step_frame(runtime, script, state, backend, |groups, rings| {
            renderer.draw_offscreen_with_rings(groups, rings)
        }) {
            Ok(None) => break,
            Ok(Some(_)) => {}
            Err(e) => return Err(RunError::from_render(e)),
        }
    }
    Ok(())
}

/// What one rendered frame reported. Only the first frame's copy is printed;
/// the rest is why the caller can tell a rendered frame from a scripted quit.
struct FrameReport {
    tick: u64,
    group_lens: [usize; ATLAS_COUNT],
    sim_ms: f64,
    upload_ms: f64,
}

/// One frame: scripted input → tick → pack → `draw` → optional HUD line.
///
/// Returns `Ok(None)` when a scripted quit ended the run *before* this frame
/// was rendered, so the frame count reflects frames that actually happened.
fn step_frame<D>(
    runtime: &mut Runtime,
    script: &mut InputScript,
    state: &mut RunState,
    backend: &str,
    mut draw: D,
) -> Result<Option<FrameReport>, RenderError>
where
    D: FnMut(&[DrawGroup; ATLAS_COUNT], &[SpriteInstance]) -> Result<(), RenderError>,
{
    let frame = state.frames + 1;
    if script.apply(frame, runtime) {
        state.quit = true;
        return Ok(None);
    }

    let frame_start = Instant::now();
    // Unhashed: a rendered frame has no consumer for the state digest, and the
    // digest walks every agent. The two lines that print one take their own.
    let out = runtime.tick_and_render_unhashed();
    let overlay_visible = out.overlay_visible;
    let agent_count = out.agent_count;
    let tick_index = out.tick_index;
    let paused = out.paused;
    let mut stats = out.stats;
    let mut group_lens = [0usize; ATLAS_COUNT];
    for (slot, group) in group_lens.iter_mut().zip(out.groups) {
        *slot = group.instances.len();
    }
    draw(out.groups, out.rings)?;

    state.frames = frame;
    // Counted per frame rather than latched: a run that pauses and then
    // unpauses must go back to owing one tick per frame, or the lockstep check
    // in `finish` stays switched off for the rest of the run.
    if !paused {
        state.expected_ticks += 1;
    }

    if overlay_visible {
        // stdout HUD (no text GPU path in phase-0). `total_ms` here spans the
        // draw as well, which the runtime's own figure cannot see.
        stats.total_ms = frame_start.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{}",
            overlay::format_overlay(backend, agent_count, tick_index, paused, stats)
        );
    }
    Ok(Some(FrameReport {
        tick: tick_index,
        group_lens,
        sim_ms: stats.sim_ms,
        upload_ms: stats.upload_ms,
    }))
}

/// Release the window from the device, then drop it — in that order.
fn release_window(renderer: &SpriteRenderer, window: sdl3::video::Window) {
    renderer.ctx.release_window(&window);
    drop(window);
    println!("run: released window");
}

/// Final checks, then the exit line. Only a run that can defend its claims
/// gets to print `clean exit`.
fn finish(
    script: &mut InputScript,
    state: &RunState,
    runtime: &Runtime,
    backend: &str,
    mode: &str,
) -> Result<(), RunError> {
    // A scripted press that never happened means the run did not do what it
    // was asked to — reporting success would make the caller's evidence void.
    let unfired = script.unfired();
    if !unfired.is_empty() {
        return Err(RunError::Failed(format!(
            "--inject-input entries never fired: {} — the run ended after {} frame(s); \
             a scripted press that never happens makes the run prove nothing",
            unfired.join(", "),
            state.frames
        )));
    }

    // Every unpaused frame owes exactly one tick. This is a self-check for the
    // *interactive* command (`run --agents 5000 --frames 300`), which no test
    // drives: a frame that quietly stopped advancing the simulation would
    // otherwise still report a clean exit.
    //
    // It deliberately does not compare state hashes. `Simulation::state_hash`
    // digests `tick_index`, so "the hash moved" is implied by the tick moving
    // and would prove nothing about the agents — a check that cannot fail is
    // worse than none, because it reads like a proof.
    if runtime.tick_index() != state.expected_ticks {
        return Err(RunError::Failed(format!(
            "tick {} after {} rendered frames ({} unpaused): a frame did not advance \
             the simulation",
            runtime.tick_index(),
            state.frames,
            state.expected_ticks
        )));
    }

    // `hitboxes=` is reported for the same reason `overlay=` is: without it a
    // scripted `H` press produces byte-identical stdout whether the toggle
    // worked or was silently dropped, and `--inject-input N:h` would prove
    // nothing.
    println!(
        "run: clean exit mode={mode} backend={backend} tick={} frames={} hash={} quit={} paused={} overlay={} hitboxes={}",
        runtime.tick_index(),
        state.frames,
        // Second of the run's two digests — and the only one on a run that quit
        // before frame 1. Nothing mutates the simulation after the last
        // rendered frame (`apply_action` reaches pause/overlay/hitboxes only),
        // so this is the state the last frame left behind.
        hex::encode(runtime.state_hash()),
        state.quit,
        runtime.paused(),
        runtime.overlay_visible(),
        runtime.hitboxes_visible()
    );
    Ok(())
}

/// Frame budget from `--frames`, else `MMD_RUN_FRAMES`, else `MMD_RUN_ONCE`.
///
/// Every rejection names the source it came from: blaming `--frames` for an
/// env var the caller never typed sends them looking in the wrong place.
fn resolve_frames(opts: &RunOptions) -> Result<Option<u64>, RunError> {
    const ENV: &str = "MMD_RUN_FRAMES";

    let (source, frames) = match opts.frames {
        Some(n) => ("--frames", Some(n)),
        None => match std::env::var(ENV) {
            // A budget that does not parse is a typo, not "run forever".
            Ok(raw) => (
                ENV,
                Some(raw.trim().parse::<u64>().map_err(|_| {
                    RunError::Failed(format!(
                        "{ENV}={raw:?} is not a frame count: set it to a whole number >= 1, \
                         or unset it to run interactively"
                    ))
                })?),
            ),
            Err(_) => (ENV, None),
        },
    };

    if frames == Some(0) {
        return Err(RunError::Failed(format!(
            "{source} 0 renders nothing: the first frame is unconditional, so N must be >= 1"
        )));
    }
    if frames.is_some() {
        return Ok(frames);
    }
    Ok(std::env::var_os("MMD_RUN_ONCE")
        .is_some()
        .then_some(HEADLESS_DEFAULT_FRAMES))
}

/// Turn a load failure into a message that names the file or the flag at fault.
fn load_error(path: &Path, e: RuntimeError) -> RunError {
    let detail = match e {
        // `ScenarioError::Io` already carries the exact file it could not read
        // — which may be the sidecar, not the scenario — so repeating the
        // scenario path here would name the wrong file twice.
        RuntimeError::Scenario(scenario @ ScenarioError::Io(_)) => format!(
            "scenario load failed: {scenario}\nhint: point --scenario at a valid file, or \
             restore the scenario and its .sha256 sidecar"
        ),
        RuntimeError::Scenario(scenario) => format!(
            "scenario {}: {scenario}\nhint: point --scenario at a valid file, or restore \
             the scenario and its .sha256 sidecar",
            path.display()
        ),
        RuntimeError::ZeroAgents => {
            "--agents 0 is not a runnable scene: the agent count must be > 0".into()
        }
        RuntimeError::AgentCount { got, cap } => format!(
            "--agents {got} exceeds the scenario's stretch cap of {cap}: pick a count in \
             1..={cap}, or use a scenario with a larger cap"
        ),
    };
    RunError::Failed(detail)
}

fn workspace_root_or_cwd() -> PathBuf {
    let root = workspace_root();
    if root.join("assets/sprites/generated/atlas_0.png").is_file() {
        root
    } else {
        std::env::current_dir().unwrap_or(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate scene at a small population: CPU only, no device needed.
    fn test_runtime() -> Runtime {
        let scenario = workspace_root_or_cwd().join("assets/scenarios/technical_prototype_v1.ron");
        Runtime::load(&scenario, Some(64)).expect("load gate scenario")
    }

    fn fresh_state() -> RunState {
        RunState {
            frames: 0,
            quit: false,
            expected_ticks: 0,
        }
    }

    /// Stand-in for the offscreen/swapchain draw. The frame body under test is
    /// the real `step_frame`; only the device is replaced.
    fn noop_draw(
        _groups: &[DrawGroup; ATLAS_COUNT],
        _rings: &[SpriteInstance],
    ) -> Result<(), RenderError> {
        Ok(())
    }

    /// A rendered frame must not digest: no line prints a per-frame hash.
    #[test]
    fn rendered_frames_never_digest_state() {
        let mut runtime = test_runtime();
        let mut script = InputScript::default();
        let mut state = fresh_state();

        for frame in 1..=5u64 {
            let report = step_frame(&mut runtime, &mut script, &mut state, "test", noop_draw)
                .expect("frame body")
                .expect("a frame was rendered");
            assert_eq!(report.tick, frame, "a frame must advance the simulation");
        }

        assert_eq!(state.frames, 5);
        assert_eq!(state.expected_ticks, 5);
        assert_eq!(runtime.tick_index(), 5);
        assert_eq!(
            runtime.state_hash_calls(),
            0,
            "the frame body digested a state nothing prints"
        );
    }

    /// The `frame0` line reports the state frame 1 produced.
    ///
    /// Checked against a second runtime advanced one tick through `tick_only`,
    /// so the expectation does not flow through the code under test.
    #[test]
    fn the_frame0_hash_is_the_state_frame_one_produced() {
        let mut driven = test_runtime();
        let mut script = InputScript::default();
        let mut state = fresh_state();
        step_frame(&mut driven, &mut script, &mut state, "test", noop_draw)
            .expect("frame body")
            .expect("a frame was rendered");

        let printed = driven.state_hash();

        let mut independent = test_runtime();
        assert!(independent.tick_only(), "the control run must tick");
        assert_eq!(
            printed,
            independent.sim().state_hash(),
            "the frame0 line reports a state frame 1 did not produce"
        );
        assert_eq!(
            driven.state_hash_calls(),
            1,
            "the frame0 line owes exactly one digest"
        );
    }

    /// A whole run digests twice: once for `frame0`, once for the exit line.
    #[test]
    fn a_whole_run_digests_at_its_two_endpoints() {
        let mut runtime = test_runtime();
        let mut script = InputScript::default();
        let mut state = fresh_state();

        step_frame(&mut runtime, &mut script, &mut state, "test", noop_draw)
            .expect("frame body")
            .expect("a frame was rendered");
        let frame0_hash = runtime.state_hash();
        for _ in 0..2 {
            step_frame(&mut runtime, &mut script, &mut state, "test", noop_draw)
                .expect("frame body")
                .expect("a frame was rendered");
        }

        finish(&mut script, &state, &runtime, "test", "offscreen").expect("clean exit");

        assert_eq!(
            runtime.state_hash_calls(),
            2,
            "a run may digest at its two endpoints and nowhere else"
        );
        // Read off the simulation: `Runtime::state_hash` is counted, and this
        // observation must not spend the budget just asserted.
        assert_ne!(
            frame0_hash,
            runtime.sim().state_hash(),
            "the exit line must report the final state, not frame 1's"
        );
    }

    /// A scripted quit on frame 1 renders nothing, so it pays for one digest —
    /// the exit line's — and never reaches the `frame0` line.
    #[test]
    fn a_quit_before_the_first_frame_digests_once() {
        let mut runtime = test_runtime();
        let mut script = InputScript::parse("1:esc").expect("script");
        let mut state = fresh_state();

        let rendered = step_frame(&mut runtime, &mut script, &mut state, "test", noop_draw)
            .expect("frame body");
        assert!(rendered.is_none(), "a quit cancels its own frame");
        assert!(state.quit);
        assert_eq!(state.frames, 0);
        assert_eq!(runtime.tick_index(), 0);

        finish(&mut script, &state, &runtime, "test", "offscreen").expect("clean exit");

        assert_eq!(
            runtime.state_hash_calls(),
            1,
            "a run that rendered nothing owes exactly the exit line's digest"
        );
    }
}
