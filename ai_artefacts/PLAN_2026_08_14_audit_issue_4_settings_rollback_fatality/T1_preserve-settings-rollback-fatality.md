# T1: Preserve settings rollback fatality

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_4_settings_rollback_fatality.md`
**Depends:** none
**Commit outcome:** primary settings failure warns only after verified rollback; any failed compensation aborts live session after GPU reclaim.

## Context (self-contained)

- Goal: fix audit F4 (`a69ef20e743616f0`). `src/rts_ui.rs::commit_setting_change` currently drops compensation errors at lines 423/434-435. `commit_setting_change_live` then puts fatal mode rollback failure inside `LiveCommit.result`; `src/rts_run.rs` renders warning, continues with cfg/runtime divergence.
- This slice: structural txn error class; exhaustive reverse compensation; mode rollback detail; deterministic fault seams; live/scripted fatal propagation; focused tests; manual proof boundary.
- Out of scope here: other audit findings; settings schema/control changes; persistence algorithm redesign; unrelated SDL/audio/window refactor; new deps.
- Assumptions in force:
  - Forward order stays runtime → gains → save → camera speeds/in-memory publish.
  - Reverse order = gains → runtime. Attempt both, collect both errors. Never fail fast inside compensation.
  - Candidate cfg remains unpublished on every failure. Disk behavior stays owned by `SettingsStore::save`.
  - `ModeChangeOutcome::RolledBack(primary)` = known-old runtime → recoverable. Requested mode + old-mode rollback failure = fatal.
  - `LiveCommit.result` contains only `Ok(())` or verified-recoverable `Err(primary)`; fatal txn/lifecycle failures use outer `Err(String)`.
  - Mode txn fatal: `release_claim` → txn/compensation → `reclaim` → outer fatal. Skip `viewport` after txn fatal. Reclaim failure text joins txn fatal text; neither failure disappears.
  - Non-mode fatal has no claim dance; return outer fatal immediately.
  - Scripted path has no window/store. Its only reachable fatal txn is failed audio compensation; latch into existing `RtsSession::audio_fatal`, never warning.

## Requirements

### Error contracts

- Add to `src/rts_window.rs`:

```rust
#[derive(Debug)]
pub struct ModeTransitionError {
    pub primary: String,
    pub rollback: String,
}

pub fn transition_window_mode<W: WindowOps>(
    window: &mut W,
    current: WindowMode,
    requested: WindowMode,
) -> Result<ModeChangeOutcome, ModeTransitionError>;
```

- `ModeTransitionError` implements `Display` + `Error`. Display exact template:
  `window mode change failed ({primary}); rollback also failed ({rollback}) — window state is indeterminate, restart the app`.
- `transition_window_mode`: convert each `RunError` with `.to_string()`; preserve `ModeChangeOutcome::Applied` + `ModeChangeOutcome::RolledBack(String)` unchanged. No string parsing in `rts_ui`.
- Add to `src/rts_ui.rs`:

```rust
#[derive(Debug, PartialEq, Eq)]
pub enum SettingsCommitError {
    Recoverable { primary: String },
    Fatal {
        primary: String,
        compensation_failures: Vec<String>,
    },
}

