//! Pointer ownership, the shared command executor, and the nested paused
//! menu / settings FSM — `T12` + `T13`.
//!
//! A HUD click is resolved here, before the world ever sees it: its
//! [`HudHit`] decides whether it drives a card/icon/minimap action or falls
//! through to `RtsWorld`'s own click/order path. Keyboard hotkeys and
//! command-grid clicks both resolve through [`execute_command`], so the two
//! input paths cannot drift.
//!
//! [`RtsUiState`] adds one more layer on top: while its `page` is not
//! [`UiPage::Gameplay`] an open modal owns *every* pointer point
//! ([`owner_for_point`] returns [`PointerOwner::Modal`]), so a click can
//! never leak through as a world order or a HUD action while the game is
//! paused for the menu.

use mmd_engine::rts::{
    AudioChannelId, BuildingKind, CommandId, ControlId, HudHit, HudLayout, InteractionSnapshot,
    ModalHit, ModalPage, ModalSnapshot, NumericSettingId, RtsWorld, UnitKind, clamp_snap,
    command_slots, control_id_from_hud_hit, control_id_from_modal_hit, hud_hit_test,
    minimap_projection, modal_hit_test, numeric_id_from_slider_control, pack_hud_interactive,
    pack_modal_interactive, snap_numeric_at_x,
};

use crate::rts_feedback::{AudioEvent, AudioSink, UiCue, effective_gains};
use crate::rts_run::RtsSession;
use crate::rts_settings::{RtsSettings, SettingsStore, WindowMode};
use crate::rts_window::{self, ClaimedWindow, ModeChangeOutcome, WindowOps};
use mmd_engine::render::DisplayViewport;

/// Which side of the input boundary a pointer gesture belongs to.
///
/// [`Self::None`] is the idle state before any point has been classified.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum PointerOwner {
    #[default]
    None,
    World,
    Hud(HudHit),
    /// An open pause menu / settings modal owns this point — never the
    /// world, never the normal HUD.
    Modal(ModalHit),
}

/// The nested menu pages `T13` adds on top of plain gameplay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiPage {
    Gameplay,
    PauseMenu,
    Settings,
}

impl UiPage {
    /// The `ui_page=` token of the `rts` exit line (`T17`). Stable, lower
    /// snake case, one word per page — never the `Debug` spelling.
    pub fn label(self) -> &'static str {
        match self {
            Self::Gameplay => "gameplay",
            Self::PauseMenu => "pause_menu",
            Self::Settings => "settings",
        }
    }
}

/// Every independent reason the sim can be paused. `manual` (Space) and
/// `menu`/`focus` (the paused-menu FSM) are tracked separately so closing
/// the menu never accidentally resumes a manual pause, and vice versa.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PauseReasons {
    pub manual: bool,
    pub menu: bool,
    pub focus: bool,
}

impl PauseReasons {
    pub fn any(&self) -> bool {
        self.manual || self.menu || self.focus
    }
}

/// One accepted, transactionally-committed settings edit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SettingsChange {
    WindowMode(WindowMode),
    KeyboardPan(u32),
    EdgePan(u32),
    Confine(bool),
    PauseOnFocusLoss(bool),
    Master(u32),
    Music(u32),
    Voice(u32),
    Sfx(u32),
    MasterMuted(bool),
    MusicMuted(bool),
    VoiceMuted(bool),
    SfxMuted(bool),
}

impl SettingsChange {
    pub(crate) fn apply_to(self, settings: &mut RtsSettings) {
        match self {
            Self::WindowMode(mode) => settings.display.mode = mode,
            Self::KeyboardPan(v) => settings.camera.keyboard_pan = v,
            Self::EdgePan(v) => settings.camera.edge_pan = v,
            Self::Confine(v) => settings.display.confine_pointer = v,
            Self::PauseOnFocusLoss(v) => settings.gameplay.pause_on_focus_loss = v,
            Self::Master(v) => settings.audio.master = v,
            Self::Music(v) => settings.audio.music = v,
            Self::Voice(v) => settings.audio.voice = v,
            Self::Sfx(v) => settings.audio.sfx = v,
            Self::MasterMuted(v) => settings.audio.master_muted = v,
            Self::MusicMuted(v) => settings.audio.music_muted = v,
            Self::VoiceMuted(v) => settings.audio.voice_muted = v,
            Self::SfxMuted(v) => settings.audio.sfx_muted = v,
        }
    }
}

/// [`SettingsChange`] for one snapped numeric row value.
pub fn settings_change_for_numeric(id: NumericSettingId, value: u32) -> SettingsChange {
    match id {
        NumericSettingId::KeyboardPan => SettingsChange::KeyboardPan(value),
        NumericSettingId::EdgePan => SettingsChange::EdgePan(value),
        NumericSettingId::Master => SettingsChange::Master(value),
        NumericSettingId::Music => SettingsChange::Music(value),
        NumericSettingId::Voice => SettingsChange::Voice(value),
        NumericSettingId::Sfx => SettingsChange::Sfx(value),
    }
}

/// Live numeric value for one settings row.
pub fn current_numeric_value(settings: &RtsSettings, id: NumericSettingId) -> u32 {
    match id {
        NumericSettingId::KeyboardPan => settings.camera.keyboard_pan,
        NumericSettingId::EdgePan => settings.camera.edge_pan,
        NumericSettingId::Master => settings.audio.master,
        NumericSettingId::Music => settings.audio.music,
        NumericSettingId::Voice => settings.audio.voice,
        NumericSettingId::Sfx => settings.audio.sfx,
    }
}

/// Fixed no-heap edit buffer for one typed numeric field (`T4`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumericEdit {
    pub id: NumericSettingId,
    pub original: u32,
    digits: [u8; 3],
    len: u8,
}

impl NumericEdit {
    /// Begin edit on `id`, seeding the buffer with `original`'s decimal digits.
    pub fn begin(id: NumericSettingId, original: u32) -> Self {
        let mut digits = [0u8; 3];
        let len = write_u32_3(&mut digits, original);
        Self {
            id,
            original,
            digits,
            len,
        }
    }

    /// Push one ASCII digit. Non-digits and a full buffer are ignored.
    pub fn push_ascii_digit(&mut self, c: u8) -> bool {
        if !c.is_ascii_digit() || self.len >= 3 {
            return false;
        }
        self.digits[self.len as usize] = c;
        self.len += 1;
        true
    }

    /// Drop the last digit, if any.
    pub fn backspace(&mut self) {
        self.len = self.len.saturating_sub(1);
    }

    /// Parsed decimal value, or `None` when the buffer is empty.
    pub fn parsed(&self) -> Option<u32> {
        if self.len == 0 {
            return None;
        }
        let mut v = 0u32;
        for &d in &self.digits[..self.len as usize] {
            v = v * 10 + u32::from(d - b'0');
        }
        Some(v)
    }

    /// ASCII digit bytes currently in the buffer (length 0..=3).
    pub fn display(&self) -> &[u8] {
        &self.digits[..self.len as usize]
    }

    /// Drop the edit; caller keeps `original` already stored in settings.
    pub fn cancel(self) -> u32 {
        self.original
    }
}

/// How [`finish_numeric_edit`] ends an active field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericEditEnd {
    /// Enter / pointer blur / OS focus loss: parse → clamp_snap → maybe stage.
    Commit,
    /// Escape: restore original (already in settings), no stage.
    Cancel,
}

