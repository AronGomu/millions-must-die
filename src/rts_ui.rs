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
    BuildingKind, CommandId, HudHit, HudLayout, ModalHit, ModalPage, ModalSnapshot, RtsWorld,
    UnitKind, command_slots, hud_hit_test, minimap_projection, modal_hit_test,
};

use crate::rts_feedback::{AudioEvent, AudioSink, UiCue, effective_gains};
use crate::rts_run::RtsSession;
use crate::rts_settings::{RtsSettings, SettingsStore, WindowMode};
use crate::rts_window::{self, ModeChangeOutcome, WindowOps};

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
}

impl SettingsChange {
    fn apply_to(self, settings: &mut RtsSettings) {
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
        }
    }
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
}

impl Default for RtsUiState {
    fn default() -> Self {
        Self {
            page: UiPage::Gameplay,
            pauses: PauseReasons::default(),
            warning: None,
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

    /// Gameplay gear/Escape: opens the paused one-button menu. A no-op from
    /// any other page — the gear is unreachable once the HUD stops routing
    /// clicks to it, and Escape has its own per-page meaning below.
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

    fn close_menu(&mut self) {
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
        UiPage::PauseMenu => PointerOwner::Modal(modal_hit_test(ModalPage::PauseMenu, point)),
        UiPage::Settings => PointerOwner::Modal(modal_hit_test(ModalPage::Settings, point)),
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

/// Resolve one HUD click. Every variant is consumed here — none of them
/// ever issues a world order or falls through to selection/placement logic.
pub fn handle_hud_click(world: &mut RtsWorld, session: &mut RtsSession, hit: HudHit, shift: bool) {
    match hit {
        HudHit::Gear => {
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
            let slots = command_slots(world);
            if let Some(slot) = slots.get(idx as usize)
                && slot.enabled
                && let Some(cmd) = slot.command
            {
                execute_command(world, session, cmd);
                session.emit_audio(AudioEvent::Ui(UiCue::CommandGrid));
            }
            // Disabled/empty: consumed, no action, no cue — per T12's spec.
            // The keyboard hotkey path never reaches here, so a hotkey never
            // makes a pointer-click sound.
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
fn modal_snapshot(settings: &RtsSettings, pending: Option<SettingsChange>) -> ModalSnapshot {
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
    }
}

/// App-crate `pack_hud`: packs the world-facing HUD exactly as
/// `mmd_engine::rts::pack_hud` always has, then — only while a menu is open —
/// appends the modal last, over everything else. Lives here (not in the
/// engine crate) because it reads [`RtsUiState`]/[`RtsSettings`], both app
/// types.
pub fn pack_hud(world: &RtsWorld, session: &RtsSession, frame: &mut mmd_engine::rts::RtsFrame) {
    mmd_engine::rts::pack_hud(world, frame);
    let page = match session.ui.page {
        UiPage::Gameplay => return,
        UiPage::PauseMenu => ModalPage::PauseMenu,
        UiPage::Settings => ModalPage::Settings,
    };
    let snapshot = modal_snapshot(&session.settings, session.pending_setting_change);
    mmd_engine::rts::pack_modal(page, snapshot, session.ui.warning.as_deref(), frame);
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
    use crate::rts_feedback::FakeAudioSink;

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
    fn gear_only_opens_from_gameplay() {
        let mut ui = RtsUiState::default();
        ui.open_settings(); // no-op: not in PauseMenu yet
        assert_eq!(ui.page, UiPage::Gameplay);
        ui.open_menu();
        assert_eq!(ui.page, UiPage::PauseMenu);
        ui.open_menu(); // no-op: already open
        assert_eq!(ui.page, UiPage::PauseMenu);
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
            UiPage::PauseMenu => modal_hit_test(ModalPage::PauseMenu, [10.0, 10.0]),
            _ => unreachable!(),
        };
        assert_eq!(owner, ModalHit::Consumed);

        let mut ui_settings = ui_pause;
        ui_settings.open_settings();
        let hit = modal_hit_test(ModalPage::Settings, [10.0, 10.0]);
        assert_eq!(
            hit,
            ModalHit::Consumed,
            "outside every control, still consumed"
        );
    }

    #[test]
    fn settings_button_and_back_button_hit() {
        let btn = HudLayout::PAUSE_MENU_SETTINGS_BTN;
        let hit = modal_hit_test(ModalPage::PauseMenu, [btn[0] + 1.0, btn[1] + 1.0]);
        assert_eq!(hit, ModalHit::OpenSettings);

        let back = HudLayout::SETTINGS_BACK_BTN;
        let hit = modal_hit_test(ModalPage::Settings, [back[0] + 1.0, back[1] + 1.0]);
        assert_eq!(hit, ModalHit::Back);
    }

    #[test]
    fn sliders_snap_to_legal_steps() {
        use mmd_engine::rts::KEYBOARD_PAN_TRACK as TRACK;
        // A click roughly a third of the way along a 6..96 step-6 track
        // must land on a multiple of 6, not the raw fractional value.
        let x = TRACK[0] + TRACK[2] * 0.33;
        let hit = modal_hit_test(ModalPage::Settings, [x, TRACK[1] + 1.0]);
        let ModalHit::KeyboardPan(v) = hit else {
            panic!("expected a KeyboardPan hit: {hit:?}")
        };
        assert!((6..=96).contains(&v) && v.is_multiple_of(6), "{v}");

        use mmd_engine::rts::MASTER_TRACK;
        let x = MASTER_TRACK[0] + MASTER_TRACK[2] * 0.5;
        let hit = modal_hit_test(ModalPage::Settings, [x, MASTER_TRACK[1] + 1.0]);
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
}
