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
//! rts: audio music=<n> voice=<n> cues=<n> reject=<n> ui=<n> gains=<m>/<v>/<s>       (e)
//! rts: clean exit mode=<offscreen|window> backend=<b> tick=<t> frames=<n> \
//!      hash=<64 hex> quit=<bool> paused=<bool> crystal=<n> gas=<n> \
//!      supply=<used>/<cap> units=<n> buildings=<n> nodes=<n> selected=<n> \
//!      camera=<cx>,<cy> body_overlaps=<n> ui_page=<p> music_starts=<n> \
//!      voice_select=<n> voice_order=<n> voice_reject=<n> sfx_ui=<n> \
//!      keyboard_pan=<n>                                                             (f)
//! ```
//!
//! - (a) absent when a scripted quit lands on frame 1.
//! - (b) printed whenever a window was claimed.
//! - (c) from [`crate::rts_overlay::format_rts_overlay`].
//! - (d) printed only when persisted settings are missing/malformed/out of
//!   range/unsupported schema and this run fell back to
//!   [`crate::rts_settings::RtsSettings::default`]; absent on a clean load
//!   and always absent under a non-interactive `SDL_VIDEODRIVER` (`offscreen`
//!   or `dummy`: the settings lookup is skipped entirely, so there is nothing
//!   to warn about).
//! - (e) the deterministic semantic audio trace (`T15`): one `StartMusic`
//!   per session, one `voice` batch per action that voiced anything (`cues`
//!   counts the individual unit cues inside them), one `reject` per refused
//!   action, one `ui` per successful enabled pointer activation, and the
//!   effective per-bus gains in basis points. No physical device is ever
//!   opened by this line's sink.
//! - (f) `T17`'s joined phase-1.1 observation, in exactly this order:
//!   `body_overlaps` from [`mmd_engine::rts::RtsWorld::body_overlap_count`]
//!   (penetrating live-unit pairs; a shipped run must always report 0),
//!   `ui_page` from the `T13` menu FSM (`gameplay|pause_menu|settings`),
//!   then the `T15` audio counters split by meaning — `music_starts`,
//!   `voice_select` (Select cues), `voice_order` (accepted Move/Gather/Build
//!   cues), `voice_reject`, `sfx_ui` — and finally the live
//!   `keyboard_pan` speed in cells/s, which a settings edit can move
//!   mid-run.
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
    DisplayViewport, RenderError, ScenePass, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH, edge_pan_dir,
};
use mmd_engine::rts::{
    DragBox, EntityId, EntityKind, MAX_SELECTION, OWNER_PLAYER, OrderReceiptBuffer, Placement,
    RtsFrame, RtsWorld, RtsWorldError, UnitKind, ghost_min_corner, is_drag, pack_frame,
};
use mmd_engine::scenario::ScenarioError;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Scancode;
use sdl3::mouse::MouseButton;

