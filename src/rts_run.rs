//! Interactive `rts` entry: the phase-1 RTS engine prototype scene (T14).
//!
//! Structurally a twin of `run.rs` — same window claim / event-pump / release
//! ordering, same exit codes — over a different world and a different
//! command set. Deliberately parallel files: `run` and `bench` are untouched
//! by this ticket.
//!
//! # stdout contract
//!
//! ```text
//! rts: settings warning=<escaped>                                                   (d)
//! rts: backend=<b> adapter=<a> view=<w>x<h> scenario=<path> (engine <v>)
//! rts: settings mode=<m> confine_pointer=<bool> keyboard_pan=<n> edge_pan=<n> \
//!      pause_on_focus_loss=<bool> master=<n> music=<n> voice=<n> sfx=<n>
//! rts: frame0 tick=<t> hash=<64 hex> world=[<n>,<n>,<n>] overlay=<n> ui=[<n>,<n>,<n>,<n>,<n>]   (a)
//! rts: offscreen draw ok (backend=<b>)                                              (a)
//! rts: window <w>x<h> claimed; Esc quit, Space pause, F1 overlay, X cancel, ...      (b)
//! <one HUD line per frame while the overlay is on>                                  (c)
//! rts: released window                                                              (b)
//! rts: clean exit mode=<offscreen|window> backend=<b> tick=<t> frames=<n> \
//!      hash=<64 hex> quit=<bool> paused=<bool> crystal=<n> gas=<n> \
//!      supply=<used>/<cap> units=<n> buildings=<n> nodes=<n> selected=<n> \
//!      camera=<cx>,<cy>
//! ```
//!
//! - (a) absent when a scripted quit lands on frame 1.
//! - (b) printed whenever a window was claimed.
//! - (c) from [`crate::rts_overlay::format_rts_overlay`].
//! - (d) printed only when persisted settings are missing/malformed/out of
//!   range/unsupported schema and this run fell back to
//!   [`crate::rts_settings::RtsSettings::default`]; absent on a clean load
//!   and always absent under `SDL_VIDEODRIVER=offscreen` (settings lookup is
//!   skipped entirely, so there is nothing to warn about).
//!
//! The `frame0` and `clean exit` lines are strictly `key=value` separated by
//! single spaces, with no spaces inside a value.
//!
//! # exit codes
//!
//! Reuses `crate::run::{RunError, EXIT_ERROR, EXIT_NO_GPU}` — one exit-code
//! contract for the whole binary. `MMD_RTS_FRAMES` / `MMD_RTS_ONCE` govern
//! this command; `run`'s `MMD_RUN_FRAMES` / `MMD_RUN_ONCE` are untouched by
//! it and vice versa.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mmd_engine::render::{
    RenderError, ScenePass, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH, edge_pan_dir,
};
use mmd_engine::rts::{
    DragBox, EntityId, EntityKind, OWNER_PLAYER, OrderReceiptBuffer, Placement, RtsFrame, RtsWorld,
    RtsWorldError, UnitKind, ghost_min_corner, is_drag, pack_frame, pack_hud,
};
use mmd_engine::scenario::ScenarioError;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Scancode;
use sdl3::mouse::MouseButton;

use crate::rts_input::{self, RtsCommand};
use crate::rts_overlay::format_rts_overlay;
use crate::rts_script::RtsScript;
use crate::rts_settings::{RtsSettings, SettingsStore, escape_warning};
use crate::rts_window::{self, FocusAction, RtsWindowState, SdlWindowOps, WindowOps};
use crate::run::RunError;

/// Frames rendered when neither `--frames` nor `MMD_RTS_FRAMES` is given and
/// no window could be opened.
const HEADLESS_DEFAULT_FRAMES: u64 = 3;

