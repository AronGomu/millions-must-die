# T1: Preserve settings rollback fatality

**Plan:** `./artifacts/PLAN_2026_08_14_audit_issue_4_settings_rollback_fatality.md`
**Depends:** none
**Commit outcome:** verified rollback warns; any failed compensation aborts live session after reclaim, retaining txn + compensation + lifecycle context.

## Context (self-contained)

- Goal: fix audit F4 (`a69ef20e743616f0`). `src/rts_ui.rs::commit_setting_change` discards compensation errors. `commit_setting_change_live` demotes txn fatality into `LiveCommit.result`. `src/rts_run.rs` then warns + continues with cfg/runtime divergence.
- This slice: typed txn/runtime errors; exhaustive compensation; deterministic fault seams; live/scripted fatal propagation; production `MouseButtonUp` teardown seam; focused tests; manual proof boundary.
- Out of scope: other audit findings; settings schema/UI redesign; `SettingsStore::save` backup-restoration protocol; unrelated SDL/audio/window refactor; new deps; native double-failure injection.
- Assumptions in force:
  - Forward order stays runtime → gains → save → camera speeds/in-memory publish.
  - Compensation order = gains → runtime. Every required attempt runs. No fail-fast inside compensation.
  - Candidate cfg remains unpublished on failure. `SettingsStore::save` keeps disk ownership.
  - Initial `ModeChangeOutcome::RolledBack(primary)` proves old runtime restored → recoverable.
  - During candidate → old compensation, `ModeChangeOutcome::RolledBack(primary)` proves old runtime **not** reached → fatal.
  - Initial confinement error may be partial. Always reapply old `display.confine_pointer`; success → recoverable, failure → fatal.
  - `LiveCommit.result` holds only success or verified-recoverable primary error. Txn/lifecycle fatality uses outer `Err(String)`.
  - Mode commit always runs `release_claim` → txn/all compensation → `reclaim`. Txn fatal skips `viewport` after successful reclaim.
  - Existing `RtsSession::audio_fatal` stays sink-fatal transport for scripted path. Settings fatal composes with any pre-latched audio error; it never overwrites either context.
  - `GpuContext::release_window` is safe on unclaimed/already-released windows (`crates/mmd-engine/src/render/device.rs:95-103`).

## Requirements

### Seam-first compile-green prep

Do this before any behavioral red. Prep may restructure testability only; txn behavior remains baseline-buggy until Green.

- Edit `src/rts_feedback.rs::FakeAudioSink`:

```rust
fail_set_gains_on_calls: Vec<u32>

pub fn set_fail_set_gains_on_calls(&mut self, calls: &[u32]);
```

- Replace `fail_set_gains: bool`. Setter replaces schedule, not appends. `set_gains` increments `gain_calls` first. Scheduled 1-based call returns `AudioError(format!("injected set_gains failure on call {}", self.gain_calls))` before changing fake `gains`; unscheduled call stores gains. Remove `set_fail_set_gains(bool)` after migrating current caller.
- Add test-only mutable shared-sink access:

```rust
#[cfg(test)]
pub fn sink_mut(&self) -> std::cell::RefMut<'_, FakeAudioSink>;
```

- Edit `src/rts_ui.rs::tests::FakeWindow`:

```rust
failures: Vec<(&'static str, u32)>,
viewport_calls: Cell<u32>,

fn failing_on(mut self, step: &'static str, occurrence: u32) -> Self;
fn call_count(&self, step: &str) -> u32;
```

- `failing_on` appends exact `(step, occurrence)`; duplicate pairs unnecessary. `record` logs first, computes 1-based count for exact `step`, then returns `Err(format!("{step} failed (injected on occurrence {occurrence})"))` only for scheduled pair. Migrate existing failures to occurrence 1.
- `FakeWindow::set_mouse_grab` sets `grabbed = requested` **before** `record("set_mouse_grab")`; this models partial native mutation before `Err`. Tests needing old confined state initialize `grabbed: true`.
- `viewport()` increments `viewport_calls` through `Cell` before checking `("viewport", occurrence)`. `call_count("viewport")` returns `viewport_calls.get()`; other steps count `log` entries.
- Add private production seam in `src/rts_run.rs`:

