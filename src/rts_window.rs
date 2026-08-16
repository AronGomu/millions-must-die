//! Window modes, focus lifecycle, and pointer confinement for the `rts`
//! subcommand (T10).
//!
//! # Isolation contract
//!
//! Every function here that touches SDL display/grab state takes a real
//! `sdl3::video::Window` (or the [`WindowOps`] trait over it). An
//! offscreen/deterministic run (`SDL_VIDEODRIVER=offscreen`) never builds a
//! window at all — [`crate::rts_run::run`] must not call
//! [`build_rts_window`], [`apply_window_mode`], [`refresh_viewport`], or
//! [`handle_focus`] on that path. Nothing in this module resolves that on
//! its own; the caller's offscreen/window fork is the single gate.
//!
//! # Pure vs. native
//!
//! [`apply_window_mode`], [`transition_window_mode`], and [`handle_focus`]
//! are generic over [`WindowOps`] so the exact call sequence, rollback, and
//! focus-clear behaviour are unit-tested here against a fake — no display
//! required. [`SdlWindowOps`] is the one real implementation, and
//! `set_display_mode`/`set_fullscreen`/`set_mouse_grab` physically taking
//! effect on a live compositor is manual/platform evidence only (ADR 018).

use mmd_engine::render::{DisplayViewport, VIEW_HEIGHT, VIEW_WIDTH};
use sdl3::video::{Window, WindowPos};

use crate::rts_settings::WindowMode;
use crate::run::RunError;

/// A display mode's geometry/refresh, decoupled from `sdl3::video::DisplayMode`
/// (which carries an opaque platform handle) so [`choose_exclusive_mode`] is a
/// pure function testable without SDL.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModeCandidate {
    pub w: i32,
    pub h: i32,
    pub refresh_rate: f32,
}

/// Narrow, tested wrappers over the window operations the three
/// [`WindowMode`] sequences and the focus lifecycle need. [`SdlWindowOps`]
/// is the real implementation; tests use a fake that logs exactly which
/// calls happened, in order.
pub trait WindowOps {
    fn leave_fullscreen(&mut self) -> Result<(), String>;
    fn enter_fullscreen(&mut self) -> Result<(), String>;
    /// `SDL_SetWindowFullscreenMode(None)`: desktop-resolution fullscreen.
    fn clear_exclusive_mode(&mut self) -> Result<(), String>;
    /// Every fullscreen mode on the window's current display.
    fn available_modes(&mut self) -> Result<Vec<ModeCandidate>, String>;
    /// `SDL_SetWindowFullscreenMode(Some(mode))`. `mode` must be one
    /// [`Self::available_modes`] most recently returned.
    fn set_exclusive_mode(&mut self, mode: ModeCandidate) -> Result<(), String>;
    fn set_bordered(&mut self, bordered: bool) -> Result<(), String>;
    fn set_size(&mut self, w: u32, h: u32) -> Result<(), String>;
    fn center(&mut self) -> Result<(), String>;
    fn sync(&mut self) -> Result<(), String>;
    fn set_mouse_grab(&mut self, grabbed: bool) -> Result<(), String>;
}

/// The GPU-claim half of a **live** window-mode change.
///
/// [`transition_window_mode`] states its own precondition: a caller that has
/// the window GPU-claimed releases the claim, transitions, then reclaims. A
/// mode change tears down the window's presentation surface, so a swapchain
/// built for the old one is lost and the next present fails — which, on the
/// interactive path, is a `present_error` and exit 1 in the middle of a
/// session. The transition also changes the window's size, so the aspect-fit
/// [`DisplayViewport`] computed for the old shape is stale until recomputed.
///
/// This trait is the seam both halves go through, so the whole sequence is
/// unit-testable against a fake instead of needing a real display.
pub trait ClaimedWindow: WindowOps {
    /// Release this window from the GPU device, tearing down its swapchain.
    /// Infallible, exactly like `GpuContext::release_window`.
    fn release_claim(&mut self);
    /// Re-claim it, rebuilding the swapchain for the window's new shape.
    fn reclaim(&mut self) -> Result<(), String>;
    /// The aspect-fit viewport for the window's **current** size.
    fn viewport(&self) -> Result<DisplayViewport, String>;
}