/// CLI options for the `rts` subcommand.
#[derive(Debug, Clone, Default)]
pub struct RtsOptions {
    pub scenario: Option<PathBuf>,
    pub frames: Option<u64>,
    pub inject_input: Option<String>,
    /// A script file, read with [`RtsScript::parse_file_text`]. Mutually
    /// exclusive with [`Self::inject_input`] at the clap layer.
    pub inject_input_file: Option<PathBuf>,
    /// Internal/test injection seam only — never set by the clap surface.
    /// When absent, an interactive run resolves
    /// [`SettingsStore::pref_path`] itself.
    pub settings_store: Option<SettingsStore>,
}

/// Per-frame interactive state the world does not own.
struct RtsSession {
    cursor: [f32; 2],
    /// Left button pressed at this position, if it is down.
    press: Option<[f32; 2]>,
    drag: Option<DragBox>,
    /// Currently held keyboard pan directions, summed and clamped per axis.
    keyboard_held: [f32; 2],
    paused: bool,
    overlay_visible: bool,
    quit: bool,
    /// Reused scratch for `RtsWorld::issue_context_order_at`.
    receipts: OrderReceiptBuffer,
}

impl Default for RtsSession {
    fn default() -> Self {
        Self {
            cursor: [0.0, 0.0],
            press: None,
            drag: None,
            keyboard_held: [0.0, 0.0],
            paused: false,
            overlay_visible: false,
            quit: false,
            receipts: OrderReceiptBuffer::new(),
        }
    }
}