```rust
fn finish_live_settings_commit(
    live: Result<crate::rts_ui::LiveCommit, String>,
    teardown: impl FnOnce(),
) -> Result<crate::rts_ui::LiveCommit, RunError>;
```

- `finish_live_settings_commit`: `Ok(live)` returns unchanged without calling `teardown`; `Err(e)` calls `teardown()` exactly once before returning `Err(RunError::Failed(e))`.
- Replace live left-`MouseButtonUp` outer-`Err` match with this helper. Production callback must execute `renderer.ctx.release_window(&window)` + existing `rts: released window` print. `?` then returns from `run_windowed`, dropping owned SDL `window`. Success path keeps window. Do not use test-only replica.
- Add `src/rts_run.rs::tests::live_mouse_button_up_fatal_runs_teardown_before_return`. Call `finish_live_settings_commit(Err("settings txn fatal".into()), ...)`; callback appends `"teardown"`, caller appends `"returned"` after result. Assert order `vec!["teardown", "returned"]`; callback count 1; returned variant `RunError::Failed("settings txn fatal")`. This is green seam proof, shared by production `MouseButtonUp` path.

### Structured window/runtime errors

- Add `src/rts_window.rs`:

```rust
#[derive(Debug, PartialEq, Eq)]
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

- Convert each `RunError` from `apply_window_mode` with `.to_string()`. Keep `ModeChangeOutcome::{Applied, RolledBack(String)}` semantics.
- `ModeTransitionError` implements `Display` + `Error`. Exact display:
  `window mode change failed ({primary}); rollback also failed ({rollback}) — window state is indeterminate, restart the app`.
- Replace `src/rts_ui.rs::apply_runtime` error with private:

```rust
#[derive(Debug, PartialEq, Eq)]
enum RuntimeApplyError {
    Primary(String),
    RolledBack(String),
    Fatal {
        primary: String,
        compensation_failures: Vec<String>,
    },
}
```

- `apply_runtime(...) -> Result<(), RuntimeApplyError>` mapping:
  - no window/non-window change → `Ok(())`;
  - mode `Applied` → `Ok(())`;
  - mode `RolledBack(primary)` → `Err(RuntimeApplyError::RolledBack(primary))`;
  - `ModeTransitionError { primary, rollback }` → `Fatal { primary, compensation_failures: vec![format!("window mode rollback failed: {rollback}")] }`;
  - confinement error → `Primary(format!("pointer grab failed: {e}"))`. `apply_runtime` itself does not retry confinement; txn layer owns old-value reapply.

### Settings txn error + exhaustive compensation

- Add `src/rts_ui.rs`:

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
  - recoverable → `{primary}`;
  - fatal → `settings change failed ({primary}); compensation failed ({failures joined by "; "}) — runtime state is indeterminate, restart the app`.
- Validation failure → `Recoverable { primary: "refusing to apply invalid settings: ..." }`; touch no runtime/audio/disk.
- Initial runtime result:
  - `Ok` → continue;
  - `RolledBack(primary)` → recoverable; no extra attempt because mode transition verified old mode;
  - `Fatal { .. }` → settings fatal with same primary + every compensation entry;
  - `Primary(primary)` → run common compensation with audio restore disabled, runtime restore enabled. This is initial confinement partial-failure path.
- Add exact private helper:

```rust
fn compensate_setting_change<W: WindowOps>(
    window: Option<&mut W>,
    old: &RtsSettings,
    candidate: &RtsSettings,
    audio: &mut dyn AudioSink,
    change: SettingsChange,
    primary: String,
    restore_audio: bool,
    restore_runtime: bool,
) -> SettingsCommitError;
```

- Helper attempts enabled stages in fixed order: old effective gains first, candidate → old runtime second. Always attempt runtime even when gain restore fails.
- Audio restore error appends `audio gain rollback failed: {e}`.
- Runtime restore mapping appends without dropping nested context:
  - `Primary(e)` or `RolledBack(e)` → `runtime rollback failed: {e}`;
  - `Fatal { primary, compensation_failures }` → first `runtime rollback failed: {primary}`, then each nested item as `runtime rollback compensation failed: {item}`, preserving source order.
- Empty failure vec → `Recoverable { primary }`; nonempty → `Fatal { primary, compensation_failures }`.
- Candidate gain failure calls helper with `restore_audio=true`, `restore_runtime=true`. This compensates potentially partial `AudioEngine::set_gains` + applied runtime.
- Save failure uses same flags. Candidate cfg publishes only after all stages succeed.
- No discarded compensation result remains.

### Live classification + lifecycle context

- Keep public shapes source-compatible:

```rust
pub struct LiveCommit {
    pub result: Result<(), String>,
    pub viewport: Option<DisplayViewport>,
}