use crate::rts_audio::{BufferedAudioSink, SdlAudioSink};
use crate::rts_feedback::{
    AudioBus, AudioCounters, AudioError, AudioEvent, AudioSink, FakeAudioSink, effective_gains,
    order_cues, reject_cue, selection_cues, snapshot_selected_units,
};
use crate::rts_input::{self, RtsCommand};
use crate::rts_overlay::format_rts_overlay;
use crate::rts_script::RtsScript;
use crate::rts_settings::{CameraSettings, RtsSettings, SettingsStore, escape_warning};
use crate::rts_ui::{PointerOwner, RtsUiState, SettingsChange};
use crate::rts_window::{
    self, ClaimedWindow, FocusAction, ModeCandidate, RtsWindowState, SdlWindowOps, WindowOps,
};
use crate::run::RunError;
use mmd_engine::rts::{control_id_from_hud_hit, control_id_from_modal_hit};

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
pub(crate) struct RtsSession {
    cursor: [f32; 2],
    /// Left button pressed at this position, if it is down.
    press: Option<[f32; 2]>,
    /// Owner classified at pointer-down — retained until release (`T1`).
    press_owner: PointerOwner,
    /// Stable control identity at pointer-down, when the owner named one.
    press_control: Option<mmd_engine::rts::ControlId>,
    drag: Option<DragBox>,
    /// Currently held keyboard pan directions, summed and clamped per axis.
    keyboard_held: [f32; 2],
    /// The paused-menu FSM and every pause reason (`T13`) — the single
    /// source `RtsWorld::tick` is skipped from; there is no separate
    /// `paused: bool` anymore.
    pub(crate) ui: RtsUiState,
    /// The live settings value: what was loaded at startup, updated in
    /// place by every successful `rts_ui::commit_setting_change`.
    pub(crate) settings: RtsSettings,
    /// Set by `rts_ui::handle_modal_click` when a modal click changed a
    /// value; drained (and committed) by the interactive left-click site
    /// that owns the window/store `rts_ui::commit_setting_change` needs.
    pub(crate) pending_setting_change: Option<SettingsChange>,
    overlay_visible: bool,
    quit: bool,
    /// Reused scratch for `RtsWorld::issue_context_order_at`.
    receipts: OrderReceiptBuffer,
    /// Armed by `CommandId::SetRally`: the building whose rally point the
    /// *next* world left-click (not one over the HUD) sets.
    pub(crate) pending_rally: Option<EntityId>,
    /// Where every derived [`AudioEvent`] goes (`T15`). Established once at
    /// startup; the scripted and live paths share it, so the two cannot emit
    /// different sound for the same action. Never a physical device in an
    /// offscreen run — `T16` owns the SDL sink.
    pub(crate) audio: Box<dyn AudioSink>,
    /// Sink-independent tally of what this run emitted, reported on the
    /// `rts: audio` line so a headless run can be asserted against.
    pub(crate) audio_counters: AudioCounters,
    /// Guards the one-per-session `StartMusic`.
    music_started: bool,
    /// Set by [`Self::emit_audio`]/[`Self::publish_gains`]/[`Self::maintain_audio`]
    /// when the live sink itself failed (never [`BufferedAudioSink`] or
    /// [`FakeAudioSink`], which cannot). The interactive loop treats this as
    /// fatal (T16): release the window, exit 1 with `e`'s operation/asset
    /// context. Latched, not overwritten, so the *first* failure is the one
    /// reported.
    pub(crate) audio_fatal: Option<AudioError>,
    /// Fixed scratch for the selection delta: the selected player units
    /// before and after one pointer action. Reserved to the selection's own
    /// cap, so a delta allocates nothing.
    selection_before: Vec<EntityId>,
    selection_after: Vec<EntityId>,
}

impl Default for RtsSession {
    fn default() -> Self {
        Self::with_sink(Box::new(FakeAudioSink::new()))
    }
}

impl RtsSession {
    /// A session wired to `audio`. The only constructor: the sink is
    /// established once, at startup, and never swapped mid-run.
    pub(crate) fn with_sink(audio: Box<dyn AudioSink>) -> Self {
        Self {
            cursor: [0.0, 0.0],
            press: None,
            press_owner: PointerOwner::None,
            press_control: None,
            drag: None,
            keyboard_held: [0.0, 0.0],
            ui: RtsUiState::default(),
            settings: RtsSettings::default(),
            pending_setting_change: None,
            overlay_visible: false,
            quit: false,
            receipts: OrderReceiptBuffer::new(),
            pending_rally: None,
            audio,
            audio_counters: AudioCounters::default(),
            music_started: false,
            audio_fatal: None,
            selection_before: Vec::with_capacity(MAX_SELECTION),
            selection_after: Vec::with_capacity(MAX_SELECTION),
        }
    }

    /// Logical cursor position the HUD packer tints hover from.
    pub(crate) fn cursor_logical(&self) -> [f32; 2] {
        self.cursor
    }

    /// Control currently held under the left button, if any.
    pub(crate) fn pressed_control(&self) -> Option<mmd_engine::rts::ControlId> {
        self.press_control
    }

    /// Clear retained pointer-down state (focus loss, content-bar cancel).
    pub(crate) fn clear_press(&mut self) {
        self.press = None;
        self.press_owner = PointerOwner::None;
        self.press_control = None;
        self.drag = None;
    }

    /// Count and forward one derived event. A sink failure latches into
    /// [`Self::audio_fatal`] (T16 owns the interactive fatal-exit policy);
    /// [`FakeAudioSink`]/[`BufferedAudioSink`] never fail, so this is only
    /// ever reachable with a live `SdlAudioSink`.
    pub(crate) fn emit_audio(&mut self, event: AudioEvent) {
        self.audio_counters.record(&event);
        if let Err(e) = self.audio.emit(event) {
            self.audio_fatal.get_or_insert(e);
        }
    }