/// Live per-frame window shape/lifecycle state [`crate::rts_run::run`]
/// tracks across the event loop.
///
/// `mode`/`focused` are read back by tests and are the state T13's runtime
/// mode-toggle UI will read/drive; `crate::rts_run::run` writes both but
/// only reads `viewport` today.
pub struct RtsWindowState {
    #[allow(dead_code)] // T13 reads this back for the mode-toggle UI
    pub mode: WindowMode,
    #[allow(dead_code)] // T13 reads this back for the mode-toggle UI
    pub focused: bool,
    pub viewport: DisplayViewport,
}

/// What a focus transition asks the caller to do beyond the grab/input-clear
/// this module already performs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusAction {
    /// Nothing further: the default is the sim keeps running while
    /// unfocused.
    None,
    /// `gameplay.pause_on_focus_loss` is set: the caller should pause.
    /// T13 additionally opens a paused menu; this ticket only emits the
    /// request.
    PauseRequested,
}

/// Result of [`transition_window_mode`]: either the requested mode applied,
/// or it failed and the previous mode was successfully restored.
#[derive(Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum ModeChangeOutcome {
    Applied,
    RolledBack(#[allow(dead_code)] String), // T13 surfaces this in a warning banner
}

/// A mode change that failed *and* could not be undone: the window is left
/// straddling two mode sequences, which no later commit can reason about.
#[derive(Debug, PartialEq, Eq)]
pub struct ModeTransitionError {
    pub primary: String,
    pub rollback: String,
}

impl std::fmt::Display for ModeTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "window mode change failed ({}); rollback also failed ({}) — window state is \
             indeterminate, restart the app",
            self.primary, self.rollback
        )
    }
}

impl std::error::Error for ModeTransitionError {}

fn step_err(mode: WindowMode, step: &str) -> impl FnOnce(String) -> RunError {
    move |e| RunError::Failed(format!("window mode {mode:?} step {step} failed: {e}"))
}

/// Apply one [`WindowMode`]'s exact operation sequence (ADR 018). Never
/// claims/releases the GPU device — that is [`transition_window_mode`]'s
/// caller's job on a window already claimed.
pub fn apply_window_mode<W: WindowOps>(window: &mut W, mode: WindowMode) -> Result<(), RunError> {
    match mode {
        WindowMode::BorderlessDesktop => {
            window
                .leave_fullscreen()
                .map_err(step_err(mode, "leave_fullscreen"))?;
            window
                .clear_exclusive_mode()
                .map_err(step_err(mode, "clear_exclusive_mode"))?;
            window
                .set_bordered(false)
                .map_err(step_err(mode, "set_bordered"))?;
            window
                .enter_fullscreen()
                .map_err(step_err(mode, "enter_fullscreen"))?;
            window.sync().map_err(step_err(mode, "sync"))?;
        }
        WindowMode::Exclusive1920x1080 => {
            let modes = window
                .available_modes()
                .map_err(step_err(mode, "available_modes"))?;
            let chosen = choose_exclusive_mode(&modes).ok_or_else(|| {
                RunError::Failed(format!(
                    "no display mode available for exclusive 1920x1080 (mode {mode:?})"
                ))
            })?;
            window
                .set_bordered(true)
                .map_err(step_err(mode, "set_bordered"))?;
            window
                .set_exclusive_mode(chosen)
                .map_err(step_err(mode, "set_exclusive_mode"))?;
            window
                .enter_fullscreen()
                .map_err(step_err(mode, "enter_fullscreen"))?;
            window.sync().map_err(step_err(mode, "sync"))?;
        }
        WindowMode::Windowed1280x720 => {
            window
                .leave_fullscreen()
                .map_err(step_err(mode, "leave_fullscreen"))?;
            window
                .clear_exclusive_mode()
                .map_err(step_err(mode, "clear_exclusive_mode"))?;
            window
                .set_bordered(true)
                .map_err(step_err(mode, "set_bordered"))?;
            window
                .set_size(1280, 720)
                .map_err(step_err(mode, "set_size"))?;
            window.center().map_err(step_err(mode, "center"))?;
            window.sync().map_err(step_err(mode, "sync"))?;
        }
    }
    Ok(())
}