pub fn commit_setting_change_live<W: ClaimedWindow>(...) -> Result<LiveCommit, String>;
```

- Add exact private classifier:

```rust
fn soft_live_result(
    result: Result<(), SettingsCommitError>,
) -> Result<Result<(), String>, String>;
```

- Mapping: success → outer `Ok(inner Ok)`; recoverable → outer `Ok(inner Err(primary))`; fatal → outer `Err(error.to_string())`. Never flatten fatal into `LiveCommit.result`.
- Non-mode branch classifies through `soft_live_result`; fatal returns outer error immediately. No claim/viewport work.
- Mode branch exact order:
  1. `release_claim()` once;
  2. retain typed txn result;
  3. call `reclaim()` once regardless of txn result;
  4. txn error + reclaim error → outer `Err(format!("{txn}; window reclaim after a mode change also failed: {reclaim}"))`; applies to recoverable or fatal txn, preserving primary context;
  5. txn success + reclaim error → existing `window reclaim after a mode change failed: {reclaim}`;
  6. reclaim success + txn fatal → outer txn error; `viewport()` count stays 0;
  7. reclaim success + txn success/recoverable → call `viewport()` once, return `LiveCommit`;
  8. recoverable txn + viewport error → retain both as `{primary}; viewport refresh after a rolled-back mode change also failed: {viewport}`;
  9. successful txn + viewport error → existing `viewport refresh after a mode change failed: {viewport}`.
- Every fatal path reaches `src/rts_run.rs::finish_live_settings_commit`; production callback releases GPU claim before `RunError::Failed` returns from `MouseButtonUp`. No comment-only proof.

### Scripted fatal composition

- `src/rts_run.rs::commit_scripted_setting_change` matches typed variants directly:
  - success → clear warning;
  - recoverable → `SETTINGS NOT SAVED: {primary}`;
  - fatal → clear warning, stringify full settings error, take existing `session.audio_fatal`, then store:
    - no prior → `AudioError(format!("settings transaction fatal: {settings_error}"))`;
    - prior → `AudioError(format!("{prior}; settings transaction fatal: {settings_error}"))`.
- Do **not** use `get_or_insert` for settings fatal. Keep existing first-audio-error semantics in `emit_audio`, `maintain_audio`, `publish_gains`; only scripted settings fatal composes because both fatal contexts must survive.
- Existing post-frame `take_audio_fatal` path releases live window + exits. Offscreen fake can reach injected fatal only in unit tests.

## Inputs

- Spec: `/home/aron/projects/millions_must_die/.tmp/MAKE_AUDIT_2026_08_14_millions-must-die_e7fe9dee0277/F4_settings-rollback-fatality.md`.
- Issue form: same dir `F4_settings-rollback-fatality_ISSUE.md`.
- Audit proof: same dir `validate-c02-settings-rollback.md`.
- Review blockers: same dir `F4-plan-review-{scope,exec,security}.md`.
- Txn/live code: `src/rts_ui.rs::{commit_setting_change, commit_setting_change_live, apply_runtime, LiveCommit}`.
- Window transition: `src/rts_window.rs::{transition_window_mode, ModeChangeOutcome, WindowOps, ClaimedWindow}`.
- Partial audio behavior: `src/rts_audio.rs::AudioEngine<S>::set_gains` mutates stored/stream gains before possible `Err`.
- Audio seam: `src/rts_feedback.rs::{FakeAudioSink, FakeSinkHandle, AudioError}`.
- Live/scripted callers: `src/rts_run.rs` left `MouseButtonUp`, `commit_scripted_setting_change`, `RtsSession::audio_fatal`.
- Teardown safety: `crates/mmd-engine/src/render/device.rs::GpuContext::release_window`.
- Manual proof: `artifacts/manual_test_checklist.md`, `## T13 settings-menu`, current save-failure bullet.
- **From Depends:** none. Baseline `e7fe9dee0277d84da78f6436832aa05e3f95f71d`; 24 `rts_ui::tests`; 95 bin unit tests.