/// Write `v` as up to 3 decimal ASCII digits into `out`. Returns length.
fn write_u32_3(out: &mut [u8; 3], mut v: u32) -> u8 {
    if v == 0 {
        out[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 3];
    let mut n = 0u8;
    while v > 0 && n < 3 {
        tmp[n as usize] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    for i in 0..n {
        out[i as usize] = tmp[(n - 1 - i) as usize];
    }
    n
}

/// Begin typing into one numeric field (replaces any prior edit).
pub fn begin_numeric_edit(session: &mut RtsSession, id: NumericSettingId) {
    let original = current_numeric_value(&session.settings, id);
    session.numeric_edit = Some(NumericEdit::begin(id, original));
}

/// Route one SDL `TextInput` string into the active field. Non-digits ignored.
pub fn numeric_edit_text_input(session: &mut RtsSession, text: &str) {
    let Some(edit) = session.numeric_edit.as_mut() else {
        return;
    };
    for b in text.bytes() {
        let _ = edit.push_ascii_digit(b);
    }
}

/// End the active numeric edit. Stages at most one pending change on Commit.
pub fn finish_numeric_edit(session: &mut RtsSession, mode: NumericEditEnd) {
    let Some(edit) = session.numeric_edit.take() else {
        return;
    };
    match mode {
        NumericEditEnd::Cancel => {
            let _ = edit.cancel();
        }
        NumericEditEnd::Commit => {
            let Some(raw) = edit.parsed() else {
                // Empty buffer: restore original / no save.
                return;
            };
            let spec = edit.id.spec();
            let value = clamp_snap(raw, spec.min, spec.max, spec.step);
            if current_numeric_value(&session.settings, edit.id) == value {
                return;
            }
            session.emit_audio(AudioEvent::Ui(UiCue::Settings));
            session.pending_setting_change = Some(settings_change_for_numeric(edit.id, value));
        }
    }
}

/// Focus-loss half of the T4 order before pause policy runs:
/// finalize edit → clear ptr/keys. Caller stops SDL text input, clears world
/// pan dirs, then applies `focus_lost` only when pause-on-focus-loss is on.
pub fn finalize_numeric_edit_on_focus_loss(session: &mut RtsSession) {
    finish_numeric_edit(session, NumericEditEnd::Commit);
    session.clear_press();
    session.keyboard_held = [0.0, 0.0];
}

/// Stage one slider step when the snapped value differs from the live cfg.
///
/// Emits at most one Settings cue per drag (`cue_emitted` latches true on the
/// first staged step). Returns whether a pending change was written.
pub fn stage_slider_step(
    session: &mut RtsSession,
    id: NumericSettingId,
    value: u32,
    cue_emitted: &mut bool,
) -> bool {
    if current_numeric_value(&session.settings, id) == value {
        return false;
    }
    if !*cue_emitted {
        session.emit_audio(AudioEvent::Ui(UiCue::Settings));
        *cue_emitted = true;
    }
    session.pending_setting_change = Some(settings_change_for_numeric(id, value));
    true
}

/// Stage a slider step from pointer x against the retained slider control.
pub fn stage_slider_at_pointer(session: &mut RtsSession, control: ControlId, x: f32) -> bool {
    let Some(id) = numeric_id_from_slider_control(control) else {
        return false;
    };
    let value = snap_numeric_at_x(id, x);
    let mut cue = session.slider_cue_emitted;
    let staged = stage_slider_step(session, id, value, &mut cue);
    session.slider_cue_emitted = cue;
    staged
}

/// `WindowMode` index into `mmd_engine::rts::WINDOW_MODE_BUTTONS`/`_LABELS` —
/// the same order the app declares its own enum in.
fn window_mode_from_index(idx: u8) -> WindowMode {
    match idx {
        0 => WindowMode::BorderlessDesktop,
        1 => WindowMode::Exclusive1920x1080,
        _ => WindowMode::Windowed1280x720,
    }
}

fn window_mode_index(mode: WindowMode) -> u8 {
    match mode {
        WindowMode::BorderlessDesktop => 0,
        WindowMode::Exclusive1920x1080 => 1,
        WindowMode::Windowed1280x720 => 2,
    }
}

/// Menu FSM + pause bookkeeping + the last settings-commit warning, if any.
pub struct RtsUiState {
    pub page: UiPage,
    pub pauses: PauseReasons,
    /// Set by a failed [`commit_setting_change`]; cleared on the next
    /// successful commit or when the settings panel is closed.
    pub warning: Option<String>,
    /// Current settings-body scroll offset in logical pixels (0 = top).
    pub settings_scroll_px: f32,
}

impl Default for RtsUiState {
    fn default() -> Self {
        Self {
            page: UiPage::Gameplay,
            pauses: PauseReasons::default(),
            warning: None,
            settings_scroll_px: 0.0,
        }
    }
}

impl RtsUiState {
    /// Whether `RtsWorld::tick` must be skipped this frame.
    pub fn sim_paused(&self) -> bool {
        self.pauses.any()
    }

    /// Space: toggles the manual pause only. Never touches `page` — a menu
    /// nested on top of a manual pause stays nested exactly as it was.
    pub fn toggle_manual_pause(&mut self) {
        self.pauses.manual = !self.pauses.manual;
    }

    /// Gameplay Menu/Escape: opens the paused menu. A no-op from any other
    /// page — Menu is unreachable once the HUD stops routing clicks to it,
    /// and Escape has its own per-page meaning below.
    pub fn open_menu(&mut self) {
        if self.page == UiPage::Gameplay {
            self.page = UiPage::PauseMenu;
            self.pauses.menu = true;
        }
    }

    /// Escape: Gameplay opens the menu; Settings backs out one level to the
    /// menu; the menu itself closes back to Gameplay (preserving a manual
    /// pause, clearing the focus pause reason — closing the menu is what
    /// clears it per the ticket's focus-toggle contract).
    pub fn handle_escape(&mut self) {
        match self.page {
            UiPage::Gameplay => self.open_menu(),
            UiPage::Settings => self.page = UiPage::PauseMenu,
            UiPage::PauseMenu => self.close_menu(),
        }
    }

    /// The pause menu's one button.
    pub fn open_settings(&mut self) {
        if self.page == UiPage::PauseMenu {
            self.page = UiPage::Settings;
        }
    }

    /// The settings panel's Back control — same one-level-back semantics as
    /// Escape from Settings.
    pub fn settings_back(&mut self) {
        if self.page == UiPage::Settings {
            self.page = UiPage::PauseMenu;
        }
    }

    /// Close the pause menu back to Gameplay. Clears `menu` + `focus` pause
    /// reasons and any settings warning; leaves `manual` unchanged (`T1`).
    pub fn close_menu(&mut self) {
        self.page = UiPage::Gameplay;
        self.pauses.menu = false;
        self.pauses.focus = false;
        self.warning = None;
    }

    /// `pause_on_focus_loss` is set and the window just lost focus: opens
    /// the menu and marks the focus pause reason. The caller (`rts_window`'s
    /// `FocusAction::PauseRequested`) already gates this on the setting
    /// being on.
    pub fn focus_lost(&mut self) {
        self.pauses.focus = true;
        self.page = UiPage::PauseMenu;
    }
}

/// Classify one logical (1920x1080) point. An open modal ([`UiPage::PauseMenu`]
/// / [`UiPage::Settings`]) owns every point; only [`UiPage::Gameplay`] routes
/// through the normal HUD/world split.
pub fn owner_for_point(world: &RtsWorld, ui: &RtsUiState, point: [f32; 2]) -> PointerOwner {
    match ui.page {
        UiPage::Gameplay => match hud_hit_test(world, point) {
            Some(hit) => PointerOwner::Hud(hit),
            None => PointerOwner::World,
        },
        UiPage::PauseMenu => PointerOwner::Modal(modal_hit_test(ModalPage::PauseMenu, point, 0.0)),
        UiPage::Settings => PointerOwner::Modal(modal_hit_test(
            ModalPage::Settings,
            point,
            ui.settings_scroll_px,
        )),
    }
}

/// Run one [`CommandId`] — shared by the command-grid click path (which
/// checks `enabled` itself before calling this) and every keyboard hotkey
/// (which always calls this: a hotkey acts on whatever is actually
/// selected, independent of what the card currently shows).
///
/// A build/produce call that is not currently legal (wrong selection, no
/// resources, no supply) is a no-op here exactly as it always was —
/// `RtsWorld::begin_placement`/`enqueue_unit` already validate and refuse.
pub fn execute_command(world: &mut RtsWorld, session: &mut RtsSession, id: CommandId) {
    match id {
        CommandId::BuildHq => {
            let _ = world.begin_placement(BuildingKind::Hq);
        }
        CommandId::BuildDepot => {
            let _ = world.begin_placement(BuildingKind::Depot);
        }
        CommandId::BuildBarracks => {
            let _ = world.begin_placement(BuildingKind::Barracks);
        }
        CommandId::TrainWorker => {
            if let Some(building) = world.selection().primary() {
                let _ = world.enqueue_unit(building, UnitKind::Worker);
            }
        }
        CommandId::TrainSoldier => {
            if let Some(building) = world.selection().primary() {
                let _ = world.enqueue_unit(building, UnitKind::Soldier);
            }
        }
        CommandId::SetRally => {
            // Arms a pending action rather than acting immediately: the next
            // world left-click (not one over the HUD) sets the cell.
            session.pending_rally = world.selection().primary();
        }
    }
}

/// Execute the command in positional slot `slot` (0–8, row-major QWE/ASD/ZXC).
///
/// Returns `true` when an action actually dispatched. `slot >= 9` returns
/// `false` without panicking. Keyboard callers must not emit a pointer SFX.
pub fn execute_slot(world: &mut RtsWorld, session: &mut RtsSession, slot: u8) -> bool {
    let slots = command_slots(world);
    if let Some(s) = slots.get(slot as usize)
        && s.enabled
        && let Some(cmd) = s.command
    {
        execute_command(world, session, cmd);
        return true;
    }
    false
}

/// Resolve one HUD click. Every variant is consumed here — none of them
/// ever issues a world order or falls through to selection/placement logic.
pub fn handle_hud_click(world: &mut RtsWorld, session: &mut RtsSession, hit: HudHit, shift: bool) {
    match hit {
        HudHit::Menu => {
            session.ui.open_menu();
            session.emit_audio(AudioEvent::Ui(UiCue::Menu));
        }
        HudHit::Minimap(point) => {
            let origin = [HudLayout::MINIMAP_MAP[0], HudLayout::MINIMAP_MAP[1]];
            let local = [point[0] - origin[0], point[1] - origin[1]];
            let projection = minimap_projection(world);
            if let Some(map_point) = projection.minimap_to_map(local) {
                world.look_at_map_point(map_point);
                session.emit_audio(AudioEvent::Ui(UiCue::Minimap));
            }
            // Outside the map diamond: consumed, no move, no cue — per T12's
            // spec.
        }
        HudHit::SelectionIcon(id) => {
            if shift {
                world.toggle_selection(id);
            } else {
                world.select_only(id);
            }
        }
        HudHit::CommandSlot(idx) => {
            if execute_slot(world, session, idx) {
                session.emit_audio(AudioEvent::Ui(UiCue::CommandGrid));
            }
            // Disabled/empty: consumed, no action, no cue.
        }
        HudHit::Background => {}
    }
}

/// Resolve one click inside an open modal. Navigation hits (`OpenSettings`,
/// `Back`) transition `session.ui` directly; a value-changing hit is stashed
/// on `session.pending_setting_change` for the caller to run through
/// [`commit_setting_change`] — this function never touches disk or a window,
/// so it stays usable from the scripted/offscreen path too.
pub fn handle_modal_click(session: &mut RtsSession, hit: ModalHit) {
    // Copied out rather than borrowed: the arms below emit audio through
    // `session`, which needs the whole struct mutably.
    let confine_pointer = session.settings.display.confine_pointer;
    let pause_on_focus_loss = session.settings.gameplay.pause_on_focus_loss;
    let change = match hit {
        ModalHit::OpenSettings => {
            session.ui.open_settings();
            session.emit_audio(AudioEvent::Ui(UiCue::Menu));
            None
        }
        ModalHit::CloseMenu => {
            session.ui.close_menu();
            session.emit_audio(AudioEvent::Ui(UiCue::Menu));
            None
        }
        ModalHit::Back => {
            session.ui.settings_back();
            session.emit_audio(AudioEvent::Ui(UiCue::Menu));
            None
        }
        ModalHit::WindowMode(idx) => Some(SettingsChange::WindowMode(window_mode_from_index(idx))),
        ModalHit::KeyboardPan(v) => Some(SettingsChange::KeyboardPan(v)),
        ModalHit::EdgePan(v) => Some(SettingsChange::EdgePan(v)),
        ModalHit::Confine => Some(SettingsChange::Confine(!confine_pointer)),
        ModalHit::Focus => Some(SettingsChange::PauseOnFocusLoss(!pause_on_focus_loss)),
        ModalHit::Master(v) => Some(SettingsChange::Master(v)),
        ModalHit::Music(v) => Some(SettingsChange::Music(v)),
        ModalHit::Voice(v) => Some(SettingsChange::Voice(v)),
        ModalHit::Sfx(v) => Some(SettingsChange::Sfx(v)),
        ModalHit::ToggleMute(ch) => {
            let audio = &session.settings.audio;
            Some(match ch {
                AudioChannelId::Master => SettingsChange::MasterMuted(!audio.master_muted),
                AudioChannelId::Music => SettingsChange::MusicMuted(!audio.music_muted),
                AudioChannelId::Voice => SettingsChange::VoiceMuted(!audio.voice_muted),
                AudioChannelId::Sfx => SettingsChange::SfxMuted(!audio.sfx_muted),
            })
        }
        ModalHit::NumericField(id) => {
            begin_numeric_edit(session, id);
            None
        }
        ModalHit::ScrollbarTrack(dir) => {
            let viewport_h = mmd_engine::rts::HudLayout::SETTINGS_BODY_VIEWPORT[3];
            let new_offset = session.ui.settings_scroll_px + dir as f32 * viewport_h;
            session.ui.settings_scroll_px = mmd_engine::rts::clamp_settings_scroll(new_offset);
            None
        }
        ModalHit::ScrollbarThumb => {
            // Drag already handled via retained Move; bare click does nothing.
            session.scroll_thumb_drag = None;
            None
        }
        ModalHit::Consumed => None,
    };
    if let Some(change) = change {
        // A control that accepted a new value is a successful, enabled
        // pointer action — the cue belongs here, on the shared path both the
        // live and the scripted click take, not on the live-only commit.
        session.emit_audio(AudioEvent::Ui(UiCue::Settings));
        session.pending_setting_change = Some(change);
    }
}

/// The modal snapshot [`mmd_engine::rts::pack_modal`] reads, built from the
/// live settings value plus [`RtsSession::pending_setting_change`]'s
/// (unsaved-until-commit) candidate, so a slider that just moved never
/// visually snaps back for one frame before its commit lands.
fn modal_snapshot(
    settings: &RtsSettings,
    pending: Option<SettingsChange>,
    scroll_offset: f32,
) -> ModalSnapshot {
    let mut preview = settings.clone();
    if let Some(change) = pending {
        change.apply_to(&mut preview);
    }
    ModalSnapshot {
        window_mode_index: window_mode_index(preview.display.mode),
        keyboard_pan: preview.camera.keyboard_pan,
        edge_pan: preview.camera.edge_pan,
        confine_pointer: preview.display.confine_pointer,
        pause_on_focus_loss: preview.gameplay.pause_on_focus_loss,
        master: preview.audio.master,
        music: preview.audio.music,
        voice: preview.audio.voice,
        sfx: preview.audio.sfx,
        master_muted: preview.audio.master_muted,
        music_muted: preview.audio.music_muted,
        voice_muted: preview.audio.voice_muted,
        sfx_muted: preview.audio.sfx_muted,
        scroll_offset,
    }
}

/// Build the interaction snapshot the HUD/modal packers tint from.
pub fn interaction_snapshot(
    world: &RtsWorld,
    ui: &RtsUiState,
    cursor: [f32; 2],
    pressed: Option<ControlId>,
) -> InteractionSnapshot {
    let hovered = match owner_for_point(world, ui, cursor) {
        PointerOwner::Hud(hit) => control_id_from_hud_hit(world, hit),
        PointerOwner::Modal(hit) => control_id_from_modal_hit(hit),
        PointerOwner::World | PointerOwner::None => None,
    };
    InteractionSnapshot { hovered, pressed }
}

/// App-crate `pack_hud`: packs the world-facing HUD with the session's live
/// interaction snapshot, then — only while a menu is open — appends the modal
/// last, over everything else.
pub fn pack_hud(world: &RtsWorld, session: &RtsSession, frame: &mut mmd_engine::rts::RtsFrame) {
    let interaction = interaction_snapshot(
        world,
        &session.ui,
        session.cursor_logical(),
        session.pressed_control(),
    );
    pack_hud_interactive(world, &interaction, frame);
    let page = match session.ui.page {
        UiPage::Gameplay => return,
        UiPage::PauseMenu => ModalPage::PauseMenu,
        UiPage::Settings => ModalPage::Settings,
    };
    let snapshot = modal_snapshot(
        &session.settings,
        session.pending_setting_change,
        session.ui.settings_scroll_px,
    );
    let active_edit = session.numeric_edit.as_ref().map(|e| (e.id, e.display()));
    pack_modal_interactive(
        page,
        snapshot,
        session.ui.warning.as_deref(),
        &interaction,
        active_edit,
        frame,
    );
}

/// The transactional settings commit, `T13`'s hard contract:
/// 1. clone current settings, apply the change, validate it;
/// 2. apply the runtime adapter (window mode / pointer confinement; camera
///    speeds always apply in-memory);
/// 3. push the candidate's effective gains at the sink (`T15`), so a volume
///    edit is heard immediately rather than at the next restart;
/// 4. save through the `T7` store;
/// 5. publish the candidate into `*settings` only once every prior step
///    succeeded.
///
/// `window` is `None` for an offscreen/headless run — window-mode and
/// pointer-confinement changes then skip their runtime step (nothing to
/// apply it to) but still validate, save and publish. `store` is `None`
/// only when the real pref path could not be resolved at startup; a commit
/// then still applies runtime + publishes in-memory, but cannot persist —
/// this is the same degraded mode `load_settings` already warns about.
///
/// Any runtime, gain or save failure restores the old runtime values (window
/// mode rolls back through [`rts_window::transition_window_mode`]'s own
/// rollback, confinement is re-applied directly, gains are re-pushed at the
/// old value) and returns the failure reason — `*settings` is left untouched,
/// and the caller is expected to show `SETTINGS NOT SAVED: <reason>`.
pub fn commit_setting_change<W: WindowOps>(
    world: &mut RtsWorld,
    mut window: Option<&mut W>,
    store: Option<&SettingsStore>,
    settings: &mut RtsSettings,
    audio: &mut dyn AudioSink,
    change: SettingsChange,
) -> Result<(), String> {
    let old = settings.clone();
    let mut candidate = settings.clone();
    change.apply_to(&mut candidate);
    candidate
        .validate()
        .map_err(|e| format!("refusing to apply invalid settings: {e}"))?;

    apply_runtime(window.as_deref_mut(), &old, &candidate, change)?;

    if let Err(e) = audio.set_gains(effective_gains(&candidate.audio)) {
        let _ = apply_runtime(window, &candidate, &old, change);
        return Err(format!("audio gain failed: {e}"));
    }

    if let Some(store) = store
        && let Err(e) = store.save(&candidate)
    {
        // Runtime already moved to the candidate's values — roll it back to
        // the old ones before reporting the failure, so a failed save never
        // leaves the window (or the mixer) in a state the (unsaved) config
        // disagrees with.
        let _ = audio.set_gains(effective_gains(&old.audio));
        let _ = apply_runtime(window, &candidate, &old, change);
        return Err(format!("save failed: {e}"));
    }

    world.set_camera_speeds(
        candidate.camera.keyboard_pan as f32,
        candidate.camera.edge_pan as f32,
    );
    *settings = candidate;
    Ok(())
}

/// What a live commit produced beyond the transaction's own answer.
#[derive(Debug)]
pub struct LiveCommit {
    /// The transactional commit's result — exactly what
    /// [`commit_setting_change`] returned.
    pub result: Result<(), String>,
    /// The viewport recomputed after a window-mode transition changed the
    /// window's shape. `None` when the change was not a mode change and the
    /// caller's viewport is therefore still current.
    pub viewport: Option<DisplayViewport>,
}

/// [`commit_setting_change`] for a window the caller has **GPU-claimed**.
///
/// A window-mode change goes through [`rts_window::transition_window_mode`],
/// whose contract requires the claim to be released first and retaken after;
/// calling it on a still-claimed window can lose the swapchain, which then
/// surfaces as a `present_error` and exit 1 mid-session. The transition also
/// resizes the window, so the aspect-fit viewport is stale until recomputed
/// — and a stale one silently mismaps every subsequent click.
///
/// Release and reclaim bracket the commit *including its own rollback path*:
/// a refused mode change rolls back inside `transition_window_mode`, which is
/// another mode sequence and needs the claim released just as much. The
/// reclaim therefore runs whatever the commit answered, and only a failed
/// reclaim (or a viewport that cannot be read back) is an `Err` here — that
/// leaves the run with no presentable window and is fatal, not a
/// "SETTINGS NOT SAVED" warning.
pub fn commit_setting_change_live<W: ClaimedWindow>(
    world: &mut RtsWorld,
    window: &mut W,
    store: Option<&SettingsStore>,
    settings: &mut RtsSettings,
    audio: &mut dyn AudioSink,
    change: SettingsChange,
) -> Result<LiveCommit, String> {
    let mode_change = matches!(change, SettingsChange::WindowMode(_));
    if !mode_change {
        return Ok(LiveCommit {
            result: commit_setting_change(world, Some(window), store, settings, audio, change),
            viewport: None,
        });
    }

    window.release_claim();
    let result = commit_setting_change(world, Some(window), store, settings, audio, change);
    window
        .reclaim()
        .map_err(|e| format!("window reclaim after a mode change failed: {e}"))?;
    let viewport = window
        .viewport()
        .map_err(|e| format!("viewport refresh after a mode change failed: {e}"))?;
    Ok(LiveCommit {
        result,
        viewport: Some(viewport),
    })
}

/// Drain one pending change through the memory-only transaction.
fn drain_one_setting_change_memory(
    world: &mut RtsWorld,
    session: &mut RtsSession,
    change: SettingsChange,
) {
    // Window type is never used when `window` is `None`.
    let outcome = commit_setting_change::<crate::rts_window::SdlWindowOps<'_>>(
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

/// Drain `session.pending_setting_change` (+ optional follow-up) through the
/// memory-only transaction (scripted / offscreen).
pub fn drain_pending_setting_change_memory(world: &mut RtsWorld, session: &mut RtsSession) {
    if let Some(change) = session.pending_setting_change.take() {
        drain_one_setting_change_memory(world, session, change);
    }
    if let Some(change) = session.followup_setting_change.take() {
        drain_one_setting_change_memory(world, session, change);
    }
}

/// Drain `session.pending_setting_change` (+ optional follow-up) through the
/// live (claim-aware) transaction. Empty pending is a no-op Ok.
pub fn drain_pending_setting_change_live<W: ClaimedWindow>(
    world: &mut RtsWorld,
    window: &mut W,
    store: Option<&SettingsStore>,
    session: &mut RtsSession,
) -> Result<LiveCommit, String> {
    let Some(change) = session.pending_setting_change.take() else {
        // Follow-up alone is still drained (defensive).
        if let Some(follow) = session.followup_setting_change.take() {
            session.pending_setting_change = Some(follow);
            return drain_pending_setting_change_live(world, window, store, session);
        }
        return Ok(LiveCommit {
            result: Ok(()),
            viewport: None,
        });
    };
    let live = commit_setting_change_live(
        world,
        window,
        store,
        &mut session.settings,
        session.audio.as_mut(),
        change,
    )?;
    match &live.result {
        Ok(()) => session.ui.warning = None,
        Err(reason) => session.ui.warning = Some(format!("SETTINGS NOT SAVED: {reason}")),
    }
    if let Some(follow) = session.followup_setting_change.take() {
        // Chain the same-down slider step after the blur commit.
        let follow_live = commit_setting_change_live(
            world,
            window,
            store,
            &mut session.settings,
            session.audio.as_mut(),
            follow,
        )?;
        match &follow_live.result {
            Ok(()) => {
                if live.result.is_ok() {
                    session.ui.warning = None;
                }
            }
            Err(reason) => session.ui.warning = Some(format!("SETTINGS NOT SAVED: {reason}")),
        }
        let result = match (live.result, follow_live.result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(e), _) | (_, Err(e)) => Err(e),
        };
        return Ok(LiveCommit {
            result,
            viewport: live.viewport.or(follow_live.viewport),
        });
    }
    Ok(live)
}

/// The runtime half of [`commit_setting_change`]'s transaction: window mode
/// / pointer confinement only, applied `from -> to`. `None` for every other
/// [`SettingsChange`] variant (camera speeds and audio have no window-level
/// runtime step).
fn apply_runtime<W: WindowOps>(
    window: Option<&mut W>,
    from: &RtsSettings,
    to: &RtsSettings,
    change: SettingsChange,
) -> Result<(), String> {
    let Some(window) = window else {
        return Ok(());
    };
    match change {
        SettingsChange::WindowMode(_) => {
            match rts_window::transition_window_mode(window, from.display.mode, to.display.mode) {
                Ok(ModeChangeOutcome::Applied) => Ok(()),
                Ok(ModeChangeOutcome::RolledBack(reason)) => Err(reason),
                Err(e) => Err(e.to_string()),
            }
        }
        SettingsChange::Confine(_) => window
            .set_mouse_grab(to.display.confine_pointer)
            .map_err(|e| format!("pointer grab failed: {e}")),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rts_feedback::{FakeAudioSink, UiCue};
    use crate::rts_run::RtsSession;

    #[test]
    fn default_pointer_owner_is_none() {
        assert_eq!(PointerOwner::default(), PointerOwner::None);
    }

    // -- FSM: T13 Test plan table -------------------------------------------

    #[test]
    fn gameplay_escape_opens_paused_menu() {
        let mut ui = RtsUiState::default();
        ui.handle_escape();
        assert_eq!(ui.page, UiPage::PauseMenu);
        assert!(ui.sim_paused());
        assert!(ui.pauses.menu);
    }

    #[test]
    fn escape_backs_out_one_level() {
        let mut ui = RtsUiState::default();
        ui.handle_escape(); // -> PauseMenu
        ui.open_settings(); // -> Settings
        assert_eq!(ui.page, UiPage::Settings);
        ui.handle_escape(); // -> PauseMenu
        assert_eq!(ui.page, UiPage::PauseMenu);
        assert!(ui.sim_paused(), "still paused one level up");
        ui.handle_escape(); // -> Gameplay
        assert_eq!(ui.page, UiPage::Gameplay);
        assert!(!ui.sim_paused());
    }

    #[test]
    fn manual_pause_survives_menu_close() {
        let mut ui = RtsUiState::default();
        ui.toggle_manual_pause();
        ui.handle_escape(); // open menu
        ui.handle_escape(); // close menu
        assert_eq!(ui.page, UiPage::Gameplay);
        assert!(ui.pauses.manual, "manual pause must survive the menu");
        assert!(ui.sim_paused());
    }

    #[test]
    fn space_never_changes_the_page() {
        let mut ui = RtsUiState::default();
        ui.handle_escape();
        ui.open_settings();
        ui.toggle_manual_pause();
        assert_eq!(ui.page, UiPage::Settings, "Space must not skip nesting");
        assert!(ui.pauses.manual);
    }

    #[test]
    fn focus_toggle_controls_pause_reason() {
        let mut ui = RtsUiState::default();
        ui.focus_lost();
        assert_eq!(ui.page, UiPage::PauseMenu);
        assert!(ui.pauses.focus);
        assert!(ui.sim_paused());
        // Closing the menu clears the focus reason.
        ui.handle_escape();
        assert_eq!(ui.page, UiPage::Gameplay);
        assert!(!ui.pauses.focus);
    }

    #[test]
    fn menu_only_opens_from_gameplay() {
        let mut ui = RtsUiState::default();
        ui.open_settings(); // no-op: not in PauseMenu yet
        assert_eq!(ui.page, UiPage::Gameplay);
        ui.open_menu();
        assert_eq!(ui.page, UiPage::PauseMenu);
        ui.open_menu(); // no-op: already open
        assert_eq!(ui.page, UiPage::PauseMenu);
    }

    #[test]
    fn close_menu_preserves_manual_pause() {
        let mut ui = RtsUiState::default();
        ui.toggle_manual_pause();
        ui.open_menu();
        ui.close_menu();
        assert_eq!(ui.page, UiPage::Gameplay);
        assert!(ui.pauses.manual);
        assert!(!ui.pauses.menu);
        assert!(!ui.pauses.focus);
        assert!(ui.sim_paused());
    }

    #[test]
    fn close_menu_clears_menu_and_focus_pause() {
        let mut ui = RtsUiState::default();
        ui.focus_lost();
        assert!(ui.pauses.focus);
        assert!(ui.pauses.menu || ui.page == UiPage::PauseMenu);
        ui.warning = Some("x".into());
        ui.close_menu();
        assert_eq!(ui.page, UiPage::Gameplay);
        assert!(!ui.pauses.menu);
        assert!(!ui.pauses.focus);
        assert!(ui.warning.is_none());
    }

    #[test]
    fn close_menu_activation_emits_menu_cue() {
        let (session, handle) = RtsSession::for_test();
        let mut session = session;
        session.ui.open_menu();
        handle_modal_click(&mut session, ModalHit::CloseMenu);
        assert_eq!(session.ui.page, UiPage::Gameplay);
        assert_eq!(handle.sink().ui_cues(), vec![UiCue::Menu]);
    }

    // -- Modal ownership ------------------------------------------------

    #[test]
    fn modal_owns_every_pointer_point_while_open() {
        let ui_pause = {
            let mut ui = RtsUiState::default();
            ui.handle_escape();
            ui
        };
        // Far outside the pause menu's own rect: still a Modal hit, never
        // Hud/World.
        let owner = match ui_pause.page {
            UiPage::PauseMenu => modal_hit_test(ModalPage::PauseMenu, [10.0, 10.0], 0.0),
            _ => unreachable!(),
        };
        assert_eq!(owner, ModalHit::Consumed);

        let mut ui_settings = ui_pause;
        ui_settings.open_settings();
        let hit = modal_hit_test(ModalPage::Settings, [10.0, 10.0], 0.0);
        assert_eq!(
            hit,
            ModalHit::Consumed,
            "outside every control, still consumed"
        );
    }

    #[test]
    fn settings_button_and_back_button_hit() {
        let btn = HudLayout::PAUSE_MENU_SETTINGS_BTN;
        let hit = modal_hit_test(ModalPage::PauseMenu, [btn[0] + 1.0, btn[1] + 1.0], 0.0);
        assert_eq!(hit, ModalHit::OpenSettings);

        let back = HudLayout::SETTINGS_BACK_BTN;
        let hit = modal_hit_test(ModalPage::Settings, [back[0] + 1.0, back[1] + 1.0], 0.0);
        assert_eq!(hit, ModalHit::Back);
    }

    #[test]
    fn sliders_snap_to_legal_steps() {
        use mmd_engine::rts::KEYBOARD_PAN_TRACK as TRACK;
        // A click roughly a third of the way along a 6..96 step-6 track
        // must land on a multiple of 6, not the raw fractional value.
        let x = TRACK[0] + TRACK[2] * 0.33;
        let hit = modal_hit_test(ModalPage::Settings, [x, TRACK[1] + 1.0], 0.0);
        let ModalHit::KeyboardPan(v) = hit else {
            panic!("expected a KeyboardPan hit: {hit:?}")
        };
        assert!((6..=96).contains(&v) && v.is_multiple_of(6), "{v}");

        use mmd_engine::rts::MASTER_TRACK;
        let x = MASTER_TRACK[0] + MASTER_TRACK[2] * 0.5;
        let hit = modal_hit_test(ModalPage::Settings, [x, MASTER_TRACK[1] + 1.0], 0.0);
        let ModalHit::Master(v) = hit else {
            panic!("expected a Master hit: {hit:?}")
        };
        assert!(v <= 100 && v.is_multiple_of(5), "{v}");
    }

    // -- Transactional settings controller -------------------------------

    #[derive(Default)]
    struct FakeWindow {
        log: Vec<String>,
        grabbed: bool,
        fail: Option<&'static str>,
        /// Logical size the fake reports back after a mode change, so a test
        /// can tell a refreshed viewport from the one the caller already had.
        size: Option<[u32; 2]>,
    }

    impl ClaimedWindow for FakeWindow {
        fn release_claim(&mut self) {
            self.log.push("release_claim".to_string());
        }
        fn reclaim(&mut self) -> Result<(), String> {
            self.record("reclaim")
        }
        fn viewport(&self) -> Result<DisplayViewport, String> {
            if self.fail == Some("viewport") {
                return Err("viewport failed (injected)".to_string());
            }
            let size = self.size.unwrap_or([1920, 1080]);
            DisplayViewport::new(size, size).ok_or_else(|| "degenerate size".to_string())
        }
    }

    impl WindowOps for FakeWindow {
        fn leave_fullscreen(&mut self) -> Result<(), String> {
            self.record("leave_fullscreen")
        }
        fn enter_fullscreen(&mut self) -> Result<(), String> {
            self.record("enter_fullscreen")
        }
        fn clear_exclusive_mode(&mut self) -> Result<(), String> {
            self.record("clear_exclusive_mode")
        }
        fn available_modes(&mut self) -> Result<Vec<crate::rts_window::ModeCandidate>, String> {
            self.record("available_modes")?;
            Ok(vec![crate::rts_window::ModeCandidate {
                w: 1920,
                h: 1080,
                refresh_rate: 60.0,
            }])
        }
        fn set_exclusive_mode(
            &mut self,
            _mode: crate::rts_window::ModeCandidate,
        ) -> Result<(), String> {
            self.record("set_exclusive_mode")
        }
        fn set_bordered(&mut self, bordered: bool) -> Result<(), String> {
            self.record(if bordered {
                "set_bordered(true)"
            } else {
                "set_bordered(false)"
            })
        }
        fn set_size(&mut self, _w: u32, _h: u32) -> Result<(), String> {
            self.record("set_size")
        }
        fn center(&mut self) -> Result<(), String> {
            self.record("center")
        }
        fn sync(&mut self) -> Result<(), String> {
            self.record("sync")
        }
        fn set_mouse_grab(&mut self, grabbed: bool) -> Result<(), String> {
            self.record("set_mouse_grab")?;
            self.grabbed = grabbed;
            Ok(())
        }
    }

    /// A live mode change must bracket the whole transition in
    /// release/reclaim, and hand back a viewport recomputed *after* it.
    ///
    /// Before this, `commit_setting_change` ran `transition_window_mode` on a
    /// still-GPU-claimed window — against that function's own documented
    /// contract — so a settings-menu mode switch could lose the swapchain and
    /// take the session down with a `present_error` and exit 1. The viewport
    /// was not refreshed either, so every later click mapped through the old
    /// window shape.
    #[test]
    fn a_live_mode_change_brackets_the_transition_in_release_and_reclaim() {
        let mut world = test_world();
        let mut settings = RtsSettings::default();
        let mut audio = FakeAudioSink::new();
        let mut window = FakeWindow {
            size: Some([1280, 720]),
            ..Default::default()
        };

        let live = commit_setting_change_live(
            &mut world,
            &mut window,
            None,
            &mut settings,
            &mut audio,
            SettingsChange::WindowMode(WindowMode::Windowed1280x720),
        )
        .expect("the reclaim succeeded");

        assert!(live.result.is_ok(), "{:?}", live.result);
        assert_eq!(
            window.log.first().map(String::as_str),
            Some("release_claim"),
            "the claim must be dropped before the first mode call: {:?}",
            window.log
        );
        assert_eq!(
            window.log.last().map(String::as_str),
            Some("reclaim"),
            "the claim must be retaken after the last mode call: {:?}",
            window.log
        );
        assert!(
            window.log.len() > 2,
            "the mode sequence itself must run between them: {:?}",
            window.log
        );
        let vp = live.viewport.expect("a mode change refreshes the viewport");
        assert_eq!(
            vp,
            DisplayViewport::new([1280, 720], [1280, 720]).unwrap(),
            "the viewport must be recomputed from the window's new shape"
        );
    }

    /// Only a mode change needs the claim dance. A pointer-confinement or
    /// volume edit must not tear down the swapchain for nothing, and leaves
    /// the caller's viewport alone.
    #[test]
    fn a_non_mode_change_never_touches_the_gpu_claim() {
        let mut world = test_world();
        let mut settings = RtsSettings::default();
        let mut audio = FakeAudioSink::new();
        let mut window = FakeWindow::default();

        let live = commit_setting_change_live(
            &mut world,
            &mut window,
            None,
            &mut settings,
            &mut audio,
            SettingsChange::Confine(false),
        )
        .expect("no claim work to fail");

        assert!(live.result.is_ok(), "{:?}", live.result);
        assert!(live.viewport.is_none(), "no mode change, no new viewport");
        assert!(
            !window
                .log
                .iter()
                .any(|s| s == "release_claim" || s == "reclaim"),
            "a non-mode change must not release the claim: {:?}",
            window.log
        );
    }

    /// A refused mode change rolls back *inside* `transition_window_mode` —
    /// another mode sequence, on the same released claim. The reclaim has to
    /// happen whatever the transaction answered, or a rejected settings edit
    /// leaves the run with no swapchain at all.
    #[test]
    fn a_refused_mode_change_still_reclaims() {
        let mut world = test_world();
        let mut settings = RtsSettings::default();
        let mut audio = FakeAudioSink::new();
        let mut window = FakeWindow {
            fail: Some("set_size"),
            ..Default::default()
        };

        let live = commit_setting_change_live(
            &mut world,
            &mut window,
            None,
            &mut settings,
            &mut audio,
            SettingsChange::WindowMode(WindowMode::Windowed1280x720),
        )
        .expect("the reclaim itself succeeded");

        assert!(
            live.result.is_err(),
            "the injected failure must refuse the commit"
        );
        assert_eq!(
            settings.display.mode,
            RtsSettings::default().display.mode,
            "a refused mode change must not publish"
        );
        assert_eq!(
            window.log.last().map(String::as_str),
            Some("reclaim"),
            "the claim must be retaken even on a refused change: {:?}",
            window.log
        );
    }

    /// A reclaim that fails leaves the run with no presentable window. That is
    /// fatal for the session, not a "SETTINGS NOT SAVED" warning, so it comes
    /// back as the outer `Err`.
    #[test]
    fn a_failed_reclaim_is_fatal_not_a_warning() {
        let mut world = test_world();
        let mut settings = RtsSettings::default();
        let mut audio = FakeAudioSink::new();
        let mut window = FakeWindow {
            fail: Some("reclaim"),
            ..Default::default()
        };

        let err = commit_setting_change_live(
            &mut world,
            &mut window,
            None,
            &mut settings,
            &mut audio,
            SettingsChange::WindowMode(WindowMode::Windowed1280x720),
        )
        .expect_err("a failed reclaim must not come back as a soft warning");
        assert!(err.contains("reclaim"), "{err}");
    }

    impl FakeWindow {
        fn record(&mut self, step: &str) -> Result<(), String> {
            self.log.push(step.to_string());
            if self.fail == Some(step) {
                Err(format!("{step} failed (injected)"))
            } else {
                Ok(())
            }
        }
    }

    // --- execute_slot tests -----------------------------------------------

    fn find_building(
        world: &RtsWorld,
        kind: mmd_engine::rts::BuildingKind,
    ) -> Option<mmd_engine::rts::EntityId> {
        let store = world.entities();
        for slot in 0..store.slot_count() {
            if let Some(id) = store.id_at(slot) {
                if store.kind(slot) == mmd_engine::rts::EntityKind::Building(kind) {
                    return Some(id);
                }
            }
        }
        None
    }

    fn select_hq(world: &mut RtsWorld) {
        let id =
            find_building(world, mmd_engine::rts::BuildingKind::Hq).expect("no HQ in test world");
        world.select_only(id);
    }

    fn select_barracks(world: &mut RtsWorld) {
        let id = find_building(world, mmd_engine::rts::BuildingKind::Barracks)
            .expect("no Barracks in test world");
        world.select_only(id);
    }

    #[test]
    fn out_of_range_slot_is_false_not_panic() {
        let mut world = test_world();
        let (mut session, _handle) = RtsSession::for_test();
        assert!(!execute_slot(&mut world, &mut session, 9));
        assert!(!execute_slot(&mut world, &mut session, 255));
    }

    #[test]
    fn hq_q_queues_worker() {
        let mut world = test_world();
        let (mut session, _handle) = RtsSession::for_test();
        select_hq(&mut world);
        let crystal_before = world.resources().crystal;
        let dispatched = execute_slot(&mut world, &mut session, 0); // Q → slot 0
        assert!(dispatched, "HQ slot 0 must dispatch");
        assert!(
            world.resources().crystal < crystal_before,
            "training worker must cost crystal"
        );
    }

    #[test]
    fn barracks_q_queues_soldier() {
        let mut world = test_world();
        let (mut session, _handle) = RtsSession::for_test();
        // Barracks may not exist in the default scenario; skip if absent.
        let Some(id) = find_building(&world, mmd_engine::rts::BuildingKind::Barracks) else {
            return;
        };
        world.select_only(id);
        let crystal_before = world.resources().crystal;
        let dispatched = execute_slot(&mut world, &mut session, 0); // Q → slot 0
        assert!(dispatched, "Barracks slot 0 must dispatch");
        assert!(
            world.resources().crystal < crystal_before,
            "training soldier must cost crystal"
        );
    }

    #[test]
    fn c_arms_rally() {
        let mut world = test_world();
        let (mut session, _handle) = RtsSession::for_test();
        select_hq(&mut world);
        assert!(session.pending_rally.is_none(), "rally not armed before");
        let dispatched = execute_slot(&mut world, &mut session, 8); // C → slot 8
        assert!(dispatched, "HQ slot 8 (rally) must dispatch");
        assert!(
            session.pending_rally.is_some(),
            "rally must be armed after C"
        );
    }

    #[test]
    fn disabled_slot_key_is_noop_without_sfx() {
        let mut world = test_world();
        let (mut session, handle) = RtsSession::for_test();
        // No selection → all slots disabled/empty.
        let dispatched = execute_slot(&mut world, &mut session, 0);
        assert!(!dispatched);
        assert!(
            handle.sink().ui_cues().is_empty(),
            "disabled slot must emit no cue"
        );
    }

    #[test]
    fn keyboard_and_pointer_share_execute_slot() {
        // Keyboard path calls execute_slot; pointer path (handle_hud_click) calls
        // execute_slot too. Verify they produce the same state.
        let mut world_key = test_world();
        let (mut session_key, _) = RtsSession::for_test();
        select_hq(&mut world_key);
        let crystal_key_before = world_key.resources().crystal;
        execute_slot(&mut world_key, &mut session_key, 0);
        let crystal_after_key = world_key.resources().crystal;

        let mut world_ptr = test_world();
        let (mut session_ptr, _) = RtsSession::for_test();
        select_hq(&mut world_ptr);
        handle_hud_click(
            &mut world_ptr,
            &mut session_ptr,
            HudHit::CommandSlot(0),
            false,
        );
        let crystal_after_ptr = world_ptr.resources().crystal;

        assert_eq!(
            crystal_key_before - crystal_after_key,
            crystal_key_before - crystal_after_ptr,
            "keyboard and pointer execute_slot must cost the same"
        );
    }

    /// Not `testkit::RtsHarness` (feature-gated out of this binary crate,
    /// per the shipping-build contract) — the tracked scenario, loaded
    /// exactly as `rts_run::run` loads it.
    fn test_world() -> RtsWorld {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/scenarios/rts_prototype_v1.ron");
        RtsWorld::load(&path).expect("tracked scenario loads")
    }

    #[test]
    fn each_accepted_change_saves_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SettingsStore::at(dir.path().join("settings-v1.json"));
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();

        let result = commit_setting_change::<FakeWindow>(
            &mut world,
            None,
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::Master(55),
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(settings.audio.master, 55);

        let bytes1 = std::fs::read(dir.path().join("settings-v1.json")).expect("one write");
        let loaded = store.load();
        assert_eq!(loaded.value.audio.master, 55);
        // A second commit of an unrelated field must not disturb the
        // already-saved value, and the file itself is written exactly once
        // per commit (T7's own canonical-bytes contract).
        commit_setting_change::<FakeWindow>(
            &mut world,
            None,
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::Sfx(45),
        )
        .expect("second legal change commits");
        let bytes2 = std::fs::read(dir.path().join("settings-v1.json")).expect("second write");
        assert_ne!(
            bytes1, bytes2,
            "the second commit changed the on-disk value"
        );
        assert_eq!(
            store.load().value.audio.master,
            55,
            "first commit's value survives"
        );
    }

    #[test]
    fn failed_save_rolls_back_runtime_and_cfg() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings-v1.json");
        // A directory in place of the settings file makes every save fail —
        // the store's own `File::create` on the `.tmp` sibling still
        // succeeds, but the final rename onto `path` cannot replace a
        // non-empty directory.
        std::fs::create_dir(&path).expect("seed a directory in place of the file");
        let store = SettingsStore::at(path);
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();
        let mut window = FakeWindow::default();

        let result = commit_setting_change(
            &mut world,
            Some(&mut window),
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::Confine(false),
        );

        assert!(
            result.is_err(),
            "a save into a directory must fail: {result:?}"
        );
        assert_eq!(
            settings.display.confine_pointer,
            RtsSettings::default().display.confine_pointer,
            "cfg must be untouched on a failed save"
        );
        // Runtime went to `false` then rolled back to the old (default,
        // `true`) value.
        assert!(window.grabbed, "runtime must roll back to the old value");
    }

    #[test]
    fn window_mode_change_uses_safe_transition() {
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SettingsStore::at(dir.path().join("settings-v1.json"));
        let mut window = FakeWindow::default();

        let result = commit_setting_change(
            &mut world,
            Some(&mut window),
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::WindowMode(WindowMode::Windowed1280x720),
        );

        assert!(result.is_ok(), "{result:?}");
        assert_eq!(settings.display.mode, WindowMode::Windowed1280x720);
        assert_eq!(
            window.log,
            vec![
                "leave_fullscreen",
                "clear_exclusive_mode",
                "set_bordered(true)",
                "set_size",
                "center",
                "sync",
            ],
            "must go through apply_window_mode's exact windowed sequence"
        );
    }

    #[test]
    fn a_rejected_change_never_touches_runtime_or_disk() {
        // `commit_setting_change` itself always applies a legally-snapped
        // value (the modal hit test guarantees that), so this proves the
        // validation gate directly: an out-of-range candidate is rejected
        // before `apply_runtime`/`save` run at all.
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SettingsStore::at(dir.path().join("settings-v1.json"));
        let mut window = FakeWindow::default();

        let result = commit_setting_change(
            &mut world,
            Some(&mut window),
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::KeyboardPan(97), // > PAN_MAX, not a legal step
        );

        assert!(result.is_err());
        assert!(window.log.is_empty(), "runtime must never be touched");
        assert_eq!(settings, RtsSettings::default());
        assert!(
            !dir.path().join("settings-v1.json").exists(),
            "nothing saved"
        );
    }

    #[test]
    fn no_window_skips_runtime_but_still_saves() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SettingsStore::at(dir.path().join("settings-v1.json"));
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();

        let result = commit_setting_change::<FakeWindow>(
            &mut world,
            None,
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::WindowMode(WindowMode::Windowed1280x720),
        );

        assert!(result.is_ok(), "{result:?}");
        assert_eq!(settings.display.mode, WindowMode::Windowed1280x720);
        assert_eq!(
            store.load().value.display.mode,
            WindowMode::Windowed1280x720
        );
    }

    #[test]
    fn keyboard_and_edge_pan_apply_to_the_world() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SettingsStore::at(dir.path().join("settings-v1.json"));
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();

        commit_setting_change::<FakeWindow>(
            &mut world,
            None,
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::KeyboardPan(24),
        )
        .expect("legal change commits");

        assert_eq!(settings.camera.keyboard_pan, 24);
    }

    // -- Audio gain hook (T15) -------------------------------------------

    #[test]
    fn a_committed_volume_edit_pushes_new_gains() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SettingsStore::at(dir.path().join("settings-v1.json"));
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();

        commit_setting_change::<FakeWindow>(
            &mut world,
            None,
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::Master(50),
        )
        .expect("a legal volume edit commits");

        assert_eq!(audio.gain_calls(), 1, "one gain push per commit");
        assert_eq!(
            audio.gains(),
            effective_gains(&settings.audio),
            "the sink must hear the committed value, not the old one"
        );
        assert_eq!(audio.gains().music_basis_points, 50 * 35);
    }

    #[test]
    fn a_failed_gain_push_rolls_back_like_a_failed_save() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings-v1.json");
        let store = SettingsStore::at(path.clone());
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut window = FakeWindow::default();
        let mut audio = FakeAudioSink::new();
        audio.set_fail_set_gains(true);

        let result = commit_setting_change(
            &mut world,
            Some(&mut window),
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::Confine(false),
        );

        let reason = result.expect_err("a failing gain push must fail the commit");
        assert!(reason.contains("audio gain failed"), "{reason}");
        assert_eq!(settings, RtsSettings::default(), "cfg must be untouched");
        assert!(window.grabbed, "runtime must roll back to the old value");
        assert!(!path.exists(), "a failed gain push must never reach disk");
    }

    #[test]
    fn a_failed_save_restores_the_old_gains() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings-v1.json");
        // Same trick as `failed_save_rolls_back_runtime_and_cfg`: a
        // directory in place of the target makes the final rename fail.
        std::fs::create_dir(&path).expect("seed a directory in place of the file");
        let store = SettingsStore::at(path);
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();
        let old_gains = effective_gains(&settings.audio);

        let result = commit_setting_change::<FakeWindow>(
            &mut world,
            None,
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::Music(5),
        );

        assert!(result.is_err(), "{result:?}");
        assert_eq!(settings, RtsSettings::default(), "cfg must be untouched");
        assert_eq!(
            audio.gains(),
            old_gains,
            "a failed save must put the old gains back"
        );
    }

    // Keeps mmd-engine's duplicated pan/volume bounds honest against T7's
    // schema-1 contract (`rts_settings.rs`'s private constants), without
    // making the engine crate depend on the app crate's settings type.
    #[test]
    fn settings_pan_and_volume_bounds_match_app_contract() {
        use mmd_engine::rts::{PAN_MAX, PAN_MIN, PAN_STEP, VOLUME_MAX, VOLUME_MIN, VOLUME_STEP};
        let mut s = RtsSettings::default();
        s.camera.keyboard_pan = PAN_MIN;
        s.camera.edge_pan = PAN_MIN;
        assert!(s.validate().is_ok(), "engine PAN_MIN must be app-legal");
        s.camera.keyboard_pan = PAN_MAX;
        s.camera.edge_pan = PAN_MAX;
        assert!(s.validate().is_ok(), "engine PAN_MAX must be app-legal");
        s.camera.keyboard_pan = PAN_MIN + PAN_STEP;
        assert!(s.validate().is_ok(), "engine PAN_STEP must be app-legal");

        let mut s = RtsSettings::default();
        s.audio.master = VOLUME_MIN;
        assert!(s.validate().is_ok());
        s.audio.master = VOLUME_MAX;
        assert!(s.validate().is_ok());
        s.audio.master = VOLUME_MIN + VOLUME_STEP;
        assert!(s.validate().is_ok());
    }

    // -- T3 live sliders -------------------------------------------------

    use crate::rts_feedback::FakeSinkHandle;
    use crate::rts_input::RtsCommand;
    use crate::rts_run::{apply, pointer_down, pointer_up};
    use mmd_engine::rts::{
        KEYBOARD_PAN_TRACK, MASTER_TRACK, NUMERIC_SETTING_SPECS, PAN_MAX, PAN_MIN,
        snap_numeric_at_x,
    };

    fn session_open_settings() -> (RtsWorld, RtsSession, FakeSinkHandle) {
        let (mut session, handle) = RtsSession::for_test();
        session.ui.open_menu();
        session.ui.open_settings();
        // Mirror startup: world camera speeds match default settings.
        let mut world = test_world();
        world.set_camera_speeds(
            session.settings.camera.keyboard_pan as f32,
            session.settings.camera.edge_pan as f32,
        );
        (world, session, handle)
    }

    fn track_x_for(track: [f32; 4], value: u32, min: u32, max: u32) -> f32 {
        let frac = (value - min) as f32 / (max - min) as f32;
        track[0] + frac * track[2]
    }

    fn track_point(track: [f32; 4], value: u32, min: u32, max: u32) -> [f32; 2] {
        [track_x_for(track, value, min, max), track[1] + 1.0]
    }

    #[test]
    fn slider_drag_retains_stable_control() {
        let (mut world, mut session, _) = session_open_settings();
        let start = track_point(KEYBOARD_PAN_TRACK, 48, PAN_MIN, PAN_MAX);
        pointer_down(&mut world, &mut session, start);
        assert_eq!(
            session.pressed_control(),
            Some(ControlId::KeyboardPanSlider)
        );
        // Move far outside the track — ownership stays on the slider.
        apply(&mut world, &mut session, RtsCommand::Move([10.0, 10.0]));
        assert_eq!(
            session.pressed_control(),
            Some(ControlId::KeyboardPanSlider),
            "motion outside track must keep the slider owner"
        );
        // x clamps to endpoints for the staged value (left end → min).
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.camera.keyboard_pan, PAN_MIN);
    }

    #[test]
    fn slider_drag_commits_only_distinct_steps() {
        let (mut world, mut session, audio) = session_open_settings();
        let start = track_point(KEYBOARD_PAN_TRACK, 48, PAN_MIN, PAN_MAX);
        // Default keyboard_pan is 48 — down at 48 stages nothing.
        pointer_down(&mut world, &mut session, start);
        drain_pending_setting_change_memory(&mut world, &mut session);
        let gains_before = audio.sink().gain_calls();

        // Ten motions inside the same step band around 60.
        let x60 = track_x_for(KEYBOARD_PAN_TRACK, 60, PAN_MIN, PAN_MAX);
        for dx in 0..10 {
            let x = x60 + (dx as f32) * 0.3; // stay inside the 60 step
            apply(
                &mut world,
                &mut session,
                RtsCommand::Move([x, KEYBOARD_PAN_TRACK[1] + 1.0]),
            );
            drain_pending_setting_change_memory(&mut world, &mut session);
        }
        assert_eq!(session.settings.camera.keyboard_pan, 60);
        // One save/runtime change → one gain push (even camera still pushes gains).
        assert_eq!(
            audio.sink().gain_calls() - gains_before,
            1,
            "duplicate motions in one step must not re-commit"
        );
        // One Settings cue for the whole drag.
        assert_eq!(audio.sink().ui_cues(), vec![UiCue::Settings]);
    }

    #[test]
    fn slider_drag_updates_camera_and_audio_before_next_frame() {
        let (mut world, mut session, audio) = session_open_settings();
        // Camera: 48 → 60.
        let p60 = track_point(KEYBOARD_PAN_TRACK, 60, PAN_MIN, PAN_MAX);
        pointer_down(&mut world, &mut session, p60);
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.camera.keyboard_pan, 60);
        assert_eq!(world.camera_speeds().0, 60.0);
        pointer_up(&mut world, &mut session, p60, false);

        // Audio: master default 80 → 50 via a fresh drag.
        let p50 = track_point(MASTER_TRACK, 50, 0, 100);
        pointer_down(&mut world, &mut session, p50);
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.audio.master, 50);
        assert_eq!(
            audio.sink().gains(),
            effective_gains(&session.settings.audio)
        );
        pointer_up(&mut world, &mut session, p50, false);
    }

    #[test]
    fn failed_slider_commit_rolls_back_runtime_and_value() {
        let (mut world, mut session, handle) = session_open_settings();
        handle.set_fail_set_gains(true);
        let p60 = track_point(KEYBOARD_PAN_TRACK, 60, PAN_MIN, PAN_MAX);
        let old = session.settings.camera.keyboard_pan;
        let old_speed = world.camera_speeds();
        pointer_down(&mut world, &mut session, p60);
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.camera.keyboard_pan, old);
        assert_eq!(world.camera_speeds(), old_speed);
        assert!(session.ui.warning.is_some());
        // Drag stays active after a failed step.
        assert_eq!(
            session.pressed_control(),
            Some(ControlId::KeyboardPanSlider)
        );
        // Later legal motion after the fault clears still commits.
        handle.set_fail_set_gains(false);
        session.ui.warning = None;
        let p72 = track_point(KEYBOARD_PAN_TRACK, 72, PAN_MIN, PAN_MAX);
        apply(&mut world, &mut session, RtsCommand::Move(p72));
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.camera.keyboard_pan, 72);
        assert_eq!(world.camera_speeds().0, 72.0);
        assert!(session.ui.warning.is_none());
    }

    #[test]
    fn slider_drag_never_selects_world() {
        let (mut world, mut session, _) = session_open_settings();
        let before = world.selection().ids().to_vec();
        let start = track_point(KEYBOARD_PAN_TRACK, 48, PAN_MIN, PAN_MAX);
        pointer_down(&mut world, &mut session, start);
        // Drag out into the world and release.
        apply(&mut world, &mut session, RtsCommand::Move([960.0, 400.0]));
        pointer_up(&mut world, &mut session, [960.0, 400.0], false);
        assert_eq!(world.selection().ids(), before.as_slice());
        assert_eq!(session.ui.page, UiPage::Settings);
        assert!(session.pressed_control().is_none());
    }

    #[test]
    fn scripted_slider_drag_uses_same_controller() {
        let (mut world, mut session, audio) = session_open_settings();
        let a = track_point(KEYBOARD_PAN_TRACK, 48, PAN_MIN, PAN_MAX);
        let b = track_point(KEYBOARD_PAN_TRACK, 72, PAN_MIN, PAN_MAX);
        apply(&mut world, &mut session, RtsCommand::Drag(a, b));
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.camera.keyboard_pan, 72);
        assert_eq!(world.camera_speeds().0, 72.0);
        assert_eq!(audio.sink().ui_cues(), vec![UiCue::Settings]);
        assert!(session.pressed_control().is_none());
        assert!(session.ui.warning.is_none());
    }

    #[test]
    fn numeric_id_mapping_is_exhaustive() {
        for spec in &NUMERIC_SETTING_SPECS {
            let change = settings_change_for_numeric(spec.id, spec.min);
            let mut s = RtsSettings::default();
            change.apply_to(&mut s);
            assert_eq!(current_numeric_value(&s, spec.id), spec.min);
            assert_eq!(
                snap_numeric_at_x(spec.id, spec.track[0]),
                spec.min,
                "{:?} left end",
                spec.id
            );
            assert_eq!(
                snap_numeric_at_x(spec.id, spec.track[0] + spec.track[2]),
                spec.max,
                "{:?} right end",
                spec.id
            );
        }
    }

    // -- T4 typed numeric fields ---------------------------------------------

    fn field_point(id: NumericSettingId) -> [f32; 2] {
        let f = id.spec().value_field;
        [f[0] + 1.0, f[1] + 1.0]
    }

    fn click_field(world: &mut RtsWorld, session: &mut RtsSession, id: NumericSettingId) {
        let p = field_point(id);
        apply(world, session, RtsCommand::LeftClick(p));
    }

    #[test]
    fn numeric_edit_accepts_three_ascii_digits() {
        let mut edit = NumericEdit::begin(NumericSettingId::KeyboardPan, 48);
        // Clear seed then type 999.
        edit.backspace();
        edit.backspace();
        assert!(edit.push_ascii_digit(b'9'));
        assert!(edit.push_ascii_digit(b'9'));
        assert!(edit.push_ascii_digit(b'9'));
        assert!(!edit.push_ascii_digit(b'9'), "4th digit must be ignored");
        assert_eq!(edit.display(), b"999");
        assert_eq!(edit.parsed(), Some(999));
    }

    #[test]
    fn unsupported_text_is_ignored() {
        let mut edit = NumericEdit::begin(NumericSettingId::Master, 80);
        edit.backspace();
        edit.backspace();
        assert!(!edit.push_ascii_digit(b'a'));
        assert!(!edit.push_ascii_digit(b'-'));
        assert!(!edit.push_ascii_digit(b'.'));
        assert!(!edit.push_ascii_digit(b' '));
        assert_eq!(edit.len_for_test(), 0);
        assert!(edit.push_ascii_digit(b'5'));
        assert_eq!(edit.display(), b"5");
    }

    #[test]
    fn backspace_edits_fixed_buffer() {
        let mut edit = NumericEdit::begin(NumericSettingId::EdgePan, 48);
        assert_eq!(edit.display(), b"48");
        edit.backspace();
        assert_eq!(edit.display(), b"4");
        edit.backspace();
        assert_eq!(edit.display(), b"");
        edit.backspace(); // empty stays empty
        assert_eq!(edit.parsed(), None);
        assert!(edit.push_ascii_digit(b'1'));
        assert!(edit.push_ascii_digit(b'2'));
        assert_eq!(edit.display(), b"12");
    }

    #[test]
    fn enter_clamps_snaps_and_commits() {
        let (mut world, mut session, audio) = session_open_settings();
        click_field(&mut world, &mut session, NumericSettingId::KeyboardPan);
        assert!(session.numeric_edit.is_some());
        // Replace seed with 999 → clamp_snap → 96.
        if let Some(edit) = session.numeric_edit.as_mut() {
            edit.backspace();
            edit.backspace();
            let _ = edit.push_ascii_digit(b'9');
            let _ = edit.push_ascii_digit(b'9');
            let _ = edit.push_ascii_digit(b'9');
        }
        finish_numeric_edit(&mut session, NumericEditEnd::Commit);
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.camera.keyboard_pan, PAN_MAX);
        assert_eq!(world.camera_speeds().0, PAN_MAX as f32);
        assert!(session.numeric_edit.is_none());
        assert_eq!(audio.sink().ui_cues(), vec![UiCue::Settings]);

        // Volume half-step 53 → 55.
        click_field(&mut world, &mut session, NumericSettingId::Master);
        if let Some(edit) = session.numeric_edit.as_mut() {
            edit.backspace();
            edit.backspace();
            let _ = edit.push_ascii_digit(b'5');
            let _ = edit.push_ascii_digit(b'3');
        }
        finish_numeric_edit(&mut session, NumericEditEnd::Commit);
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.audio.master, 55);
    }

    #[test]
    fn empty_enter_restores_without_save() {
        let (mut world, mut session, audio) = session_open_settings();
        let old = session.settings.camera.keyboard_pan;
        let gains_before = audio.sink().gain_calls();
        click_field(&mut world, &mut session, NumericSettingId::KeyboardPan);
        if let Some(edit) = session.numeric_edit.as_mut() {
            while edit.parsed().is_some() {
                edit.backspace();
            }
        }
        finish_numeric_edit(&mut session, NumericEditEnd::Commit);
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.camera.keyboard_pan, old);
        assert_eq!(audio.sink().gain_calls(), gains_before);
        assert!(session.ui.warning.is_none());
        assert!(session.numeric_edit.is_none());
    }

    #[test]
    fn pointer_blur_commits_before_activation() {
        let (mut world, mut session, _) = session_open_settings();
        click_field(&mut world, &mut session, NumericSettingId::KeyboardPan);
        if let Some(edit) = session.numeric_edit.as_mut() {
            edit.backspace();
            edit.backspace();
            let _ = edit.push_ascii_digit(b'6');
            let _ = edit.push_ascii_digit(b'0');
        }
        // Click the focus-loss checkbox — blur commits 60, then toggles checkbox.
        let focus = mmd_engine::rts::FOCUS_CONTROL_RECT;
        let p = [focus[0] + 1.0, focus[1] + 1.0];
        apply(&mut world, &mut session, RtsCommand::LeftClick(p));
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.camera.keyboard_pan, 60);
        assert!(session.settings.gameplay.pause_on_focus_loss);
        assert!(session.numeric_edit.is_none());
    }

    #[test]
    fn escape_restores_and_consumes_navigation() {
        let (mut world, mut session, audio) = session_open_settings();
        let old = session.settings.camera.keyboard_pan;
        let gains_before = audio.sink().gain_calls();
        click_field(&mut world, &mut session, NumericSettingId::KeyboardPan);
        if let Some(edit) = session.numeric_edit.as_mut() {
            edit.backspace();
            edit.backspace();
            let _ = edit.push_ascii_digit(b'9');
            let _ = edit.push_ascii_digit(b'9');
            let _ = edit.push_ascii_digit(b'9');
        }
        apply(&mut world, &mut session, RtsCommand::Escape);
        drain_pending_setting_change_memory(&mut world, &mut session);
        assert_eq!(session.settings.camera.keyboard_pan, old);
        assert_eq!(audio.sink().gain_calls(), gains_before);
        assert_eq!(
            session.ui.page,
            UiPage::Settings,
            "Escape must not leave Settings"
        );
        assert!(session.numeric_edit.is_none());
    }

    #[test]
    fn os_focus_loss_finalizes_before_pause_and_clear() {
        let (mut world, mut session, _) = session_open_settings();
        click_field(&mut world, &mut session, NumericSettingId::Master);
        if let Some(edit) = session.numeric_edit.as_mut() {
            edit.backspace();
            edit.backspace();
            let _ = edit.push_ascii_digit(b'5');
            let _ = edit.push_ascii_digit(b'3');
        }
        // Simulate a held key + press before focus loss.
        session.keyboard_held = [1.0, 0.0];
        pointer_down(
            &mut world,
            &mut session,
            field_point(NumericSettingId::Master),
        );
        finalize_numeric_edit_on_focus_loss(&mut session);
        drain_pending_setting_change_memory(&mut world, &mut session);
        // Live path only calls focus_lost when pause-on-focus-loss is on.
        session.ui.focus_lost();
        assert_eq!(session.settings.audio.master, 55);
        assert!(session.numeric_edit.is_none());
        assert!(session.pressed_control().is_none());
        assert_eq!(session.keyboard_held, [0.0, 0.0]);
        assert!(session.ui.pauses.focus);
        assert_eq!(session.ui.page, UiPage::PauseMenu);
    }

    #[test]
    fn offscreen_never_starts_text_input() {
        // Pure apply/pointer path focuses a field + accepts digits with no SDL.
        let (mut world, mut session, _) = session_open_settings();
        // SFX field is at content_y=968, outside viewport at offset=0; scroll to max.
        use mmd_engine::rts::settings_max_scroll;
        let offset = settings_max_scroll();
        session.ui.settings_scroll_px = offset;
        let sf = NumericSettingId::Sfx.spec().value_field;
        let p = [sf[0] + 1.0, sf[1] + 1.0 - offset];
        apply(&mut world, &mut session, RtsCommand::LeftClick(p));
        assert!(session.numeric_edit.is_some());
        if let Some(edit) = session.numeric_edit.as_mut() {
            while edit.parsed().is_some() {
                edit.backspace();
            }
        }
        numeric_edit_text_input(&mut session, "abc40x5");
        assert_eq!(session.numeric_edit.as_ref().unwrap().display(), b"405");
        // Offscreen/scripted path never touches VideoSubsystem::text_input —
        // only the interactive window loop does. This test exercises the pure
        // FSM exclusively.
        finish_numeric_edit(&mut session, NumericEditEnd::Cancel);
        assert!(session.numeric_edit.is_none());
    }

    // -- T5: mute-label controls -----------------------------------------

    fn mute_label_point(ch: usize) -> [f32; 2] {
        let rect = mmd_engine::rts::MUTE_LABEL_RECTS[ch];
        // Use right edge to avoid the Back button (x 472..632) for SFX (ch=3, y 928..960).
        [rect[0] + rect[2] - 1.0, rect[1] + 1.0]
    }

    #[test]
    fn muting_master_keeps_stored_levels() {
        let (mut world, mut session, _) = session_open_settings();
        let orig_master = session.settings.audio.master;
        let orig_music = session.settings.audio.music;
        assert!(!session.settings.audio.master_muted);

        let p = mute_label_point(0);
        pointer_down(&mut world, &mut session, p);
        pointer_up(&mut world, &mut session, p, false);
        drain_pending_setting_change_memory(&mut world, &mut session);

        assert!(session.settings.audio.master_muted, "flag must flip");
        assert_eq!(
            session.settings.audio.master, orig_master,
            "stored level unchanged"
        );
        assert_eq!(
            session.settings.audio.music, orig_music,
            "other level unchanged"
        );
    }

    #[test]
    fn muting_zeroes_gains_without_touching_stored_levels() {
        let (mut world, mut session, audio) = session_open_settings();
        let orig_music_level = session.settings.audio.music;
        assert!(orig_music_level > 0);

        let p = mute_label_point(1); // Music mute label
        pointer_down(&mut world, &mut session, p);
        pointer_up(&mut world, &mut session, p, false);
        drain_pending_setting_change_memory(&mut world, &mut session);

        assert!(session.settings.audio.music_muted);
        assert_eq!(
            session.settings.audio.music, orig_music_level,
            "stored level unchanged"
        );
        assert_eq!(
            audio.sink().gains().music_basis_points,
            0,
            "gain zeroed while muted"
        );
    }

    #[test]
    fn unmuting_restores_gain_to_stored_level() {
        let (mut world, mut session, audio) = session_open_settings();
        session.settings.audio.sfx_muted = true;
        session.settings.audio.sfx = 60;

        // SFX mute label is at content_y=928, outside viewport at offset=0; scroll to max.
        use mmd_engine::rts::settings_max_scroll;
        let offset = settings_max_scroll();
        session.ui.settings_scroll_px = offset;
        let rect = mmd_engine::rts::MUTE_LABEL_RECTS[3];
        let p = [rect[0] + rect[2] - 1.0, rect[1] + 1.0 - offset];
        pointer_down(&mut world, &mut session, p);
        pointer_up(&mut world, &mut session, p, false);
        drain_pending_setting_change_memory(&mut world, &mut session);

        assert!(
            !session.settings.audio.sfx_muted,
            "must flip back to unmuted"
        );
        assert!(
            audio.sink().gains().sfx_basis_points > 0,
            "gain restored after unmute"
        );
    }

    #[test]
    fn mute_commit_pushes_gains_and_saves_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SettingsStore::at(dir.path().join("settings-v1.json"));
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();

        let result = commit_setting_change::<FakeWindow>(
            &mut world,
            None,
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::MasterMuted(true),
        );
        assert!(result.is_ok(), "{result:?}");
        assert!(settings.audio.master_muted);
        assert_eq!(audio.gain_calls(), 1, "exactly one gain push per commit");
        let bytes = std::fs::read(dir.path().join("settings-v1.json")).expect("file written");
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(
            text.contains("\"master_muted\": true"),
            "persisted muted flag"
        );
    }

    #[test]
    fn mute_gain_failure_rolls_back_flag_and_gains() {
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();
        audio.set_fail_set_gains(true);

        let result = commit_setting_change::<FakeWindow>(
            &mut world,
            None,
            None,
            &mut settings,
            &mut audio,
            SettingsChange::MusicMuted(true),
        );
        assert!(result.is_err(), "gain failure must be reported");
        assert!(
            !settings.audio.music_muted,
            "flag must roll back on gain failure"
        );
    }

    #[test]
    fn mute_save_failure_rolls_back_flag_and_gains() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings-v1.json");
        std::fs::create_dir(&path).expect("seed directory in place of file");
        let store = SettingsStore::at(path);
        let mut settings = RtsSettings::default();
        let mut world = test_world();
        let mut audio = FakeAudioSink::new();
        let old_gains = effective_gains(&settings.audio);

        let result = commit_setting_change::<FakeWindow>(
            &mut world,
            None,
            Some(&store),
            &mut settings,
            &mut audio,
            SettingsChange::VoiceMuted(true),
        );
        assert!(result.is_err(), "save failure must be reported");
        assert!(
            !settings.audio.voice_muted,
            "flag must roll back on save failure"
        );
        assert_eq!(
            audio.gains(),
            old_gains,
            "gains must roll back on save failure"
        );
    }
}

impl NumericEdit {
    /// Test-only length accessor (field is private).
    #[cfg(test)]
    fn len_for_test(&self) -> u8 {
        self.len
    }
}