pub fn commit_setting_change<W: WindowOps>(
    world: &mut RtsWorld,
    window: Option<&mut W>,
    store: Option<&SettingsStore>,
    settings: &mut RtsSettings,
    audio: &mut dyn AudioSink,
    change: SettingsChange,
) -> Result<(), SettingsCommitError>;
```

- `SettingsCommitError` implements `Display` + `Error`:
  - `Recoverable` → `{primary}`.
  - `Fatal` → `settings change failed ({primary}); compensation failed ({compensation_failures joined by "; "}) — runtime state is indeterminate, restart the app`.
- Validation failure maps to `Recoverable { primary: "refusing to apply invalid settings: ..." }`; no runtime touched, no compensation.
- Initial window mode failure:
  - `ModeChangeOutcome::RolledBack(reason)` → `Recoverable { primary: reason }`.
  - `ModeTransitionError { primary, rollback }` → `Fatal { primary, compensation_failures: vec![format!("window mode rollback failed: {rollback}")] }`.
- Initial confinement failure must reapply old `display.confine_pointer`. Success → recoverable; failure → fatal.
- Candidate `audio.set_gains` failure must compensate both possibly-partial audio gains + already-applied runtime. This fixes missing old-gain push at current line 422 path.
- Save failure must compensate gains + runtime.
- Compensation helper remains private in `src/rts_ui.rs`; it receives primary text plus booleans for stages needing compensation. It calls old gains first, old runtime second, regardless of first result. Empty failure vec → `Recoverable`; nonempty vec → `Fatal`.
- Compensation labels exact:
  - `audio gain rollback failed: {AudioError}`
  - `runtime rollback failed: {runtime error}`
- Runtime compensation treats every non-`Ok(())` as failure. `ModeChangeOutcome::RolledBack` while trying candidate → old means old was not reached, even though candidate was restored.

### Live propagation

- Keep `LiveCommit` fields unchanged:

```rust
pub struct LiveCommit {
    pub result: Result<(), String>,
    pub viewport: Option<DisplayViewport>,
}
```

- Keep `commit_setting_change_live(...) -> Result<LiveCommit, String>` public signature unchanged.
- Add exact private helper `fn soft_live_result(result: Result<(), SettingsCommitError>) -> Result<Result<(), String>, String>`. Do not use `to_string()` blindly:
  - txn success → inner `Ok(())`;
  - `Recoverable { primary }` → outer `Ok(inner Err(primary))`;
  - `Fatal { .. }` → outer `Err(error.to_string())`.
- Non-mode branch uses this classifier. Fatal compensation cannot enter `LiveCommit.result`.
- Mode branch stores typed txn answer, always calls `reclaim`, then:
  1. reclaim failed + txn fatal → outer error contains both txn text + `window reclaim after a mode change also failed: {e}`;
  2. reclaim failed otherwise → current `window reclaim after a mode change failed: {e}` outer error;
  3. reclaim succeeded + txn fatal → txn outer error; do not call `viewport`;
  4. reclaim succeeded + success/recoverable → refresh viewport, return `LiveCommit`.
- `src/rts_run.rs` live `MouseButtonUp` branch keeps existing outer-`Err` teardown: `release_window(&renderer, window)` then `return Err(RunError::Failed(e))`. Update comments to state failed txn compensation joins reclaim/viewport as outer fatal.
- `commit_scripted_setting_change`: match variants. Recoverable sets `SETTINGS NOT SAVED`; fatal leaves `session.ui.warning = None` plus calls `session.audio_fatal.get_or_insert(AudioError(error.to_string()))`. Existing interactive post-frame check then releases window + exits. Fake/offscreen sink cannot reach fatal.

### Deterministic fakes

- `src/rts_feedback.rs::FakeAudioSink` replaces bool fault with call schedule:

```rust
fail_set_gains_on_calls: Vec<u32>

pub fn set_fail_set_gains_on_calls(&mut self, calls: &[u32]);
```

- `set_fail_set_gains_on_calls` replaces schedule, not appends. `set_gains` increments `gain_calls` first; matching 1-based call returns `AudioError(format!("injected set_gains failure on call {}", self.gain_calls))` without mutating gains. Remove `set_fail_set_gains(bool)` after migrating sole test caller.
- Add `FakeSinkHandle::sink_mut() -> RefMut<'_, FakeAudioSink>` under `#[cfg(test)]` for scripted caller test. No production SDL seam change.
- In `src/rts_ui.rs::tests::FakeWindow`, replace single persistent `fail` with exact step-occurrence schedule:

```rust
failures: Vec<(&'static str, u32)>
viewport_calls: Cell<u32>

fn failing_on(mut self, step: &'static str, occurrence: u32) -> Self;
fn call_count(&self, step: &str) -> u32;
```

- `record` pushes log, computes 1-based occurrence for exact step, fails only scheduled pair. `viewport` increments `viewport_calls` before scheduled viewport check. Existing tests migrate: `set_size`/`reclaim`/`viewport` occurrence 1.

## Inputs

- Spec: `/home/aron/projects/millions_must_die/.tmp/MAKE_AUDIT_2026_08_14_millions-must-die_e7fe9dee0277/F4_settings-rollback-fatality.md`.
- Audit proof: same dir `validate-c02-settings-rollback.md`.
- Production txn: `src/rts_ui.rs::{commit_setting_change, commit_setting_change_live, apply_runtime, LiveCommit}`.
- Window compensation: `src/rts_window.rs::{transition_window_mode, ModeChangeOutcome, WindowOps, ClaimedWindow}`.
- Audio partial mutation: `src/rts_audio.rs::AudioEngine<S>::set_gains` mutates streams sequentially before `Err`.
- Audio seam: `src/rts_feedback.rs::{FakeAudioSink, FakeSinkHandle}`.
- Live caller: `src/rts_run.rs` `MouseButtonUp` settings branch + `commit_scripted_setting_change`.
- Manual proof: `ai_artefacts/manual_test_checklist.md`, `## T13 settings-menu`.
- **From Depends:** none. Baseline at `e7fe9dee0277d84da78f6436832aa05e3f95f71d`; focused baseline = 24 `rts_ui::tests`, 95 bin unit tests.