## TDD

### P0 — compile-green seam prep

- [ ] Implement only seam-first prep above + `live_mouse_button_up_fatal_runs_teardown_before_return`. Migrate current bool/call-1 fake users. Run `cargo fmt --all && cargo test --locked --bin millions_must_die rts_run::tests::live_mouse_button_up_fatal_runs_teardown_before_return -- --exact --nocapture && cargo test --locked --bin millions_must_die -- --nocapture`. **Expect:** exit 0; seam test `1 passed`; bin `96 passed`; existing txn semantics unchanged.

### Behavioral reds — add all tests against P0 + existing prod APIs

No red below names `SettingsCommitError`, `ModeTransitionError`, `RuntimeApplyError`, or any later Green-only helper. Each calls existing `commit_setting_change`, `commit_setting_change_live`, or `commit_scripted_setting_change` through P0 seams.

- [ ] **R1 initial mode internal rollback fatal.** Add `rts_ui::tests::a_mode_change_with_failed_internal_rollback_is_fatal_after_reclaim`. Run `cargo test --locked --bin millions_must_die rts_ui::tests::a_mode_change_with_failed_internal_rollback_is_fatal_after_reclaim -- --exact --nocapture`. **Expect red:** exit 101; `0 passed; 1 failed`; panic text `failed internal rollback must be outer fatal` because baseline returns outer `Ok`; scheduled counts at failure point: `sync=2`, `reclaim=1`, baseline `viewport=1`.
- [ ] **R2 reverse mode compensation cannot accept `RolledBack`.** Add `rts_ui::tests::a_candidate_to_old_mode_rolled_back_to_candidate_is_fatal`. Run `cargo test --locked --bin millions_must_die rts_ui::tests::a_candidate_to_old_mode_rolled_back_to_candidate_is_fatal -- --exact --nocapture`. **Expect red:** exit 101; `0 passed; 1 failed`; panic `candidate-to-old RolledBack must be outer fatal`; baseline returns outer `Ok`; `sync=3`, `reclaim=1`, baseline `viewport=1`.
- [ ] **R3 initial txn fatal + reclaim fatal context.** Add `rts_ui::tests::a_mode_transaction_fatal_and_reclaim_fatal_preserve_both_contexts`. Run `cargo test --locked --bin millions_must_die rts_ui::tests::a_mode_transaction_fatal_and_reclaim_fatal_preserve_both_contexts -- --exact --nocapture`. **Expect red:** exit 101; `0 passed; 1 failed`; assertion `missing primary mode failure` because baseline error contains reclaim only; `sync=2`, `reclaim=1`, `viewport=0`.
- [ ] **R4 compensation fatal + reclaim fatal context.** Add `rts_ui::tests::a_mode_compensation_fatal_and_reclaim_fatal_preserve_all_contexts`. Run `cargo test --locked --bin millions_must_die rts_ui::tests::a_mode_compensation_fatal_and_reclaim_fatal_preserve_all_contexts -- --exact --nocapture`. **Expect red:** exit 101; `0 passed; 1 failed`; assertion `missing save primary` because baseline error contains reclaim only; `sync=3`, `reclaim=1`, `viewport=0`.
- [ ] **R5 initial confinement partial failure, old reapply succeeds.** Add `rts_ui::tests::an_initial_confinement_failure_reapplies_old_and_stays_recoverable`. Run `cargo test --locked --bin millions_must_die rts_ui::tests::an_initial_confinement_failure_reapplies_old_and_stays_recoverable -- --exact --nocapture`. **Expect red:** exit 101; `0 passed; 1 failed`; `assert_eq!(window.call_count("set_mouse_grab"), 2)` fails `left: 1, right: 2`; baseline skips old reapply.
- [ ] **R6 initial confinement partial failure + old reapply fails.** Add `rts_ui::tests::an_initial_confinement_failure_with_failed_reapply_is_fatal`. Run `cargo test --locked --bin millions_must_die rts_ui::tests::an_initial_confinement_failure_with_failed_reapply_is_fatal -- --exact --nocapture`. **Expect red:** exit 101; `0 passed; 1 failed`; panic `failed confinement reapply must be outer fatal`; baseline returns outer `Ok`; baseline call count 1, required 2.
- [ ] **R7 gain primary + verified compensation recoverable.** Replace `a_failed_gain_push_rolls_back_like_a_failed_save` with `rts_ui::tests::an_audio_failure_with_successful_compensation_stays_recoverable`. Run `cargo test --locked --bin millions_must_die rts_ui::tests::an_audio_failure_with_successful_compensation_stays_recoverable -- --exact --nocapture`. **Expect red:** exit 101; `0 passed; 1 failed`; `assert_eq!(audio.gain_calls(), 2)` fails `left: 1, right: 2`; baseline never restores possibly-partial gains.
- [ ] **R8 gain primary + gain compensation fatal.** Add `rts_ui::tests::an_audio_failure_with_failed_audio_compensation_is_fatal`. Run `cargo test --locked --bin millions_must_die rts_ui::tests::an_audio_failure_with_failed_audio_compensation_is_fatal -- --exact --nocapture`. **Expect red:** exit 101; `0 passed; 1 failed`; panic `failed gain compensation must be outer fatal`; baseline returns outer `Ok`; baseline call count 1, required 2.
- [ ] **R9 save primary + all compensations.** Add `rts_ui::tests::a_save_failure_attempts_every_compensation_and_any_failure_is_fatal`. Run `cargo test --locked --bin millions_must_die rts_ui::tests::a_save_failure_attempts_every_compensation_and_any_failure_is_fatal -- --exact --nocapture`. **Expect red:** exit 101; `0 passed; 1 failed`; panic `failed save compensation must be outer fatal`; baseline returns outer `Ok` despite `gain_calls=2`, `set_mouse_grab=2`.
- [ ] **R10 scripted pre-latched composition.** Add `rts_run::tests::scripted_fatal_setting_commit_composes_pre_latched_audio_error`. Run `cargo test --locked --bin millions_must_die rts_run::tests::scripted_fatal_setting_commit_composes_pre_latched_audio_error -- --exact --nocapture`. **Expect red:** exit 101; `0 passed; 1 failed`; `assert!(session.ui.warning.is_none())` fails because baseline warns; baseline gain count 1 vs required 2; subsequent fatal retains only pre-latched value, losing settings primary/compensation.