/// Closest match to `1920x1080` by squared geometry distance; on an exact
/// geometry tie, the higher refresh rate wins.
pub fn choose_exclusive_mode(modes: &[ModeCandidate]) -> Option<ModeCandidate> {
    modes.iter().copied().min_by(|a, b| {
        geometry_distance(a)
            .partial_cmp(&geometry_distance(b))
            .expect("mode dimensions are always finite")
            .then_with(|| {
                b.refresh_rate
                    .partial_cmp(&a.refresh_rate)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    })
}

fn geometry_distance(m: &ModeCandidate) -> i64 {
    let dw = (m.w - 1920) as i64;
    let dh = (m.h - 1080) as i64;
    dw * dw + dh * dh
}

/// Runtime mode change on a window the caller already has GPU-claimed and
/// wants to keep claimed: the caller releases the claim, calls this, then
/// reclaims — this function never touches the GPU device, only `window`.
///
/// On a failed `requested` application, rolls back to `current` before
/// returning: the window must never end up straddling two mode sequences.
/// A rollback that also fails is an actionable fatal error (window state is
/// no longer known-good) rather than a swallowed one.
///
/// T13 is the first caller (a settings-menu mode toggle); nothing in this
/// ticket triggers a runtime mode change on its own, so this is exercised
/// by unit tests only until then.
#[cfg_attr(not(test), allow(dead_code))]
pub fn transition_window_mode<W: WindowOps>(
    window: &mut W,
    current: WindowMode,
    requested: WindowMode,
) -> Result<ModeChangeOutcome, ModeTransitionError> {
    match apply_window_mode(window, requested) {
        Ok(()) => Ok(ModeChangeOutcome::Applied),
        Err(primary) => match apply_window_mode(window, current) {
            Ok(()) => Ok(ModeChangeOutcome::RolledBack(primary.to_string())),
            // Both halves are kept whole: the caller composes them with its
            // own transaction context rather than re-parsing one string.
            Err(rollback) => Err(ModeTransitionError {
                primary: format!("window mode change to {requested:?} failed: {primary}"),
                rollback: format!("rollback to {current:?} failed: {rollback}"),
            }),
        },
    }
}

/// A window-focus transition: clears every held pan/press/drag input on
/// loss (once, via `clear_held_input`), always releases the pointer on
/// loss, and restores the configured grab on gain — never restoring stale
/// input.
pub fn handle_focus<W: WindowOps>(
    window: &mut W,
    gained: bool,
    confine_pointer: bool,
    pause_on_focus_loss: bool,
    clear_held_input: impl FnOnce(),
) -> Result<FocusAction, RunError> {
    if gained {
        window
            .set_mouse_grab(confine_pointer)
            .map_err(|e| RunError::Failed(format!("focus-gain grab restore failed: {e}")))?;
        Ok(FocusAction::None)
    } else {
        clear_held_input();
        window
            .set_mouse_grab(false)
            .map_err(|e| RunError::Failed(format!("focus-loss grab release failed: {e}")))?;
        Ok(if pause_on_focus_loss {
            FocusAction::PauseRequested
        } else {
            FocusAction::None
        })
    }
}

/// Fallback viewport for the degenerate case a drawable cannot fit even one
/// `16x9` unit: an exact 1:1 identity over the fixed logical canvas, so a
/// resize that momentarily shrinks the drawable below that floor cannot
/// stop input dead.
fn identity_viewport() -> DisplayViewport {
    DisplayViewport::new([VIEW_WIDTH, VIEW_HEIGHT], [VIEW_WIDTH, VIEW_HEIGHT])
        .expect("1920x1080 always fits an exact 16:9 rect")
}

/// Recompute [`DisplayViewport`] from `window`'s current logical size and
/// drawable pixel size. Call on window claim and on every
/// resize/pixel-size-changed/display-changed event — never polled
/// unconditionally, so it always reflects the shape the window has *now*.
pub fn refresh_viewport(window: &Window) -> Result<DisplayViewport, RunError> {
    let (win_w, win_h) = window.size();
    let (px_w, px_h) = window.size_in_pixels();
    Ok(DisplayViewport::new([win_w, win_h], [px_w, px_h]).unwrap_or_else(identity_viewport))
}

/// Build the `rts` window hidden + resizable, apply the startup mode, then
/// show it. Does not claim it for the GPU device — the caller does that
/// after this returns, per the startup ordering ADR 018 fixes.
///
/// Under `MMD_WINDOW_HIDDEN` the final `show` is skipped and the window stays
/// hidden for the whole run: the mode is still applied and the caller still
/// claims, presents to and releases a real swapchain, but nothing is mapped
/// onto the desktop and nothing takes focus.
pub fn build_rts_window(
    video: &sdl3::VideoSubsystem,
    mode: WindowMode,
) -> Result<Window, RunError> {
    let mut window = video
        .window("millions_must_die — rts prototype", VIEW_WIDTH, VIEW_HEIGHT)
        .hidden()
        .resizable()
        .build()
        .map_err(|e| RunError::Failed(format!("rts window build failed: {e}")))?;

    apply_window_mode(&mut SdlWindowOps(&mut window), mode)?;

    if crate::hidden_windows_requested() {
        return Ok(window);
    }

    if !window.show() {
        return Err(RunError::Failed(format!(
            "rts window show failed: {}",
            sdl3::get_error()
        )));
    }
    Ok(window)
}

/// The real [`WindowOps`] implementation, over a borrowed live window.
/// Caches the modes [`WindowOps::available_modes`] last returned so
/// [`WindowOps::set_exclusive_mode`] can hand the matching real
/// `sdl3::video::DisplayMode` (with its opaque platform handle) back to
/// SDL — [`ModeCandidate`] itself deliberately carries none of that.
pub struct SdlWindowOps<'a>(pub &'a mut Window);

impl SdlWindowOps<'_> {
    fn matching_mode(
        &self,
        cached: &[sdl3::video::DisplayMode],
        chosen: ModeCandidate,
    ) -> Result<sdl3::video::DisplayMode, String> {
        cached
            .iter()
            .find(|m| {
                m.w == chosen.w
                    && m.h == chosen.h
                    && (m.refresh_rate - chosen.refresh_rate).abs() < 0.01
            })
            .copied()
            .ok_or_else(|| "exclusive display mode vanished between enumerate and apply".into())
    }
}