## TDD

- [ ] **Red R1 — mode fatal:** add `a_mode_change_with_failed_internal_rollback_is_fatal_after_reclaim`; run only it. **Criterion:** old code exits nonzero at outer-fatal assertion. Do not require later log/count assertions during red; those become green checks.
- [ ] **Red R2 — audio recoverable/fatal split:** replace `a_failed_gain_push_rolls_back_like_a_failed_save` with `an_audio_failure_with_successful_compensation_stays_recoverable`; add `an_audio_failure_with_failed_audio_compensation_is_fatal`; run each separately. **Criterion:** old code fails classification; red pass need not reach post-classification counts.
- [ ] **Red R3 — all compensations:** add `a_save_failure_attempts_every_compensation_and_any_failure_is_fatal`; run alone. **Criterion:** old code fails outer-fatal assertion. Green phase later requires both compensation attempts/fragments.
- [ ] **Red R4 — scripted caller:** add `scripted_fatal_setting_commit_latches_audio_fatal_not_warning` in `src/rts_run.rs::tests`. **Criterion:** typed-error integration test fails before caller handling exists; no native window/audio needed.
- [ ] **Green G1 — seams:** implement exact mode/audio/window fake contracts above. **Criterion:** all old tests compile; fault injection selects exact 1-based calls.
- [ ] **Green G2 — txn:** implement typed runtime/txn failures + exhaustive reverse compensation. **Criterion:** R1-R3 pass; settings cfg never publishes on failure.
- [ ] **Green G3 — callers:** classify `commit_setting_change_live` after reclaim; wire scripted fatal latch. **Criterion:** R4 passes; fatal never reaches warning branch.
- [ ] **Refactor:** update stale doc comments only around touched txn/caller symbols; remove old bool fault API/imports. **Criterion:** no `let _ = audio.set_gains`, `let _ = apply_runtime`, or `set_fail_set_gains(` remains in txn/tests.

## Test plan

| Test | Input/fault schedule | Exact expect |
| --- | --- | --- |
| `a_refused_mode_change_still_reclaims` (strengthen) | windowed `set_size#1` fails; internal old-mode rollback succeeds | outer `Ok`; inner `Err`; cfg old; `release_claim#1`, `reclaim#1`, `viewport#1`; gain calls 0 |
| `a_mode_change_with_failed_internal_rollback_is_fatal_after_reclaim` | windowed `sync#1` + borderless rollback `sync#2` fail | outer `Err` contains primary sync, `window mode rollback failed`, `indeterminate`, `restart the app`; release 1; reclaim 1; viewport 0; gain calls 0; cfg old |
| `an_audio_failure_with_successful_compensation_stays_recoverable` | `Confine(false)`; gain call 1 fails only | outer `Ok`; inner error exactly `audio gain failed: injected set_gains failure on call 1`; gain calls 2; gains old; mouse-grab calls 2; `grabbed=true`; cfg old; no claim calls/disk write |
| `an_audio_failure_with_failed_audio_compensation_is_fatal` | `Master(50)`; gain calls 1 + 2 fail | outer `Err` contains primary call 1 + `audio gain rollback failed: injected set_gains failure on call 2` + indeterminate/restart; gain calls 2; cfg old; no window ops |
| `a_save_failure_attempts_every_compensation_and_any_failure_is_fatal` | target path = non-empty dir; `Confine(false)`; gain call 2 fails; `set_mouse_grab#2` fails | outer `Err` contains `save failed:`, audio rollback label, runtime rollback label, indeterminate/restart; gain calls exactly 2; mouse-grab calls exactly 2 despite audio rollback failure; cfg old; `grabbed=false` exposes failed runtime rollback |
| `a_failed_reclaim_is_fatal_not_a_warning` (retain) | `reclaim#1` fails | outer fatal contains reclaim; never inner warning |
| `scripted_fatal_setting_commit_latches_audio_fatal_not_warning` | session pending `Master(50)`; shared fake gain calls 1 + 2 fail | `session.ui.warning == None`; `take_audio_fatal()` contains primary + audio rollback failure; second take = `None` |

## Impl steps