### Green

- [ ] **G1 errors/runtime.** Implement `ModeTransitionError`, `RuntimeApplyError`, `SettingsCommitError`; migrate existing transition tests. **Expect:** project compiles; lower fatal transition test asserts exact `primary`, `rollback`, `indeterminate`, `restart the app`.
- [ ] **G2 txn compensation.** Implement common helper + no-discard rules. **Expect:** R5-R9 pass; call counts exact; cfg never publishes on failure.
- [ ] **G3 live lifecycle.** Implement classifier + reclaim/context order. **Expect:** R1-R4 pass; every mode fatal reclaims once; viewport stays zero after txn fatal.
- [ ] **G4 callers.** Compose scripted pre-latched context; route `MouseButtonUp` through prepared teardown seam. **Expect:** R10 + prep teardown test pass; fatal never becomes warning.
- [ ] **Refactor.** Update doc comments only around touched txn/caller/fake symbols. Remove old bool fault API + discarded-result forms. No adjacent cleanup.

## Test plan

R1-R9 call production `src/rts_ui.rs::commit_setting_change_live`; no direct `apply_runtime`/compensation-helper calls. R10 calls production `src/rts_run.rs::commit_scripted_setting_change`. Teardown proof calls same private `finish_live_settings_commit` used only by production left-`MouseButtonUp` branch.