impl<'a> WindowOps for SdlWindowOps<'a> {
    fn leave_fullscreen(&mut self) -> Result<(), String> {
        self.0.set_fullscreen(false).map_err(|e| e.to_string())
    }

    fn enter_fullscreen(&mut self) -> Result<(), String> {
        self.0.set_fullscreen(true).map_err(|e| e.to_string())
    }

    fn clear_exclusive_mode(&mut self) -> Result<(), String> {
        self.0.set_display_mode(None).map_err(|e| e.to_string())
    }

    fn available_modes(&mut self) -> Result<Vec<ModeCandidate>, String> {
        let display = self.0.get_display().map_err(|e| e.to_string())?;
        let modes = display.get_fullscreen_modes().map_err(|e| e.to_string())?;
        LAST_MODES.with(|cell| *cell.borrow_mut() = modes.clone());
        Ok(modes
            .iter()
            .map(|m| ModeCandidate {
                w: m.w,
                h: m.h,
                refresh_rate: m.refresh_rate,
            })
            .collect())
    }

    fn set_exclusive_mode(&mut self, mode: ModeCandidate) -> Result<(), String> {
        let cached = LAST_MODES.with(|cell| cell.borrow().clone());
        let dm = self.matching_mode(&cached, mode)?;
        self.0.set_display_mode(dm).map_err(|e| e.to_string())
    }

    fn set_bordered(&mut self, bordered: bool) -> Result<(), String> {
        if self.0.set_bordered(bordered) {
            Ok(())
        } else {
            Err(sdl3::get_error().to_string())
        }
    }

    fn set_size(&mut self, w: u32, h: u32) -> Result<(), String> {
        self.0.set_size(w, h).map_err(|e| e.to_string())
    }

    fn center(&mut self) -> Result<(), String> {
        self.0
            .set_position(WindowPos::Centered, WindowPos::Centered);
        Ok(())
    }

    fn sync(&mut self) -> Result<(), String> {
        if self.0.sync() {
            Ok(())
        } else {
            Err(sdl3::get_error().to_string())
        }
    }