fn add2(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub2(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn clamp_axes(v: [f32; 2]) -> [f32; 2] {
    [v[0].clamp(-1.0, 1.0), v[1].clamp(-1.0, 1.0)]
}

/// The first live player [`UnitKind::Worker`] in the selection, else the
/// first live player worker in the world.
fn find_builder(world: &RtsWorld) -> Option<EntityId> {
    let store = world.entities();
    for &id in world.selection().ids() {
        if let Some(slot) = store.slot(id)
            && store.owner(slot) == OWNER_PLAYER
            && store.kind(slot) == EntityKind::Unit(UnitKind::Worker)
        {
            return Some(id);
        }
    }
    for slot in 0..store.slot_count() {
        if store.alive(slot)
            && store.owner(slot) == OWNER_PLAYER
            && store.kind(slot) == EntityKind::Unit(UnitKind::Worker)
            && let Some(id) = store.id_at(slot)
        {
            return Some(id);
        }
    }
    None
}

/// Apply one [`RtsCommand`] to `world`/`session`. Shared by the live SDL path
/// and the scripted path, so the two cannot drift.
fn apply(world: &mut RtsWorld, session: &mut RtsSession, cmd: RtsCommand) {
    match cmd {
        RtsCommand::Quit => session.quit = true,
        RtsCommand::TogglePause => session.paused = !session.paused,
        RtsCommand::ToggleOverlay => session.overlay_visible = !session.overlay_visible,
        RtsCommand::CancelPlacement => world.cancel_placement(),
        RtsCommand::Build(kind) => {
            let _ = world.begin_placement(kind);
        }
        RtsCommand::Produce(unit) => {
            if let Some(id) = world.selection().primary() {
                let _ = world.enqueue_unit(id, unit);
            }
        }
        RtsCommand::SetRally => {
            if let Some(id) = world.selection().primary() {
                let view = world.iso_view();
                let width = world.scenario().width();
                let height = world.scenario().height();
                let cell = view.cell_at(session.cursor[0], session.cursor[1], width, height);
                let _ = world.set_rally(id, cell);
            }
        }
        RtsCommand::PanStart(d) => {
            session.keyboard_held = clamp_axes(add2(session.keyboard_held, d));
            world.set_keyboard_pan_dir(session.keyboard_held);
        }
        RtsCommand::PanStop(d) => {
            session.keyboard_held = clamp_axes(sub2(session.keyboard_held, d));
            world.set_keyboard_pan_dir(session.keyboard_held);
        }
        RtsCommand::Move(p) => {
            session.cursor = p;
            if let Some(a) = session.press
                && is_drag(a, p)
            {
                session.drag = Some(DragBox { a, b: p });
            }
            let edge = edge_pan_dir(p, [VIEW_WIDTH as f32, VIEW_HEIGHT as f32]);
            world.set_edge_pan_dir(edge);
        }
        RtsCommand::LeftClick(p) => {
            if let Placement::Pending { kind } = world.placement() {
                let view = world.iso_view();
                let width = world.scenario().width();
                let height = world.scenario().height();
                if let Some(cell) = view.cell_at(p[0], p[1], width, height) {
                    let min = ghost_min_corner(cell, kind.footprint_cells());
                    if let Some(builder) = find_builder(world) {
                        let _ = world.confirm_placement(min, builder);
                    }
                }
            } else {
                let view = world.iso_view();
                world.click_select(&view, p);
            }
        }
        RtsCommand::ShiftClick(p) => {
            // A shift-click never confirms a placement.
            let view = world.iso_view();
            world.shift_click_select(&view, p);
        }
        RtsCommand::Drag(a, b) => {
            let view = world.iso_view();
            world.box_select_into_selection(&view, a, b);
        }
        RtsCommand::RightClick(p) => {
            if matches!(world.placement(), Placement::Pending { .. }) {
                // A right click while a ghost is pending cancels it instead
                // of issuing an order.
                world.cancel_placement();
            } else {
                let view = world.iso_view();
                let _ = world.issue_context_order_at(&view, p, &mut session.receipts);
            }
        }
    }
}

/// Per-frame scratch bundled into one argument so `step_frame` /
/// `run_offscreen` stay under the too-many-arguments lint.
struct Scratch {
    frame_buf: RtsFrame,
    cmd_buf: Vec<RtsCommand>,
}

/// Everything the exit line reports, accumulated as the run proceeds.
struct RunState {
    frames: u64,
    quit: bool,
    expected_ticks: u64,
    first_hash: [u8; 32],
    last_hash: [u8; 32],
}

/// What one rendered frame reported.
struct FrameReport {
    tick: u64,
    world_lens: [usize; 3],
    overlay_len: usize,
    ui_lens: [usize; 5],
}

fn fmt_counts(counts: &[usize]) -> String {
    let joined = counts
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("[{joined}]")
}

/// Split a render failure into "this host has no GPU" and everything else —
/// the same classification `run::RunError::from_render` uses, duplicated
/// here because that associated fn is private to `run.rs`.
fn from_render(e: RenderError) -> RunError {
    if e.is_device_unavailable() {
        RunError::NoGpu(e.to_string())
    } else {
        RunError::Failed(e.to_string())
    }
}

/// Resolves validated settings once, before interactive window init.
///
/// An offscreen/deterministic run (`SDL_VIDEODRIVER=offscreen`) always uses
/// [`RtsSettings::default`] and never resolves [`SettingsStore::pref_path`]
/// (which creates the real per-user pref directory as a side effect of being
/// called) — the hard isolation constraint this ticket exists to prove.
fn load_settings(opts: &RtsOptions, offscreen_driver: bool) -> RtsSettings {
    if offscreen_driver {
        return RtsSettings::default();
    }

    let store = match opts.settings_store.clone() {
        Some(store) => Ok(store),
        None => SettingsStore::pref_path().map(SettingsStore::at),
    };

    match store {
        Ok(store) => {
            let loaded = store.load();
            if let Some(warning) = loaded.warning {
                println!("rts: settings warning={}", escape_warning(&warning));
            }
            loaded.value
        }
        Err(e) => {
            println!(
                "rts: settings warning={}",
                escape_warning(&format!("pref path unavailable: {e}"))
            );
            RtsSettings::default()
        }
    }
}

/// Run the `rts` subcommand.
pub fn run(opts: RtsOptions) -> Result<(), RunError> {
    // Detected once, up front, and reused for both the settings lookup below
    // and the later window-vs-offscreen fork: an offscreen/deterministic run
    // must never resolve the real per-user pref path (`SettingsStore::pref_path`
    // creates it as a side effect of being called), so this check has to gate
    // the settings lookup, not just window creation.
    let offscreen_driver = std::env::var_os("SDL_VIDEODRIVER")
        .map(|v| v == "offscreen")
        .unwrap_or(false);
    let settings = load_settings(&opts, offscreen_driver);

    let root = workspace_root_or_cwd();
    let scenario_path = opts
        .scenario
        .clone()
        .unwrap_or_else(|| root.join("assets/scenarios/rts_prototype_v1.ron"));

    // Flag validation first: a typo must not cost a device init.
    let auto_frames = resolve_frames(&opts)?;
    let mut script = match (
        opts.inject_input_file.as_deref(),
        opts.inject_input.as_deref(),
    ) {
        // A read or parse failure names the file, so the message points at the
        // line a human has to edit rather than at the flag. The `_` is not a
        // precedence rule: clap refuses both flags together, so this arm is
        // only reachable with `inject_input` unset.
        (Some(path), _) => {
            let text = std::fs::read_to_string(path).map_err(|e| {
                RunError::Failed(format!(
                    "--inject-input-file {}: {e}\nhint: point it at a readable script file",
                    path.display()
                ))
            })?;
            RtsScript::parse_file_text(&text).map_err(|e| {
                RunError::Failed(format!("--inject-input-file {}: {e}", path.display()))
            })?
        }
        (None, Some(spec)) => RtsScript::parse(spec).map_err(RunError::Failed)?,
        (None, None) => RtsScript::default(),
    };

    let mut world = RtsWorld::load(&scenario_path).map_err(|e| load_error(&scenario_path, e))?;
    world.set_camera_speeds(
        settings.camera.keyboard_pan as f32,
        settings.camera.edge_pan as f32,
    );
    let mut renderer = SpriteRenderer::new(&root, true).map_err(from_render)?;
    let iso = world.iso_view();
    renderer.set_depth_params(iso.depth_scale, iso.depth_bias);
    let backend = renderer.backend().to_string();

    println!(
        "rts: backend={} adapter={} view={}x{} scenario={} (engine {})",
        backend,
        renderer.ctx.adapter,
        VIEW_WIDTH,
        VIEW_HEIGHT,
        scenario_path.display(),
        mmd_engine::version()
    );
    println!("{}", settings.debug_line());

    let mut session = RtsSession::default();
    let mut scratch = Scratch {
        frame_buf: RtsFrame::new(),
        cmd_buf: Vec::with_capacity(8),
    };

    let initial_hash = world.state_hash();
    let mut state = RunState {
        frames: 0,
        quit: false,
        expected_ticks: 0,
        first_hash: initial_hash,
        last_hash: initial_hash,
    };

    let frame0 = step_frame(
        &mut world,
        &mut script,
        &mut session,
        &mut scratch,
        &mut state,
        |scene| renderer.draw_offscreen_scene(scene),
    )
    .map_err(from_render)?;

    let Some(frame0) = frame0 else {
        // A quit scheduled for frame 1: nothing rendered, nothing claimed.
        return finish(&mut script, &state, &session, &world, &backend, "offscreen");
    };
    println!(
        "rts: frame0 tick={} hash={} world={} overlay={} ui={}",
        frame0.tick,
        hex::encode(state.first_hash),
        fmt_counts(&frame0.world_lens),
        frame0.overlay_len,
        fmt_counts(&frame0.ui_lens),
    );
    println!("rts: offscreen draw ok (backend={backend})");

    // Acquired before the window is claimed: every `?` between a claim and
    // the matching `release_window` would drop a still-claimed window,
    // leaving the device with a dangling swapchain.
    let mut pump = renderer
        .ctx
        .sdl
        .event_pump()
        .map_err(|e| RunError::Failed(format!("SDL event pump unavailable: {e}")))?;

    let window = if offscreen_driver {
        None
    } else {
        match rts_window::build_rts_window(&renderer.ctx.video, settings.display.mode) {
            Ok(w) => match renderer.ctx.claim_window(&w) {
                Ok(()) => Some(w),
                Err(e) => {
                    eprintln!("rts: claim_window failed ({e}); offscreen-only");
                    None
                }
            },
            Err(e) => {
                eprintln!("rts: window create failed ({e}); offscreen-only");
                None
            }
        }
    };

    let Some(mut window) = window else {
        run_offscreen(
            &mut world,
            &mut renderer,
            &mut script,
            &mut session,
            &mut scratch,
            &mut state,
            auto_frames,
        )?;
        return finish(&mut script, &state, &session, &world, &backend, "offscreen");
    };

    println!(
        "rts: window {}x{} claimed; {}",
        VIEW_WIDTH,
        VIEW_HEIGHT,
        rts_input::window_banner()
    );

    // Startup grab: applied directly rather than waiting on a
    // `WindowEvent::FocusGained` — some window managers never deliver one
    // for the window that already has focus at creation, and a run must
    // never start with a stale (missing) confinement.
    if let Err(e) = SdlWindowOps(&mut window).set_mouse_grab(settings.display.confine_pointer) {
        eprintln!("rts: startup pointer grab failed ({e}); continuing unconfined");
    }
    let mut win_state = RtsWindowState {
        mode: settings.display.mode,
        focused: true,
        viewport: rts_window::refresh_viewport(&window)?,
    };

    if let Err(e) = renderer.draw_to_swapchain_scene(&window, scratch.frame_buf.scene()) {
        eprintln!("rts: present failed ({e}); offscreen-only");
        // Release before `window` drops: a still-claimed window leaves the
        // device holding a dangling swapchain.
        release_window(&renderer, window);
        run_offscreen(
            &mut world,
            &mut renderer,
            &mut script,
            &mut session,
            &mut scratch,
            &mut state,
            auto_frames,
        )?;
        return finish(&mut script, &state, &session, &world, &backend, "offscreen");
    }

    let mut present_error: Option<RenderError> = None;

    'running: loop {
        // Budget checked at the *top*: frame 1 is already rendered by the
        // time this loop is entered.
        if auto_frames.is_some_and(|limit| state.frames >= limit) {
            break;
        }

        // `win_state.viewport` is refreshed reactively below on
        // resize/pixel-size/display-change events, not recomputed every
        // batch: every mouse event still maps through whatever shape the
        // window has *now*, without a redundant `size()`/`size_in_pixels()`
        // syscall pair on batches that changed nothing.
        let viewport = win_state.viewport;

        // Collected rather than iterated live: `pump.keyboard_state()` below
        // needs an immutable borrow of `pump`, which cannot coexist with the
        // mutable borrow `pump.poll_iter()` holds for the loop's duration.
        let events: Vec<Event> = pump.poll_iter().collect();
        for event in events {
            match event {
                Event::Quit { .. } => {
                    session.quit = true;
                    state.quit = true;
                    break 'running;
                }
                Event::Window { win_event, .. } => match win_event {
                    WindowEvent::FocusGained => {
                        win_state.focused = true;
                        let mut ops = SdlWindowOps(&mut window);
                        if let Err(e) = rts_window::handle_focus(
                            &mut ops,
                            true,
                            settings.display.confine_pointer,
                            settings.gameplay.pause_on_focus_loss,
                            || {},
                        ) {
                            eprintln!("rts: focus-gain grab restore failed ({e})");
                        }
                    }
                    WindowEvent::FocusLost => {
                        win_state.focused = false;
                        let mut ops = SdlWindowOps(&mut window);
                        match rts_window::handle_focus(
                            &mut ops,
                            false,
                            settings.display.confine_pointer,
                            settings.gameplay.pause_on_focus_loss,
                            || {
                                session.keyboard_held = [0.0, 0.0];
                                session.press = None;
                                session.drag = None;
                                world.set_keyboard_pan_dir([0.0, 0.0]);
                                world.set_edge_pan_dir([0.0, 0.0]);
                            },
                        ) {
                            Ok(FocusAction::PauseRequested) => session.paused = true,
                            Ok(FocusAction::None) => {}
                            Err(e) => eprintln!("rts: focus-loss grab release failed ({e})"),
                        }
                    }
                    WindowEvent::Resized(_, _)
                    | WindowEvent::PixelSizeChanged(_, _)
                    | WindowEvent::DisplayChanged(_) => match rts_window::refresh_viewport(&window)
                    {
                        Ok(vp) => win_state.viewport = vp,
                        Err(e) => {
                            eprintln!("rts: viewport refresh failed ({e}); keeping previous")
                        }
                    },
                    _ => {}
                },
                Event::KeyDown {
                    keycode: Some(kc),
                    repeat: false,
                    ..
                } => {
                    if let Some(cmd) = rts_input::command_from_keycode(kc) {
                        apply(&mut world, &mut session, cmd);
                        if session.quit {
                            state.quit = true;
                            break 'running;
                        }
                    } else if let Some(dir) = rts_input::pan_from_keycode(kc) {
                        apply(&mut world, &mut session, RtsCommand::PanStart(dir));
                    }
                }
                Event::KeyUp {
                    keycode: Some(kc),
                    repeat: false,
                    ..
                } => {
                    if let Some(dir) = rts_input::pan_from_keycode(kc) {
                        apply(&mut world, &mut session, RtsCommand::PanStop(dir));
                    }
                }
                Event::MouseMotion { x, y, .. } => {
                    // Motion always updates the clamped logical cursor, even
                    // in a bar: that clamp-to-edge is what lets a pointer
                    // parked against the drawable's physical border still
                    // edge-pan the camera.
                    let mapped = viewport.map_pointer([x, y]);
                    apply(&mut world, &mut session, RtsCommand::Move(mapped.logical));
                }
                Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    let mapped = viewport.map_pointer([x, y]);
                    // A press that starts in a bar leaves `press` unset, so a
                    // release anywhere cannot read it as a drag/click origin
                    // — the bar press did nothing, per contract.
                    session.press = mapped.inside_content.then_some(mapped.logical);
                }
                Event::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    let mapped = viewport.map_pointer([x, y]);
                    session.drag = None;
                    if mapped.inside_content {
                        let end = mapped.logical;
                        let shift = {
                            let ks = pump.keyboard_state();
                            ks.is_scancode_pressed(Scancode::LShift)
                                || ks.is_scancode_pressed(Scancode::RShift)
                        };
                        let cmd = if shift {
                            RtsCommand::ShiftClick(end)
                        } else if session.press.is_some_and(|a| is_drag(a, end)) {
                            RtsCommand::Drag(session.press.unwrap(), end)
                        } else {
                            RtsCommand::LeftClick(end)
                        };
                        session.press = None;
                        apply(&mut world, &mut session, cmd);
                    } else {
                        // Release in a bar: no click/drag/order, per contract.
                        session.press = None;
                    }
                }
                Event::MouseButtonUp {
                    mouse_btn: MouseButton::Right,
                    x,
                    y,
                    ..
                } => {
                    let mapped = viewport.map_pointer([x, y]);
                    if mapped.inside_content {
                        apply(
                            &mut world,
                            &mut session,
                            RtsCommand::RightClick(mapped.logical),
                        );
                    }
                }
                _ => {}
            }
        }

        let frame_start = Instant::now();
        match step_frame(
            &mut world,
            &mut script,
            &mut session,
            &mut scratch,
            &mut state,
            |scene| renderer.draw_to_swapchain_scene(&window, scene),
        ) {
            Ok(None) => break 'running,
            Ok(Some(_)) => {}
            Err(e) => {
                present_error = Some(e);
                break 'running;
            }
        }

        if auto_frames.is_none() {
            let elapsed = frame_start.elapsed();
            if elapsed < Duration::from_millis(16) {
                std::thread::sleep(Duration::from_millis(16) - elapsed);
            }
        }
    }

    // Shutdown order: release the window from the device, drop the window,
    // then let `renderer` drop.
    release_window(&renderer, window);

    if let Some(e) = present_error {
        return Err(from_render(e));
    }

    finish(&mut script, &state, &session, &world, &backend, "window")
}