| Test | Exact setup/fault schedule | Exact green expect |
| --- | --- | --- |
| `live_mouse_button_up_fatal_runs_teardown_before_return` | `finish_live_settings_commit(Err("settings txn fatal"), callback)` | callback once before return; order `teardown, returned`; `RunError::Failed` retains exact text |
| `a_refused_mode_change_still_reclaims` (strengthen) | default old borderless; request windowed; `set_size#1` fails; internal old reapply succeeds | outer `Ok`; inner exact primary `Err`; cfg old; release 1; reclaim 1; viewport 1; gain 0 |
| `a_mode_change_with_failed_internal_rollback_is_fatal_after_reclaim` | default old borderless; request windowed; `sync#1,#2` fail | outer error contains windowed primary + `window mode rollback failed:` borderless failure + indeterminate/restart; release 1; reclaim 1; viewport 0; gain 0; cfg old |
| `a_candidate_to_old_mode_rolled_back_to_candidate_is_fatal` | nonempty-dir store fails save; borderless → windowed applies (`sync#1`); old borderless compensation fails `sync#2`; internal candidate restore succeeds `sync#3` | outer error contains `save failed:`, `runtime rollback failed:` + borderless `sync#2`, indeterminate/restart; release 1; reclaim 1; viewport 0; gain 2; cfg old |
| `a_mode_transaction_fatal_and_reclaim_fatal_preserve_both_contexts` | request windowed; `sync#1,#2`, `reclaim#1` fail | outer error contains requested-mode primary + old-mode rollback + indeterminate/restart + exact `window reclaim after a mode change also failed:`; release 1; reclaim 1; viewport 0; gain 0; cfg old |
| `a_mode_compensation_fatal_and_reclaim_fatal_preserve_all_contexts` | failing store; forward windowed succeeds; `sync#2`, `reclaim#1` fail; internal candidate restore `sync#3` succeeds | outer error contains save primary + runtime rollback `sync#2` + indeterminate/restart + reclaim fragment; release 1; reclaim 1; viewport 0; gain 2; cfg old |
| `an_initial_confinement_failure_reapplies_old_and_stays_recoverable` | old cfg/grab true; `Confine(false)`; `set_mouse_grab#1` partial-fails, call 2 succeeds | outer `Ok`; inner exact call-1 primary; grab calls 2; `grabbed=true`; cfg old; gain 0; no claim/disk |
| `an_initial_confinement_failure_with_failed_reapply_is_fatal` | old cfg/grab true; `Confine(false)`; `set_mouse_grab#1,#2` partial-fail | outer error contains call-1 primary + `runtime rollback failed:` call-2 + indeterminate/restart; grab calls 2; cfg old; gain 0; no claim/disk |
| `an_audio_failure_with_successful_compensation_stays_recoverable` | old cfg/grab true; valid store; `Confine(false)`; gain call 1 fails only | outer `Ok`; inner exact `audio gain failed: injected set_gains failure on call 1`; gain calls 2; gains old; grab calls 2; `grabbed=true`; cfg old; no claim/disk write |
| `an_audio_failure_with_failed_audio_compensation_is_fatal` | `Master(50)`; gain calls 1,2 fail | outer error contains call-1 primary + `audio gain rollback failed:` call 2 + indeterminate/restart; gain calls 2; cfg old; zero window ops |
| `a_save_failure_attempts_every_compensation_and_any_failure_is_fatal` | nonempty-dir store; old cfg/grab true; `Confine(false)`; gain call 2 + `set_mouse_grab#2` fail | outer error contains save primary, audio rollback, runtime rollback in that order, indeterminate/restart; gain calls 2; grab calls 2 despite audio failure; cfg old; no claim |
| `a_failed_reclaim_is_fatal_not_a_warning` (retain) | successful mode txn; `reclaim#1` fails | outer reclaim fatal; reclaim once; viewport 0; never inner warning |
| `scripted_fatal_setting_commit_composes_pre_latched_audio_error` | shared fake schedule gain 1,2; session pending `Master(50)`; pre-latch `AudioError("pre-latched UI cue failure")` | warning `None`; gain calls 2; cfg old; first `take_audio_fatal()` contains pre-latched text + settings primary + audio compensation + indeterminate/restart in order; second take `None` |