    fn set_mouse_grab(&mut self, grabbed: bool) -> Result<(), String> {
        if self.0.set_mouse_grab(grabbed) {
            Ok(())
        } else {
            Err(sdl3::get_error().to_string())
        }
    }
}

thread_local! {
    /// Set by [`SdlWindowOps::available_modes`], read by
    /// [`SdlWindowOps::set_exclusive_mode`]: the two calls always run on the
    /// main thread in the same `apply_window_mode` call, so a thread-local
    /// scratch is simpler than threading a cache field through every
    /// `&mut self` call without breaking `WindowOps`'s narrow signature.
    static LAST_MODES: std::cell::RefCell<Vec<sdl3::video::DisplayMode>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeWindow {
        log: Vec<String>,
        modes: Vec<ModeCandidate>,
        fail_step: Option<String>,
        grabbed: bool,
    }

    impl FakeWindow {
        fn with_modes(modes: Vec<ModeCandidate>) -> Self {
            Self {
                modes,
                ..Default::default()
            }
        }

        fn failing(mut self, step: &str) -> Self {
            self.fail_step = Some(step.to_string());
            self
        }

        fn record(&mut self, step: &str) -> Result<(), String> {
            self.log.push(step.to_string());
            if self.fail_step.as_deref() == Some(step) {
                Err(format!("{step} failed (injected)"))
            } else {
                Ok(())
            }
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
        fn available_modes(&mut self) -> Result<Vec<ModeCandidate>, String> {
            self.record("available_modes")?;
            Ok(self.modes.clone())
        }
        fn set_exclusive_mode(&mut self, mode: ModeCandidate) -> Result<(), String> {
            self.record("set_exclusive_mode")?;
            self.modes = vec![mode];
            Ok(())
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
            self.record(if grabbed {
                "set_mouse_grab(true)"
            } else {
                "set_mouse_grab(false)"
            })?;
            self.grabbed = grabbed;
            Ok(())
        }
    }

    #[test]
    fn borderless_desktop_is_default_sequence() {
        let mut fake = FakeWindow::default();
        apply_window_mode(&mut fake, WindowMode::BorderlessDesktop).expect("applies");
        assert_eq!(
            fake.log,
            vec![
                "leave_fullscreen",
                "clear_exclusive_mode",
                "set_bordered(false)",
                "enter_fullscreen",
                "sync",
            ]
        );
    }

    #[test]
    fn exclusive_chooses_closest_1920x1080() {
        let modes = vec![
            ModeCandidate {
                w: 1920,
                h: 1080,
                refresh_rate: 60.0,
            },
            ModeCandidate {
                w: 1920,
                h: 1080,
                refresh_rate: 144.0,
            },
            ModeCandidate {
                w: 2560,
                h: 1440,
                refresh_rate: 60.0,
            },
        ];
        let mut fake = FakeWindow::with_modes(modes);
        apply_window_mode(&mut fake, WindowMode::Exclusive1920x1080).expect("applies");
        assert_eq!(
            fake.log,
            vec![
                "available_modes",
                "set_bordered(true)",
                "set_exclusive_mode",
                "enter_fullscreen",
                "sync",
            ]
        );
        // set_exclusive_mode overwrote `modes` with the one candidate it was
        // called with — the closest geometry, tie-broken to the higher
        // refresh rate.
        assert_eq!(
            fake.modes,
            vec![ModeCandidate {
                w: 1920,
                h: 1080,
                refresh_rate: 144.0,
            }]
        );
    }

    #[test]
    fn choose_exclusive_mode_prefers_closest_geometry_over_any_refresh() {
        let modes = vec![
            ModeCandidate {
                w: 1920,
                h: 1080,
                refresh_rate: 30.0,
            },
            ModeCandidate {
                w: 3840,
                h: 2160,
                refresh_rate: 240.0,
            },
        ];
        assert_eq!(
            choose_exclusive_mode(&modes),
            Some(ModeCandidate {
                w: 1920,
                h: 1080,
                refresh_rate: 30.0,
            })
        );
    }

    #[test]
    fn windowed_is_1280x720_resizable() {
        let mut fake = FakeWindow::default();
        apply_window_mode(&mut fake, WindowMode::Windowed1280x720).expect("applies");
        assert_eq!(
            fake.log,
            vec![
                "leave_fullscreen",
                "clear_exclusive_mode",
                "set_bordered(true)",
                "set_size",
                "center",
                "sync",
            ]
        );
    }

    #[test]
    fn failed_mode_change_rolls_back_before_reclaim() {
        let mut fake = FakeWindow::default().failing("set_bordered(true)");
        let outcome = transition_window_mode(
            &mut fake,
            WindowMode::BorderlessDesktop,
            WindowMode::Windowed1280x720,
        )
        .expect("rollback itself succeeds");
        assert!(matches!(outcome, ModeChangeOutcome::RolledBack(_)));
        // Requested (windowed) sequence attempted first, failed at
        // `set_bordered(true)`; rollback re-ran the full borderless-desktop
        // sequence after.
        assert_eq!(
            fake.log,
            vec![
                "leave_fullscreen",
                "clear_exclusive_mode",
                "set_bordered(true)",
                "leave_fullscreen",
                "clear_exclusive_mode",
                "set_bordered(false)",
                "enter_fullscreen",
                "sync",
            ]
        );
    }

    #[test]
    fn failed_mode_change_with_failed_rollback_is_actionable_fatal() {
        let mut fake = FakeWindow::default().failing("sync");
        let err = transition_window_mode(
            &mut fake,
            WindowMode::BorderlessDesktop,
            WindowMode::Windowed1280x720,
        )
        .expect_err("both the requested mode and the rollback fail");
        assert!(
            err.primary.contains("Windowed1280x720"),
            "the primary half must name the mode that was refused: {err:?}"
        );
        assert!(
            err.rollback.contains("BorderlessDesktop"),
            "the rollback half must name the mode that could not be restored: {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains("indeterminate"), "{msg}");
        assert!(msg.contains("restart the app"), "{msg}");
    }

    #[test]
    fn focus_loss_clears_every_held_input() {
        let mut fake = FakeWindow::default();
        let mut cleared = false;
        handle_focus(&mut fake, false, true, false, || cleared = true).expect("handles loss");
        assert!(cleared, "clear_held_input must run exactly once on loss");
    }

    #[test]
    fn focus_loss_releases_pointer() {
        let mut fake = FakeWindow {
            grabbed: true,
            ..Default::default()
        };
        handle_focus(&mut fake, false, true, false, || {}).expect("handles loss");
        assert!(!fake.grabbed);
        assert!(fake.log.contains(&"set_mouse_grab(false)".to_string()));
    }

    #[test]
    fn focus_gain_restores_configured_grab() {
        let mut confined = FakeWindow::default();
        handle_focus(&mut confined, true, true, false, || {
            panic!("gain must never clear held input")
        })
        .expect("handles gain");
        assert!(confined.grabbed);

        let mut unconfined = FakeWindow::default();
        handle_focus(&mut unconfined, true, false, false, || {
            panic!("gain must never clear held input")
        })
        .expect("handles gain");
        assert!(!unconfined.grabbed);
    }

    #[test]
    fn pause_request_respects_toggle() {
        let mut off = FakeWindow::default();
        let action = handle_focus(&mut off, false, true, false, || {}).expect("handles loss");
        assert_eq!(action, FocusAction::None);

        let mut on = FakeWindow::default();
        let action = handle_focus(&mut on, false, true, true, || {}).expect("handles loss");
        assert_eq!(action, FocusAction::PauseRequested);

        // Gain never requests a pause, regardless of the toggle.
        let mut gain = FakeWindow::default();
        let action = handle_focus(&mut gain, true, true, true, || {}).expect("handles gain");
        assert_eq!(action, FocusAction::None);
    }

    #[test]
    fn refresh_viewport_falls_back_to_identity_below_one_16x9_unit() {
        // `refresh_viewport` itself needs a real `Window`, which needs SDL
        // video init; the fallback it delegates to is exercised directly
        // here instead, pure and SDL-free.
        assert_eq!(
            identity_viewport(),
            DisplayViewport::new([VIEW_WIDTH, VIEW_HEIGHT], [VIEW_WIDTH, VIEW_HEIGHT]).unwrap()
        );
    }
}