/// Tick to the frame budget with no window: same frame body, offscreen draws.
fn run_offscreen(
    world: &mut RtsWorld,
    renderer: &mut SpriteRenderer,
    script: &mut RtsScript,
    session: &mut RtsSession,
    scratch: &mut Scratch,
    state: &mut RunState,
    auto_frames: Option<u64>,
) -> Result<(), RunError> {
    let target = auto_frames.unwrap_or(HEADLESS_DEFAULT_FRAMES);
    while state.frames < target {
        match step_frame(world, script, session, scratch, state, |scene| {
            renderer.draw_offscreen_scene(scene)
        }) {
            Ok(None) => break,
            Ok(Some(_)) => {}
            Err(e) => return Err(from_render(e)),
        }
    }
    Ok(())
}

/// One frame: scripted input → apply → tick → pack → `draw` → optional HUD
/// line.
///
/// Returns `Ok(None)` when a scripted quit ended the run *before* this frame
/// was rendered.
fn step_frame<D>(
    world: &mut RtsWorld,
    script: &mut RtsScript,
    session: &mut RtsSession,
    scratch: &mut Scratch,
    state: &mut RunState,
    mut draw: D,
) -> Result<Option<FrameReport>, RenderError>
where
    D: FnMut(ScenePass<'_>) -> Result<(), RenderError>,
{
    let frame_buf = &mut scratch.frame_buf;
    let cmd_buf = &mut scratch.cmd_buf;

    let frame = state.frames + 1;
    cmd_buf.clear();
    if script.drain_frame(frame, cmd_buf) {
        session.quit = true;
        state.quit = true;
        return Ok(None);
    }
    for &cmd in cmd_buf.iter() {
        apply(world, session, cmd);
    }
    if session.quit {
        state.quit = true;
        return Ok(None);
    }

    if !session.paused {
        world.tick();
    }

    pack_frame(world, session.cursor, session.drag, frame_buf);
    pack_hud(world, frame_buf);

    draw(frame_buf.scene())?;

    let hash = world.state_hash();
    if frame == 1 {
        state.first_hash = hash;
    }
    state.frames = frame;
    state.last_hash = hash;
    // Counted per frame rather than latched: a run that pauses and then
    // unpauses must go back to owing one tick per frame.
    if !session.paused {
        state.expected_ticks += 1;
    }

    if session.overlay_visible {
        println!("{}", format_rts_overlay(world));
    }

    let world_lens = [
        frame_buf.world[0].instances.len(),
        frame_buf.world[1].instances.len(),
        frame_buf.world[2].instances.len(),
    ];
    let ui_lens = [
        frame_buf.ui[0].instances.len(),
        frame_buf.ui[1].instances.len(),
        frame_buf.ui[2].instances.len(),
        frame_buf.ui[3].instances.len(),
        frame_buf.ui[4].instances.len(),
    ];
    Ok(Some(FrameReport {
        tick: world.tick_index(),
        world_lens,
        overlay_len: frame_buf.overlay.len(),
        ui_lens,
    }))
}

/// Release the window from the device, then drop it — in that order.
fn release_window(renderer: &SpriteRenderer, window: sdl3::video::Window) {
    renderer.ctx.release_window(&window);
    drop(window);
    println!("rts: released window");
}

/// Final checks, then the exit line.
fn finish(
    script: &mut RtsScript,
    state: &RunState,
    session: &RtsSession,
    world: &RtsWorld,
    backend: &str,
    mode: &str,
) -> Result<(), RunError> {
    let unfired = script.unfired();
    if !unfired.is_empty() {
        return Err(RunError::Failed(format!(
            "--inject-input entries never fired: {} — the run ended after {} frame(s); \
             a scripted event that never happens makes the run prove nothing",
            unfired.join(", "),
            state.frames
        )));
    }

    if world.tick_index() != state.expected_ticks {
        return Err(RunError::Failed(format!(
            "tick {} after {} rendered frames ({} unpaused): a frame did not advance \
             the simulation",
            world.tick_index(),
            state.frames,
            state.expected_ticks
        )));
    }

    let res = world.resources();
    let supply = world.supply();
    let (units, buildings, nodes) = count_entities(world);
    // Cell-space camera centre, `x,y`. In the exit line because it is the only
    // way a scripted run can prove it panned: the camera is world state, and a
    // run that never looked away from its own base did not exercise it.
    let center = world.camera().center();

    println!(
        "rts: clean exit mode={mode} backend={backend} tick={} frames={} hash={} quit={} \
         paused={} crystal={} gas={} supply={}/{} units={} buildings={} nodes={} selected={} \
         camera={},{}",
        world.tick_index(),
        state.frames,
        hex::encode(state.last_hash),
        state.quit,
        session.paused,
        res.crystal,
        res.gas,
        supply.used(),
        supply.cap(),
        units,
        buildings,
        nodes,
        world.selection().len(),
        center[0],
        center[1],
    );
    Ok(())
}

fn count_entities(world: &RtsWorld) -> (u32, u32, u32) {
    let store = world.entities();
    let (mut units, mut buildings, mut nodes) = (0u32, 0u32, 0u32);
    for slot in 0..store.slot_count() {
        if !store.alive(slot) {
            continue;
        }
        match store.kind(slot) {
            EntityKind::Unit(_) => units += 1,
            EntityKind::Building(_) => buildings += 1,
            EntityKind::Node(_) => nodes += 1,
        }
    }
    (units, buildings, nodes)
}

/// Frame budget from `--frames`, else `MMD_RTS_FRAMES`, else `MMD_RTS_ONCE`.
///
/// Deliberately its own env vars, not `run`'s: `MMD_RUN_FRAMES` /
/// `MMD_RUN_ONCE` govern `run` only, so the two commands cannot be
/// accidentally cross-configured.
fn resolve_frames(opts: &RtsOptions) -> Result<Option<u64>, RunError> {
    const ENV: &str = "MMD_RTS_FRAMES";

    let (source, frames) = match opts.frames {
        Some(n) => ("--frames", Some(n)),
        None => match std::env::var(ENV) {
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
    Ok(std::env::var_os("MMD_RTS_ONCE")
        .is_some()
        .then_some(HEADLESS_DEFAULT_FRAMES))
}

/// Turn a load failure into a message that names the file or the flag at
/// fault.
fn load_error(path: &Path, e: RtsWorldError) -> RunError {
    let detail = match &e {
        RtsWorldError::Scenario(scenario @ ScenarioError::Io(_)) => format!(
            "scenario load failed: {scenario}\nhint: point --scenario at a valid file, or \
             restore the scenario and its .sha256 sidecar"
        ),
        RtsWorldError::Scenario(scenario) => format!(
            "scenario {}: {scenario}\nhint: point --scenario at a valid file, or restore \
             the scenario and its .sha256 sidecar",
            path.display()
        ),
        other => format!("scenario {}: {other}", path.display()),
    };
    RunError::Failed(detail)
}

fn workspace_root_or_cwd() -> PathBuf {
    let root = mmd_engine::workspace_root();
    if root.join("assets/sprites/generated/atlas_0.png").is_file() {
        root
    } else {
        std::env::current_dir().unwrap_or(root)
    }
}