## Impl steps

- [ ] **I1** Apply P0 fake schedules in `src/rts_feedback.rs`, test fake schedules in `src/rts_ui.rs`, production teardown helper/test in `src/rts_run.rs`. **Criterion:** exact P0 cmd passes; 96 bin tests; no txn classification change.
- [ ] **I2** Add R1-R10 exactly; record each literal red cmd result before Green. **Criterion:** each compiles against P0/current prod API, runs one test, fails specified assertion; no compile-error red.
- [ ] **I3** Edit `src/rts_window.rs`: add `ModeTransitionError`; migrate `transition_window_mode`; strengthen existing `failed_mode_change_with_failed_rollback_is_actionable_fatal`. **Criterion:** public fields retain both exact `RunError` strings; recovered rollback unchanged.
- [ ] **I4** Edit `src/rts_ui.rs`: add typed runtime/settings errors + Display/Error impls; migrate call sites/tests. **Criterion:** only verified rollback maps recoverable.
- [ ] **I5** Add `compensate_setting_change`; route initial confinement, gain, save failures through it. **Criterion:** audio → runtime order; every attempt/context retained; no early return inside helper.
- [ ] **I6** Rewrite `commit_setting_change_live` typed classification/reclaim ordering. **Criterion:** R1-R4 pass; both txn+reclaim intersections retain all text; one reclaim; zero viewport.
- [ ] **I7** Edit `src/rts_run.rs::commit_scripted_setting_change`: compose pre-latched + settings fatal; keep audio methods' first-error rule. **Criterion:** R10 passes; no scripted fatal warning.
- [ ] **I8** Keep left `MouseButtonUp` wired through `finish_live_settings_commit`. **Criterion:** production callback calls `GpuContext::release_window`; helper test proves callback-before-return; run scope drops owned window on `?`.
- [ ] **I9** Edit only existing save-failure bullet in `artifacts/manual_test_checklist.md` `## T13 settings-menu`: successful rollback → warning, reverted control/runtime, continued session. Add one adjacent bullet: failed native display/audio compensation intentionally automated-only (unsafe/non-portable injection); expected live result = GPU release/window teardown + exit 1 with primary + every rollback/lifecycle context. State no manual fatal injection was run.
- [ ] **I10** Run fmt/validation; inspect focused diff. **Criterion:** app files limited to `src/rts_ui.rs`, `src/rts_window.rs`, `src/rts_feedback.rs`, `src/rts_run.rs`; doc limited to checklist; no deps/config/schema changes.