- [ ] **I1** Edit `src/rts_window.rs`: add `ModeTransitionError`; return structured primary/rollback strings from `transition_window_mode`; migrate existing two mode-transition tests. **Criterion:** success + recovered rollback behavior unchanged; fatal test checks both public fields plus existing `indeterminate`/`restart the app` display.
- [ ] **I2** Edit `src/rts_feedback.rs`: add 1-based gain failure schedule + `FakeSinkHandle::sink_mut`; migrate sole old bool caller. **Criterion:** call 1 may fail while call 2 succeeds; matching failed call leaves recorded gains unchanged.
- [ ] **I3** Edit `src/rts_ui.rs` test fake with step-occurrence schedule + viewport count; migrate existing fake construction. **Criterion:** forward and rollback occurrences fail independently without affecting unrelated steps.
- [ ] **I4** Add R1-R3 tests against production `commit_setting_change`/`commit_setting_change_live`; do not create replica txn helper in tests. **Criterion:** each test calls production seam named in table; no direct call to private compensation helper.
- [ ] **I5** Add `SettingsCommitError`, internal runtime classification, compensation collector; change `commit_setting_change` return type. **Criterion:** every primary error retains text; every scheduled compensation runs; vec order = audio then runtime.
- [ ] **I6** Rewrite `commit_setting_change_live` classification/reclaim ordering. **Criterion:** only verified rollback enters `LiveCommit.result`; mode fatal test logs reclaim before outer return; viewport count stays 0.
- [ ] **I7** Edit `src/rts_run.rs::commit_scripted_setting_change` + comments/imports; add R4. Keep live SDL outer-Err branch teardown unchanged except comment. **Criterion:** fatal typed txn cannot produce `SETTINGS NOT SAVED`; live outer fatal still releases window before `RunError::Failed` return.
- [ ] **I8** Edit one existing save-failure bullet under `ai_artefacts/manual_test_checklist.md` `## T13 settings-menu`: state ordinary save failure + successful rollback shows warning, reverts control/runtime, session continues; failed compensation is intentionally automated-only because inducing native display/audio rollback failure is unsafe/non-portable, expected behavior = window release + exit 1 with primary + rollback context. **Criterion:** no claim that manual run executed; proof boundary explicit.
- [ ] **I9** Run fmt after green; inspect focused diff only. **Criterion:** touched app files limited to `src/rts_ui.rs`, `src/rts_window.rs`, `src/rts_feedback.rs`, `src/rts_run.rs`, checklist.

## Outputs

- Modified impl: `src/rts_ui.rs`, `src/rts_window.rs`, `src/rts_feedback.rs`, `src/rts_run.rs`.
- Modified manual doc: `ai_artefacts/manual_test_checklist.md`.
- Public API: `SettingsCommitError`; `ModeTransitionError`; typed `commit_setting_change` return. `LiveCommit` + `commit_setting_change_live` signatures stay source-compatible.
- Behavior: verified primary failure → warning/continue; any failed compensation → outer fatal/teardown; mode fatal always attempts reclaim first.
- Config/deps/migrations: none.

## Validation

- [ ] `cargo fmt --all -- --check` **Criterion:** exit 0; no fmt diff.
- [ ] `cargo test --locked --bin millions_must_die rts_ui::tests -- --nocapture` **Criterion:** exit 0; exactly 27 passed, 0 failed.
- [ ] `cargo test --locked --bin millions_must_die scripted_fatal_setting_commit_latches_audio_fatal_not_warning -- --nocapture` **Criterion:** exit 0; exactly 1 passed, 0 failed.
- [ ] `cargo test --locked --bin millions_must_die -- --nocapture` **Criterion:** exit 0; exactly 99 passed, 0 failed.
- [ ] `cargo test --workspace --locked` **Criterion:** exit 0; all workspace tests pass; no ignored-test failure.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` **Criterion:** exit 0; no warning.
- [ ] `SDL_VIDEODRIVER=offscreen cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` **Criterion:** exit 0; clean-exit suffix remains `body_overlaps=0 ui_page=gameplay music_starts=1 voice_select=8 voice_order=9 voice_reject=1 sfx_ui=8 keyboard_pan=78`; never claim native failure proof from offscreen.
- [ ] `rg -n 'let _ = (audio\.set_gains|apply_runtime)|set_fail_set_gains\(' src/rts_ui.rs src/rts_feedback.rs` **Criterion:** exit 1 with no matches; no discarded compensation/old bool injector.
- [ ] `git diff --check` **Criterion:** exit 0.
- [ ] Manual checklist review **Criterion:** T13 bullet distinguishes recoverable continuation from fatal failed compensation; automated-only native failure boundary stated.
- [ ] App functional **Criterion:** normal settings commit still updates runtime/gains/store/camera/cfg once; existing focused success tests pass.
- [ ] Commit msg draft: `fix(rts): make failed settings compensation fatal` **Criterion:** one DCO-signed impl commit; no unrelated files.