    /// Per-rendered-frame sink upkeep (music refill, T16). Same fatal-latch
    /// discipline as [`Self::emit_audio`].
    pub(crate) fn maintain_audio(&mut self) {
        if let Err(e) = self.audio.maintain() {
            self.audio_fatal.get_or_insert(e);
        }
    }

    /// Take the latched fatal sink error, if any, clearing it.
    pub(crate) fn take_audio_fatal(&mut self) -> Option<AudioError> {
        self.audio_fatal.take()
    }

    /// The one music start of this session. Idempotent: nothing — menu,
    /// focus loss, a second call — ever restarts or stops it.
    pub(crate) fn start_music(&mut self) {
        if self.music_started {
            return;
        }
        self.music_started = true;
        self.emit_audio(AudioEvent::StartMusic);
    }

    /// Push the current effective gains at the sink.
    fn publish_gains(&mut self) {
        let gains = effective_gains(&self.settings.audio);
        if let Err(e) = self.audio.set_gains(gains) {
            self.audio_fatal.get_or_insert(e);
        }
    }
}

#[cfg(test)]
impl RtsSession {
    /// A session plus a handle on the recording sink it was built with, for
    /// app unit tests.
    pub(crate) fn for_test() -> (Self, crate::rts_feedback::FakeSinkHandle) {
        let handle = crate::rts_feedback::FakeSinkHandle::new();
        (Self::with_sink(handle.boxed()), handle)
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

/// Snapshot the selected player units before a selection-capable action.
fn selection_snapshot(world: &RtsWorld, session: &mut RtsSession) {
    snapshot_selected_units(world, &mut session.selection_before);
}

/// Voice whatever that action newly selected (`T15`): the sorted set
/// difference against [`selection_snapshot`]'s snapshot, capped globally.
fn emit_selection_cues(world: &RtsWorld, session: &mut RtsSession) {
    snapshot_selected_units(world, &mut session.selection_after);
    let batch = selection_cues(&session.selection_before, &session.selection_after);
    if let Some(batch) = batch {
        session.emit_audio(AudioEvent::Voice(batch));
    }
}

/// Record pointer-down owner + stable control id (`T1`).
pub(crate) fn pointer_down(world: &RtsWorld, session: &mut RtsSession, p: [f32; 2]) {
    session.cursor = p;
    session.press = Some(p);
    let owner = crate::rts_ui::owner_for_point(world, &session.ui, p);
    session.press_owner = owner;
    session.press_control = match owner {
        PointerOwner::Hud(hit) => control_id_from_hud_hit(world, hit),
        PointerOwner::Modal(hit) => control_id_from_modal_hit(hit),
        PointerOwner::World | PointerOwner::None => None,
    };
}

/// Whether release may activate given retained down identity (`T1`).
fn activation_matches(world: &RtsWorld, session: &RtsSession, up_owner: PointerOwner) -> bool {
    match session.press_control {
        Some(down_id) => match up_owner {
            PointerOwner::Hud(hit) => control_id_from_hud_hit(world, hit) == Some(down_id),
            PointerOwner::Modal(hit) => control_id_from_modal_hit(hit) == Some(down_id),
            PointerOwner::World | PointerOwner::None => false,
        },
        // No discrete control on down: only world/minimap-style owners activate
        // when the release stays on the same owner kind (never modal/HUD leak).
        None => match (session.press_owner, up_owner) {
            (PointerOwner::World, PointerOwner::World) => true,
            (
                PointerOwner::Hud(mmd_engine::rts::HudHit::Minimap(_)),
                PointerOwner::Hud(mmd_engine::rts::HudHit::Minimap(_)),
            ) => true,
            _ => false,
        },
    }
}

/// Activate one world left-click at `p` (select / place / rally).
fn activate_world_left(world: &mut RtsWorld, session: &mut RtsSession, p: [f32; 2]) {
    if let Some(building) = session.pending_rally.take() {
        let view = world.iso_view();
        let width = world.scenario().width();
        let height = world.scenario().height();
        let cell = view.cell_at(p[0], p[1], width, height);
        let _ = world.set_rally(building, cell);
    } else if let Placement::Pending { kind } = world.placement() {
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

/// Pointer-up activation using retained down owner/control (`T1`).
pub(crate) fn pointer_up(world: &mut RtsWorld, session: &mut RtsSession, p: [f32; 2], shift: bool) {
    session.cursor = p;
    selection_snapshot(world, session);
    let up_owner = crate::rts_ui::owner_for_point(world, &session.ui, p);
    let down_owner = session.press_owner;
    let matched = activation_matches(world, session, up_owner);
    // Clear retained press before handlers (they may open menus / change page).
    session.press = None;
    session.press_owner = PointerOwner::None;
    session.press_control = None;
    session.drag = None;

    if !matched {
        // Modal/HUD down never leaks to world after motion; mismatched IDs no-op.
        emit_selection_cues(world, session);
        return;
    }

    match down_owner {
        PointerOwner::Modal(_) => {
            if let PointerOwner::Modal(hit) = up_owner {
                crate::rts_ui::handle_modal_click(session, hit);
            }
        }
        PointerOwner::Hud(_) => {
            if let PointerOwner::Hud(hit) = up_owner {
                crate::rts_ui::handle_hud_click(world, session, hit, shift);
            }
        }
        PointerOwner::World => {
            if shift {
                let view = world.iso_view();
                world.shift_click_select(&view, p);
            } else {
                activate_world_left(world, session, p);
            }
        }
        PointerOwner::None => {}
    }
    emit_selection_cues(world, session);
}

/// Apply one [`RtsCommand`] to `world`/`session`. Shared by the live SDL path
/// and the scripted path, so the two cannot drift — including the audio
/// events derived from each command's own receipts (`T15`).
pub(crate) fn apply(world: &mut RtsWorld, session: &mut RtsSession, cmd: RtsCommand) {
    match cmd {
        RtsCommand::Quit => session.quit = true,
        RtsCommand::Escape => session.ui.handle_escape(),
        RtsCommand::TogglePause => session.ui.toggle_manual_pause(),
        RtsCommand::ToggleOverlay => session.overlay_visible = !session.overlay_visible,
        RtsCommand::CancelPlacement => world.cancel_placement(),
        RtsCommand::Execute(id) => crate::rts_ui::execute_command(world, session, id),
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
        // Scripted lclick synthesizes atomic down/up on the same control (`T1`).
        RtsCommand::LeftClick(p) => {
            pointer_down(world, session, p);
            pointer_up(world, session, p, false);
        }
        RtsCommand::ShiftClick(p) => {
            pointer_down(world, session, p);
            pointer_up(world, session, p, true);
        }
        RtsCommand::Drag(a, b) => {
            // Scripted drag: only a world-origin press becomes a box select.
            pointer_down(world, session, a);
            selection_snapshot(world, session);
            if matches!(session.press_owner, PointerOwner::World) {
                let view = world.iso_view();
                world.box_select_into_selection(&view, a, b);
            }
            session.clear_press();
            emit_selection_cues(world, session);
        }
        RtsCommand::RightClick(p) => {
            let owner = crate::rts_ui::owner_for_point(world, &session.ui, p);
            if matches!(owner, PointerOwner::Modal(_) | PointerOwner::Hud(_)) {
                // Consumed: a right click over a modal/the HUD is never a
                // world order.
            } else if matches!(world.placement(), Placement::Pending { .. }) {
                // A right click while a ghost is pending cancels it instead
                // of issuing an order.
                world.cancel_placement();
            } else {
                let view = world.iso_view();
                let result = world.issue_context_order_at(&view, p, &mut session.receipts);
                // One batch for every accepted order and at most one reject
                // for the whole action — accepted overflow past the cap is a
                // cap, not a rejection.
                let batch = order_cues(session.receipts.as_slice());
                if let Some(batch) = batch {
                    session.emit_audio(AudioEvent::Voice(batch));
                }
                if let Some(event) = reject_cue(&result) {
                    session.emit_audio(event);
                }
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

/// Resolves validated settings once, before interactive window init, along
/// with the [`SettingsStore`] a later [`crate::rts_ui::commit_setting_change`]
/// saves through — `None` under the same conditions the load itself falls
/// back to defaults (offscreen, or an unresolvable pref path).
///
/// A non-interactive run ([`headless_driver`]) always uses
/// [`RtsSettings::default`] and never resolves [`SettingsStore::pref_path`]
/// (which creates the real per-user pref directory as a side effect of being
/// called) — the hard isolation constraint this ticket exists to prove. A
/// settings-menu edit can therefore change values in memory during such a
/// run, but never persists one: there is no store to save through.
fn load_settings(opts: &RtsOptions, headless_driver: bool) -> (RtsSettings, Option<SettingsStore>) {
    if headless_driver {
        return (RtsSettings::default(), None);
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
            (loaded.value, Some(store))
        }
        Err(e) => {
            println!(
                "rts: settings warning={}",
                escape_warning(&format!("pref path unavailable: {e}"))
            );
            (RtsSettings::default(), None)
        }
    }
}

/// Strip the parts of a loaded settings value a **deterministic replay** must
/// not depend on.
///
/// [`RtsWorld::state_hash`] covers the camera centre, and the camera's pan
/// speeds feed it through the world's own camera system. A non-interactive run
/// always uses [`RtsSettings::default`] (48/48), so a windowed run of the same
/// script that read *this machine's* persisted `camera.keyboard_pan` /
/// `camera.edge_pan` ended at a different centre and therefore a different
/// state hash — for the same script. The tracked acceptance run's whole claim
/// is that it is live-equivalent (ADR 016), and a hash that depends on whose
/// machine it ran on is not.
///
/// So a scripted run:
/// - takes the **default** camera speeds, whatever is persisted — the only
///   settings that reach hashed world state;
/// - keeps every other setting (display, gameplay, audio), which reach no
///   hashed state, so a windowed replay still honours this user's window mode
///   and pointer confinement;
/// - has **no store**, so a replay's own scripted settings edits are never
///   written back over the user's file.
///
/// An interactive (unscripted) run is untouched: nothing compares its hash to
/// anything, and the persisted camera speed is the point of the setting.
fn replay_settings(
    mut settings: RtsSettings,
    store: Option<SettingsStore>,
    scripted: bool,
) -> (RtsSettings, Option<SettingsStore>) {
    if !scripted {
        return (settings, store);
    }
    settings.camera = CameraSettings::default();
    (settings, None)
}

/// SDL video drivers that never drive a real display, and therefore never a
/// real user: `offscreen` (the deterministic gate driver) and `dummy` (SDL's
/// own no-op driver, which CI images and `nix flake check` sandboxes pick up).
///
/// Both are treated as fully isolated: no per-user settings are read or
/// written, no window mode is applied, no pointer is grabbed. Matching only
/// the exact string `offscreen` — as this used to — left `dummy` resolving the
/// developer's real `SDL_GetPrefPath`, which creates the pref directory just
/// by being called.
fn headless_driver() -> bool {
    std::env::var_os("SDL_VIDEODRIVER")
        .map(|v| v == "offscreen" || v == "dummy")
        .unwrap_or(false)
}

/// Run the `rts` subcommand.
pub fn run(opts: RtsOptions) -> Result<(), RunError> {
    // Detected once, up front, and reused for both the settings lookup and the
    // later window-vs-offscreen fork: a non-interactive run must never resolve
    // the real per-user pref path (`SettingsStore::pref_path` creates it as a
    // side effect of being called), so this check has to gate the settings
    // lookup, not just window creation.
    let headless_driver = headless_driver();

    let root = workspace_root_or_cwd();
    let scenario_path = opts
        .scenario
        .clone()
        .unwrap_or_else(|| root.join("assets/scenarios/rts_prototype_v1.ron"));

    // Flag validation first: a typo must not cost a device init — nor a
    // per-user pref directory. `rts --frames 0` used to create the real one
    // before rejecting the flag, so the settings lookup sits *after* every
    // flag has been validated, not before.
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

    let (settings, settings_store) = load_settings(&opts, headless_driver);
    let (settings, settings_store) = replay_settings(
        settings,
        settings_store,
        opts.inject_input.is_some() || opts.inject_input_file.is_some(),
    );

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

    // Before frame 1's window-vs-offscreen decision, every audio call goes
    // through a `BufferedAudioSink` (T16 startup ordering, ADR 018/020):
    // `audio_replay` is a second owner of the same recorded log, replayed
    // once into whichever real sink the fork below picks.
    let buffered_audio = BufferedAudioSink::new();
    let audio_replay = buffered_audio.handle();
    let mut session = RtsSession {
        settings: settings.clone(),
        ..RtsSession::with_sink(Box::new(buffered_audio))
    };
    // Gains before the first event, and the one music start of this session
    // before frame 1 — music then runs logically forever: nothing (menu,
    // pause, focus loss, window loss) ever stops it.
    session.publish_gains();
    session.start_music();
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

    let window = if headless_driver {
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
        // No real window: fold the buffered startup log into a
        // `FakeAudioSink` (T16) — no physical device, no user audio config,
        // ever touched on this path. `FakeAudioSink::emit`/`maintain` never
        // fail, so this replay cannot fail either.
        let mut fake = FakeAudioSink::new();
        audio_replay
            .replay_into(&mut fake)
            .expect("FakeAudioSink never fails a replay");
        session.audio = Box::new(fake);
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

    // A real window is claimed: build the SDL sink and replay the buffered
    // startup log into it before the first present (T16 startup ordering).
    // Any load/open/create/bind/gain/queue failure here releases the
    // already-claimed window and exits 1 — no offscreen fallback for an
    // audio failure on the interactive path.
    match SdlAudioSink::open(&renderer.ctx.sdl, &root.join("assets/audio/generated")) {
        Ok(mut sdl_sink) => {
            if let Err(e) = audio_replay.replay_into(&mut sdl_sink) {
                release_window(&renderer, window);
                return Err(RunError::Failed(format!(
                    "interactive audio startup failed (replay): {e}"
                )));
            }
            session.audio = Box::new(sdl_sink);
        }
        Err(e) => {
            release_window(&renderer, window);
            return Err(RunError::Failed(format!(
                "interactive audio startup failed (device init): {e}"
            )));
        }
    }

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
        // syscall pair on batches that changed nothing. Mutable because a
        // window-mode change committed *inside* this batch resizes the window
        // and hands back a fresh viewport the rest of the batch must use.
        let mut viewport = win_state.viewport;

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
                            session.settings.display.confine_pointer,
                            session.settings.gameplay.pause_on_focus_loss,
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
                            session.settings.display.confine_pointer,
                            session.settings.gameplay.pause_on_focus_loss,
                            || {
                                session.keyboard_held = [0.0, 0.0];
                                session.clear_press();
                                world.set_keyboard_pan_dir([0.0, 0.0]);
                                world.set_edge_pan_dir([0.0, 0.0]);
                            },
                        ) {
                            Ok(FocusAction::PauseRequested) => session.ui.focus_lost(),
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
                    if mapped.inside_content {
                        pointer_down(&world, &mut session, mapped.logical);
                    } else {
                        session.clear_press();
                    }
                }
                Event::MouseButtonUp {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    let mapped = viewport.map_pointer([x, y]);
                    if mapped.inside_content && session.press.is_some() {
                        let end = mapped.logical;
                        let shift = {
                            let ks = pump.keyboard_state();
                            ks.is_scancode_pressed(Scancode::LShift)
                                || ks.is_scancode_pressed(Scancode::RShift)
                        };
                        // World-origin drag becomes a box select; otherwise
                        // matching down/up control activates (`T1`).
                        if !shift
                            && matches!(session.press_owner, PointerOwner::World)
                            && session.press.is_some_and(|a| is_drag(a, end))
                        {
                            let start = session.press.unwrap();
                            selection_snapshot(&world, &mut session);
                            let view = world.iso_view();
                            world.box_select_into_selection(&view, start, end);
                            session.clear_press();
                            emit_selection_cues(&world, &mut session);
                        } else {
                            pointer_up(&mut world, &mut session, end, shift);
                        }
                        if let Some(change) = session.pending_setting_change.take() {
                            // Through the claim-aware seam: a window-mode
                            // change must run with the GPU claim released and
                            // retaken, and invalidates the viewport.
                            let live = {
                                let mut ops = SdlClaimedWindow {
                                    window: &mut window,
                                    renderer: &renderer,
                                };
                                crate::rts_ui::commit_setting_change_live(
                                    &mut world,
                                    &mut ops,
                                    settings_store.as_ref(),
                                    &mut session.settings,
                                    session.audio.as_mut(),
                                    change,
                                )
                            };
                            let live = match live {
                                Ok(live) => live,
                                // No presentable window left: fatal, not a
                                // "SETTINGS NOT SAVED" warning.
                                Err(e) => {
                                    release_window(&renderer, window);
                                    return Err(RunError::Failed(e));
                                }
                            };
                            if let Some(v) = live.viewport {
                                win_state.viewport = v;
                                viewport = v;
                            }
                            match live.result {
                                Ok(()) => {
                                    session.ui.warning = None;
                                    win_state.mode = session.settings.display.mode;
                                }
                                Err(reason) => {
                                    session.ui.warning =
                                        Some(format!("SETTINGS NOT SAVED: {reason}"));
                                }
                            }
                        }
                    } else {
                        // Release in a bar: no click/drag/order, per contract.
                        session.clear_press();
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

        // A live `SdlAudioSink` call inside the event handling above (e.g.
        // a UI click's cue) can fail; that is a fatal interactive error
        // (T16) — release the window and exit 1 with the sink's own
        // operation/asset context.
        if let Some(e) = session.take_audio_fatal() {
            release_window(&renderer, window);
            return Err(RunError::Failed(format!("interactive audio failure: {e}")));
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
            Ok(Some(_)) => {
                if let Some(e) = session.take_audio_fatal() {
                    release_window(&renderer, window);
                    return Err(RunError::Failed(format!("interactive audio failure: {e}")));
                }
            }
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
        commit_scripted_setting_change(world, session);
    }
    if session.quit {
        state.quit = true;
        return Ok(None);
    }

    if !session.ui.sim_paused() {
        world.tick();
    }

    pack_frame(world, session.cursor, session.drag, frame_buf);
    crate::rts_ui::pack_hud(world, session, frame_buf);

    draw(frame_buf.scene())?;

    // Per-rendered-frame sink upkeep (music refill in `T16`); never emits,
    // so it cannot change what a run heard.
    session.maintain_audio();

    let hash = world.state_hash();
    if frame == 1 {
        state.first_hash = hash;
    }
    state.frames = frame;
    state.last_hash = hash;
    // Counted per frame rather than latched: a run that pauses and then
    // unpauses must go back to owing one tick per frame.
    if !session.ui.sim_paused() {
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

/// Drain a scripted click's pending settings edit (`T17`).
///
/// The live path drains it at the `MouseButtonUp` site, which owns the
/// window and the store [`crate::rts_ui::commit_setting_change`] needs. A
/// scripted click reaches [`apply`] with neither, so it commits **in memory
/// only**: it validates, applies the camera-speed runtime and publishes the
/// new value into `session.settings`, but never touches a window and never
/// writes a user file. That is exactly the offscreen-isolation contract the
/// deterministic acceptance run needs — a scripted settings edit must be
/// observable on the exit line without ever becoming a side effect on the
/// developer's machine.
///
/// A refused edit leaves `session.ui.warning` set, exactly as the live path
/// does, so a script cannot silently apply an illegal value.
fn commit_scripted_setting_change(world: &mut RtsWorld, session: &mut RtsSession) {
    let Some(change) = session.pending_setting_change.take() else {
        return;
    };
    let outcome = crate::rts_ui::commit_setting_change::<SdlWindowOps<'_>>(
        world,
        None,
        None,
        &mut session.settings,
        session.audio.as_mut(),
        change,
    );
    match outcome {
        Ok(()) => session.ui.warning = None,
        Err(reason) => session.ui.warning = Some(format!("SETTINGS NOT SAVED: {reason}")),
    }
}

/// Release the window from the device, then drop it — in that order.
/// The live window plus the GPU device holding its claim — the real
/// [`ClaimedWindow`] a settings-menu window-mode change commits through.
///
/// Every [`WindowOps`] call delegates to [`SdlWindowOps`], so the mode
/// sequences themselves stay exactly as they are; this type only adds the
/// release/reclaim/viewport half `rts_ui::commit_setting_change_live` needs.
struct SdlClaimedWindow<'a> {
    window: &'a mut sdl3::video::Window,
    renderer: &'a SpriteRenderer,
}

impl WindowOps for SdlClaimedWindow<'_> {
    fn leave_fullscreen(&mut self) -> Result<(), String> {
        SdlWindowOps(self.window).leave_fullscreen()
    }
    fn enter_fullscreen(&mut self) -> Result<(), String> {
        SdlWindowOps(self.window).enter_fullscreen()
    }
    fn clear_exclusive_mode(&mut self) -> Result<(), String> {
        SdlWindowOps(self.window).clear_exclusive_mode()
    }
    fn available_modes(&mut self) -> Result<Vec<ModeCandidate>, String> {
        SdlWindowOps(self.window).available_modes()
    }
    fn set_exclusive_mode(&mut self, mode: ModeCandidate) -> Result<(), String> {
        SdlWindowOps(self.window).set_exclusive_mode(mode)
    }
    fn set_bordered(&mut self, bordered: bool) -> Result<(), String> {
        SdlWindowOps(self.window).set_bordered(bordered)
    }
    fn set_size(&mut self, w: u32, h: u32) -> Result<(), String> {
        SdlWindowOps(self.window).set_size(w, h)
    }
    fn center(&mut self) -> Result<(), String> {
        SdlWindowOps(self.window).center()
    }
    fn sync(&mut self) -> Result<(), String> {
        SdlWindowOps(self.window).sync()
    }
    fn set_mouse_grab(&mut self, grabbed: bool) -> Result<(), String> {
        SdlWindowOps(self.window).set_mouse_grab(grabbed)
    }
}

impl ClaimedWindow for SdlClaimedWindow<'_> {
    fn release_claim(&mut self) {
        self.renderer.ctx.release_window(self.window);
    }

    fn reclaim(&mut self) -> Result<(), String> {
        self.renderer
            .ctx
            .claim_window(self.window)
            .map_err(|e| e.to_string())
    }

    fn viewport(&self) -> Result<DisplayViewport, String> {
        rts_window::refresh_viewport(self.window).map_err(|e| e.to_string())
    }
}

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

    let counters = session.audio_counters;
    let gains = effective_gains(&session.settings.audio);
    println!(
        "rts: audio music={} voice={} cues={} reject={} ui={} gains={}/{}/{}",
        counters.music,
        counters.voice,
        counters.cues,
        counters.reject,
        counters.ui,
        gains.for_bus(AudioBus::Music),
        gains.for_bus(AudioBus::Voice),
        gains.for_bus(AudioBus::Sfx),
    );

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
         camera={},{} body_overlaps={} ui_page={} music_starts={} voice_select={} \
         voice_order={} voice_reject={} sfx_ui={} keyboard_pan={}",
        world.tick_index(),
        state.frames,
        hex::encode(state.last_hash),
        state.quit,
        session.ui.sim_paused(),
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
        world.body_overlap_count(),
        session.ui.page.label(),
        counters.music,
        counters.select_cues,
        counters.order_cues,
        counters.reject,
        counters.ui,
        session.settings.camera.keyboard_pan,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn persisted_non_default() -> RtsSettings {
        let mut s = RtsSettings::default();
        s.camera.keyboard_pan = 24;
        s.camera.edge_pan = 96;
        s.display.mode = crate::rts_settings::WindowMode::Windowed1280x720;
        s.audio.master = 55;
        s
    }

    /// The determinism hole this closes: a scripted run's camera speeds are
    /// the only settings that reach `RtsWorld::state_hash`, and an offscreen
    /// run has always used the defaults. A windowed replay of the same script
    /// that read this machine's persisted speeds hashed differently.
    #[test]
    fn a_scripted_run_takes_the_default_camera_speeds() {
        let (settings, store) = replay_settings(persisted_non_default(), None, true);
        assert_eq!(
            settings.camera,
            CameraSettings::default(),
            "a replay's camera speeds must not depend on whose machine it runs on"
        );
        assert!(
            store.is_none(),
            "a replay must not write its own scripted values back over the user's file"
        );
    }

    /// Everything that reaches no hashed state stays the user's, so a windowed
    /// replay still honours their window mode, confinement and volumes.
    #[test]
    fn a_scripted_run_keeps_every_setting_that_reaches_no_hashed_state() {
        let (settings, _) = replay_settings(persisted_non_default(), None, true);
        let persisted = persisted_non_default();
        assert_eq!(settings.display, persisted.display);
        assert_eq!(settings.gameplay, persisted.gameplay);
        assert_eq!(settings.audio, persisted.audio);
    }

    /// An interactive run is untouched: nothing compares its hash to anything,
    /// and the persisted pan speed is the whole point of the setting.
    #[test]
    fn an_unscripted_run_keeps_the_persisted_camera_speeds() {
        let (settings, _) = replay_settings(persisted_non_default(), None, false);
        assert_eq!(settings.camera, persisted_non_default().camera);
    }
}