## Outputs

- Modified impl: `src/rts_ui.rs`, `src/rts_window.rs`, `src/rts_feedback.rs`, `src/rts_run.rs`.
- Modified manual doc: `artifacts/manual_test_checklist.md`.
- Public API: `SettingsCommitError`, `ModeTransitionError`; typed `commit_setting_change` result. `LiveCommit` + `commit_setting_change_live` signatures unchanged.
- Tests: 8 net new `rts_ui::tests` (one existing gain test replaced), 2 new `rts_run::tests`; focused count 24 → 32; bin count 95 → 105.
- Behavior: verified primary rollback → warning/continue. Any failed compensation → fatal. Mode fatal reclaims first. Combined fatal retains txn primary + every compensation + reclaim. Live `MouseButtonUp` releases claim/tears down before return. Scripted fatal composes pre-latched audio + settings context.
- Config/deps/migrations: none.

## Validation

- [ ] `cargo fmt --all -- --check` **Criterion:** exit 0; no fmt diff.
- [ ] `cargo test --locked --bin millions_must_die rts_ui::tests -- --nocapture` **Criterion:** exit 0; exactly 32 passed, 0 failed.
- [ ] `cargo test --locked --bin millions_must_die rts_run::tests::live_mouse_button_up_fatal_runs_teardown_before_return -- --exact --nocapture` **Criterion:** exit 0; exactly 1 passed; callback count 1 + before-return order asserted.
- [ ] `cargo test --locked --bin millions_must_die rts_run::tests::scripted_fatal_setting_commit_composes_pre_latched_audio_error -- --exact --nocapture` **Criterion:** exit 0; exactly 1 passed; pre-latched + settings primary/compensation retained.
- [ ] `cargo test --locked --bin millions_must_die -- --nocapture` **Criterion:** exit 0; exactly 105 passed, 0 failed.
- [ ] `cargo test --workspace --locked` **Criterion:** exit 0; all workspace tests pass; no ignored-test failure.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` **Criterion:** exit 0; no warning.
- [ ] `SDL_VIDEODRIVER=offscreen cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` **Criterion:** exit 0; suffix remains `body_overlaps=0 ui_page=gameplay music_starts=1 voice_select=8 voice_order=9 voice_reject=1 sfx_ui=8 keyboard_pan=78`; no native-failure claim from offscreen.
- [ ] `rg -n 'let _ = (audio\.set_gains|apply_runtime)|set_fail_set_gains\(' src/rts_ui.rs src/rts_feedback.rs` **Criterion:** exit 1, no matches; no discarded compensation/old bool injector.
- [ ] `rg -n 'get_or_insert\(AudioError\(error\.to_string\(\)\)\)' src/rts_run.rs` **Criterion:** exit 1, no match; scripted settings fatal does not discard pre-latched context.
- [ ] `git diff --check` **Criterion:** exit 0.
- [ ] Manual checklist review **Criterion:** T13 separates recoverable continuation from automated-only fatal path; says no unsafe native fault injection executed.
- [ ] App functional **Criterion:** normal settings commit updates runtime/gains/store/camera/cfg once; existing success tests + 1,600-frame smoke pass.
- [ ] Commit msg draft: `fix(rts): make failed settings compensation fatal` **Criterion:** one DCO-signed impl commit; no unrelated files.

## Residual risk

- `src/rts_settings.rs::SettingsStore::save` still discards backup-restoration failures at current lines 362/376. Persistence protocol remains explicit Scope Out.
- Automated fakes prove classification/teardown control flow, not compositor/audio-driver physical behavior. Manual checklist states boundary; native double-failure injection remains unsafe/non-portable.
