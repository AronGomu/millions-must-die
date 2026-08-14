# T1: Schedule RTS hashes only at observable endpoints

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_6_rts_per_frame_hash.md`

**Depends:** none

**Commit outcome:** RTS loop computes only initial, successful-frame-1, final-required hashes; stdout/quit/error semantics remain exact.

## Context (self-contained)

- Goal: remove `world.state_hash()` from every successfully rendered RTS frame. `RtsWorld::state_hash` traverses full state plus allocates live-slot `Vec`; intermediate hashes have no consumer.
- Current flow: `src/rts_run.rs::run` hashes loaded world once. `step_frame` hashes after every successful draw; frame 1 feeds `rts: frame0`; latest value feeds `finish` clean-exit line.
- This slice: replace rolling per-frame digest with endpoint scheduler. Add deterministic in-process call/allocation proof plus exact real-binary stdout checks.
- Out of scope: `crates/mmd-engine/src/rts/world.rs::RtsWorld::state_hash`; horde `src/run.rs`; input fixes; horde F8; perf/time threshold; broad loop extraction; public API; CLI flags/env vars; gameplay.
- Assumptions in force:
  - Frame means successfully rendered frame. Failed draw never increments `RunState::frames`.
  - `frame0` hash = world immediately after first successful render.
  - Clean-exit hash = last successfully rendered world; if no render succeeded, loaded initial world.
  - Script `quit` stops `RtsScript::drain_frame` before `step_frame` applies any command from that frame. Current earlier/later same-frame script entries remain unapplied/unfired exactly as today.
  - Current `KEY_BINDINGS` has no `RtsCommand::Quit`; Escape opens menu. Do not add key binding. Retain key-derived semantic classification so live loop cannot drift if binding changes later.
  - `Event::AppTerminating`, window close-like events other than `Event::Quit`, UI Escape, focus loss, frame budget are not explicit live quit events today. Preserve classification.
  - Render errors, interactive audio errors, tick mismatch, unfired script entries still return `Err` with no `rts: clean exit`.

## Requirements

### Endpoint truth table

| Stop path | Last successful render | Hash source | `RtsWorld::state_hash` calls |
| --- | ---: | --- | ---: |
| Script `1:quit` | none | cached initial | 1 |
| `--frames 1` | frame 1 | cached frame-1 endpoint | 2 |
| `--frames N`, `N > 1` | frame N | hash current world once in clean `finish` | 3 |
| Script `K:quit`, `K > 2` | frame `K-1` | hash unchanged current world once in clean `finish` | 3 |
| Live `Event::Quit`/key-derived `Quit` after frame 1 | prior rendered frame | cached frame-1 endpoint | 2 |
| Live `Event::Quit`/key-derived `Quit` after later frame | prior rendered frame | pre-event-batch snapshot | 3 |
| First/later render error | prior/none | no clean endpoint | no final hash; return error |
| Interactive audio fatal | prior/none | no clean endpoint | no final hash; return error |

- Initial hash remains mandatory: frame-1 scripted quit must print it without rendering.
- Successful frame-1 hash remains mandatory: `frame0` prints it. One-frame final reuses it.
- Later successful frames perform zero full hashes, zero hash-induced heap allocs.
- Normal/scripted final world is still current world → defer its one required hash to `finish`.
- Live events mutate `world` before next render. When collected batch contains explicit terminal input, capture last-rendered hash before processing any batch event. Reuse frame-1 hash when only one frame rendered; hash once when more rendered.
- Never snapshot ordinary live event batch. Explicit terminal classifier only:
  1. `Event::Quit { .. }`;
  2. non-repeat `Event::KeyDown` whose `rts_input::command_from_keycode` returns `Some(RtsCommand::Quit)`.
- When terminal follows earlier audio-emitting event, fatal audio still wins. Replace direct outer-loop quits with inner event-loop break, run existing `session.take_audio_fatal()` check, then break `'running` when `session.quit`; later events remain ignored.
- Hash bytes/hex formatting/order unchanged. No new stdout/stderr field.
- `finish` resolves final hash only after unfired-entry + tick checks pass. Failed checks compute no unobservable final hash.
- Counter is deterministic, per-scheduler, test-only observation. No process-global atomic; parallel tests cannot cross-talk.

### Quit-source completeness proof

Current repository search yields exactly these writes/paths:

1. `src/rts_run.rs::apply`: only `RtsCommand::Quit` writes `session.quit = true`.
2. `src/rts_script.rs::drain_frame`: only bare script `quit` creates/recognizes `RtsCommand::Quit`; returns before `step_frame` applies buffered commands.
3. Live loop: direct `Event::Quit`; key path calls `rts_input::command_from_keycode`, then `apply`, then checks `session.quit`.
4. `src/rts_input.rs::KEY_BINDINGS`: no current `Quit`; Escape = `Escape` menu command.
5. Mouse/UI/focus/window handlers never write `session.quit`; frame budget exits without quit.
6. `step_frame` `Err`, `present_error`, startup/render/audio returns are fatal, not clean quit.

Implementation must rerun:

```sh
rg -n "RtsCommand::Quit|session\.quit|state\.quit|Event::Quit|break 'running" src/rts_*.rs
```

Expected: no new quit source; any new writer absent from classifier blocks completion.

### Baseline hashes at `e7fe9de`

Tracked scene/default settings, Linux Vulkan baseline captured before implementation:

| State | Exact hash |
| --- | --- |
| Loaded/0 renders | `a5862bf12c393ebfa6a63b0e3c90d3e149556520385ec4c9ff4f3f47336d3930` |
| Frame 1 | `34f5314bc221f94fc0f25ebff74df18255fb0843e0faf1ab92174637c48894fc` |
| Frame 2 | `8c2ea9b57b7655afcb04f1fea2928cc37b7018f3822be23fbfc1ec1e25a5bf7d` |
| Frame 3 | `3a07a50808798e18f8882379f121d77371fb70add5447dd8a59dae72dcf4b7b2` |
| Frame 5 | `1dff55254b069ff74514490e7301d238251d114f3f7496db5a3d69a357cea1ca` |
| Frame 50 | `6be744e0375c56a277e984fec6b1d4f9e0a9d5ba294c2bf8f8832ace2acf28b2` |
| Acceptance script, 1,559 renders/1,519 ticks | `684d69aafa1195e8a02c3f18ea0f8d8af5b56fdd8aac52023e0c30fce3381a46` |

These are same-host exact contracts, not cross-platform claims. Cross-process test still compares two child processes; literal assertions catch semantic drift inside one implementation.

## Inputs

- `src/rts_run.rs`
  - `RunState`
  - `run`
  - live `'running` event loop
  - `step_frame`
  - `finish`
  - `tests` module
- `src/rts_script.rs::RtsScript::drain_frame`
- `src/rts_input.rs::{RtsCommand, command_from_keycode, KEY_BINDINGS}`
- `crates/mmd-engine/src/rts/world.rs::RtsWorld::state_hash` — read only; do not edit.
- `mmd_engine::alloc_guard::MeasureGuard` — root binary already installs `CountingAllocator` in `src/main.rs`; app unit allocation checks are non-vacuous.
- `tests/rts_cli_contract.rs`
  - `Cli::{frame0_line, final_hash, assert_success}`
  - `rts`, `or_skip`, `the_run_is_deterministic`, `a_quit_on_frame_one_renders_nothing`, `the_window_is_released_before_it_drops`
- **From Depends:** none.

## TDD

1. **Red — endpoint calls/allocs in `src/rts_run.rs::tests`.**
   - Add `endpoint_hash_calls_cover_clean_exit_shapes`.
   - Build fresh tracked `RtsWorld` per case via `RtsWorld::load(&mmd_engine::workspace_root().join("assets/scenarios/rts_prototype_v1.ron"))`.
   - Simulate render by `world.tick()` then scheduler `record_rendered`.
   - Assert exact counts: frame-1 quit `1`; one-frame budget `2`; five-frame budget `3`; scripted quit before frame 3 after two renders `3`.
   - Assert returned final hashes equal direct hashes of initial/frame 1/frame 5/frame 2 respectively.
   - Add `intermediate_rendered_frames_hash_nothing_and_allocate_nothing`: initialize + record frame 1 outside measure; arm `MeasureGuard`; tick/record frames `2..=100`; assert `allocations() == 0`, scheduler call count stays `2`; final resolve after guard increments to `3`.
2. **Red — terminal batch semantics in `src/rts_run.rs::tests`.**
   - Add `live_quit_keeps_pre_batch_rendered_hash`.
   - Advance/record two rendered frames. Create ordered batch `[MouseButtonUp(left at [960.0, 518.0]), Event::Quit]`; call production pre-batch helper before applying equivalent `RtsCommand::LeftClick([960.0, 518.0])`.
   - Assert click changes selection/world hash. Assert final scheduler hash equals pre-click rendered hash, differs from mutated current hash, call count = `3`.
   - Add `event_and_key_quit_sources_are_classified`: direct SDL quit = true; semantic non-repeat `Some(RtsCommand::Quit)` key command = true; repeat Quit = false; Escape/`None` = false; actual Escape key event = false.
3. **Red — stdout endpoints in `tests/rts_cli_contract.rs`.**
   - Add constants `INITIAL_HASH`, `FRAME_1_HASH`, `FRAME_2_HASH`, `FRAME_3_HASH`, `FRAME_5_HASH`, `FRAME_50_HASH` with literals above.
   - Extend `a_quit_on_frame_one_renders_nothing`: final hash = `INITIAL_HASH`; still no `rts: frame0`.
   - Add `one_and_multi_frame_budgets_keep_exact_hashes`: run budgets 1, 2, 5; each exit 0; every `frame0` = `FRAME_1_HASH`; finals = frame 1/2/5 constants; frames/ticks = budget.
   - Add `scripted_quit_hashes_the_last_rendered_world`: compare `--frames 9 --inject-input 3:quit` with `--frames 2`; both final = `FRAME_2_HASH`; quit run has `frames=2`, `tick=2`, `quit=true`.
   - Extend `the_run_is_deterministic`: both 50-frame child hashes equal each other plus `FRAME_50_HASH`.
   - Extend window-path `the_window_is_released_before_it_drops`: script quits before frame 4; final = `FRAME_3_HASH` in both real-window/offscreen-fallback modes.
4. **Green — scheduler in `src/rts_run.rs`.**
   - Add private `EndpointHashes` directly above `RunState` with fields:
     - `initial: [u8; 32]`
     - `first_rendered: Option<[u8; 32]>`
     - `live_quit: Option<[u8; 32]>`
     - `#[cfg(test)] hash_calls: u64`
   - Add exact private methods:
     - `EndpointHashes::new(&RtsWorld) -> Self`
     - `EndpointHashes::compute(&mut self, &RtsWorld) -> [u8; 32]` — sole `world.state_hash()` call in production module; increments test counter first.
     - `EndpointHashes::record_rendered(&mut self, frame: u64, &RtsWorld)` — computes only `frame == 1`.
     - `EndpointHashes::capture_live_quit(&mut self, rendered_frames: u64, &RtsWorld)` — cache initial for 0, first for 1, compute for >1; never overwrite existing snapshot.
     - `EndpointHashes::final_hash(&mut self, rendered_frames: u64, &RtsWorld) -> [u8; 32]` — live snapshot first; else initial/first/current by 0/1/>1.
     - `#[cfg(test)] EndpointHashes::call_count(&self) -> u64`.
   - Replace `RunState::{first_hash,last_hash}` with `hashes: EndpointHashes`. Construct through `EndpointHashes::new(&world)`.
   - In `step_frame`, after successful draw/audio maintenance, call `state.hashes.record_rendered(frame, world)`; delete unconditional per-frame hash.
   - Print frame0 from `state.hashes.first_rendered.expect("frame 1 rendered")`.
   - Change `finish` state arg to `&mut RunState`. After existing unfired/tick checks, resolve once with `state.hashes.final_hash(state.frames, world)`; print returned digest. Update all call sites.
5. **Green — explicit live terminal handling in `src/rts_run.rs`.**
   - Add `key_command_requests_quit(command: Option<RtsCommand>, repeat: bool) -> bool`.
   - Add `live_event_requests_quit(event: &Event) -> bool`; match only direct `Event::Quit` plus keydown mapped by `command_from_keycode` through helper.
   - Add `preserve_last_rendered_for_quit_batch(events: &[Event], state: &mut RunState, world: &RtsWorld)`. If `events.iter().any(live_event_requests_quit)`, call `capture_live_quit`; otherwise no-op.
   - Call helper immediately after `pump.poll_iter().collect()`, before `for event in events`.
   - In direct/key quit arms, set existing flags then `break` inner event loop, not `break 'running`.
   - Run existing post-event `session.take_audio_fatal()` check. Then `if session.quit { break 'running; }` before `step_frame`. This preserves ignored post-quit events plus fatal-audio precedence.
6. **Green — contract comment.**
   - In `src/rts_run.rs` module stdout docs, state: absent `frame0` on pre-frame-1 quit; frame0 hash is frame 1 after successful draw; clean hash is last successful draw or loaded initial state when none. Do not change line format.
7. **Refactor.**
   - `rg -n "state_hash\(\)" src/rts_run.rs` must show one production occurrence inside `EndpointHashes::compute`; direct test-oracle calls may also appear under `#[cfg(test)]`.
   - Keep scheduler private in `src/rts_run.rs`; no trait/generic/runtime extraction; no `RtsWorld::state_hash` changes.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `endpoint_hash_calls_cover_clean_exit_shapes` | 0/1/2/5 rendered-state simulations | calls `1/2/3/3`; final world exact |
| `intermediate_rendered_frames_hash_nothing_and_allocate_nothing` | frames 2–100 after frame-1 endpoint | `0` measured allocs; calls stay `2`; final calls `3` |
| `live_quit_keeps_pre_batch_rendered_hash` | click mutation before same-batch `Event::Quit` | mutated current hash differs; final = pre-batch rendered hash; 3 calls |
| `event_and_key_quit_sources_are_classified` | direct quit; semantic key Quit; repeat/Escape/none | only direct + non-repeat Quit true |
| `a_quit_on_frame_one_renders_nothing` | `1:quit` | exit 0; frames/tick 0; no frame0; initial literal |
| `one_and_multi_frame_budgets_keep_exact_hashes` | budgets 1/2/5 | frame0 literal; exact final literals; exit 0 |
| `scripted_quit_hashes_the_last_rendered_world` | `3:quit` vs budget 2 | both frame-2 literal; quit reports true/2/2 |
| `the_run_is_deterministic` | two 50-frame processes | hashes equal each other + frame-50 literal |
| `the_window_is_released_before_it_drops` | live-driver attempt, script `4:quit` | frame-3 literal in window/fallback; release semantics unchanged |
| Existing fatal tests/full suite | render/audio failures | exit 1/3 as existing contract; no clean exit |

## Impl steps

- [ ] 1. Read `src/rts_run.rs`, `src/rts_script.rs`, `src/rts_input.rs`, `tests/rts_cli_contract.rs`; rerun quit-source `rg`; stop if new source invalidates classification.
- [ ] 2. Add four red app-unit tests with exact names/assertions above; run focused unit cmd; verify 4 tests execute, fail for missing scheduler/helper—not compile-unrelated cause.
- [ ] 3. Add/extend red CLI endpoint tests; run each exact filter with `MMD_REQUIRE_GPU=1`; verify each executes one test, never zero/skip.
- [ ] 4. Implement private `EndpointHashes`; replace rolling hashes; resolve clean final after validation.
- [ ] 5. Add explicit event/key classifier + pre-batch snapshot; route live quit through post-event audio-fatal check.
- [ ] 6. Update inline stdout contract only; no external docs/checklist.
- [ ] 7. Run focused unit tests. Confirm exact 4-test count; zero intermediate allocs; call counts `1/2/3`.
- [ ] 8. Run focused CLI tests serially. Never overlap GPU subprocess tests: `GPU_LOCK` is process-local.
- [ ] 9. Run exact determinism command + full merge checks below. No wall-time/perf number assertion.
- [ ] 10. Inspect diff: only `src/rts_run.rs`, `tests/rts_cli_contract.rs` unless external docs truly required; no `RtsWorld::state_hash`/horde/input binding change.

## Outputs

- `src/rts_run.rs`
  - private `EndpointHashes`
  - private `key_command_requests_quit`
  - private `live_event_requests_quit`
  - private `preserve_last_rendered_for_quit_batch`
  - updated `RunState`, `run`, `step_frame`, `finish`, unit tests, endpoint comment
- `tests/rts_cli_contract.rs`
  - six baseline constants
  - new/updated endpoint + determinism assertions
- Public API: none.
- CLI/stdout: exact existing lines/fields/hashes; no new output.
- Config/migration/assets/docs/checklist: none.

## Validation

- [ ] Focused app-unit proof: `cargo test --locked --bin millions_must_die rts_run::tests::endpoint_hash_calls_cover_clean_exit_shapes -- --exact --nocapture` → exit 0; `running 1 test`; 1 passed; no ignored.
- [ ] Allocation proof: `cargo test --locked --bin millions_must_die rts_run::tests::intermediate_rendered_frames_hash_nothing_and_allocate_nothing -- --exact --nocapture` → exit 0; `running 1 test`; 0 intermediate allocs.
- [ ] Live batch proof: `cargo test --locked --bin millions_must_die rts_run::tests::live_quit_keeps_pre_batch_rendered_hash -- --exact --nocapture` → exit 0; `running 1 test`; final pre-batch hash.
- [ ] Quit classification: `cargo test --locked --bin millions_must_die rts_run::tests::event_and_key_quit_sources_are_classified -- --exact --nocapture` → exit 0; `running 1 test`.
- [ ] Frame-1 quit: `MMD_REQUIRE_GPU=1 cargo test --locked --test rts_cli_contract a_quit_on_frame_one_renders_nothing -- --exact --nocapture` → exit 0; `running 1 test`; literal initial hash.
- [ ] Budgets: `MMD_REQUIRE_GPU=1 cargo test --locked --test rts_cli_contract one_and_multi_frame_budgets_keep_exact_hashes -- --exact --nocapture` → exit 0; `running 1 test`; frame 1/2/5 literals.
- [ ] Script quit: `MMD_REQUIRE_GPU=1 cargo test --locked --test rts_cli_contract scripted_quit_hashes_the_last_rendered_world -- --exact --nocapture` → exit 0; `running 1 test`; frame-2 literal.
- [ ] Cross-process determinism: `MMD_REQUIRE_GPU=1 cargo test --locked --test rts_cli_contract the_run_is_deterministic -- --exact --nocapture` → exit 0; `running 1 test`; both = `6be744e0375c56a277e984fec6b1d4f9e0a9d5ba294c2bf8f8832ace2acf28b2`.
- [ ] Window/fallback endpoint: `MMD_REQUIRE_GPU=1 cargo test --locked --test rts_cli_contract the_window_is_released_before_it_drops -- --exact --nocapture` → exit 0; `running 1 test`; final = `3a07a50808798e18f8882379f121d77371fb70add5447dd8a59dae72dcf4b7b2`.
- [ ] Tracked acceptance: `SDL_VIDEODRIVER=offscreen cargo run --locked --quiet -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` → exit 0; exactly one clean-exit line; `frames=1559`, `tick=1519`, `hash=684d69aafa1195e8a02c3f18ea0f8d8af5b56fdd8aac52023e0c30fce3381a46`, `quit=true`; suffix counters unchanged.
- [ ] Hash site audit: `rg -n "state_hash\(\)" src/rts_run.rs` → sole non-test call inside `EndpointHashes::compute`; no per-frame call in `step_frame`.
- [ ] Quit-source audit: `rg -n "RtsCommand::Quit|session\.quit|state\.quit|Event::Quit|break 'running" src/rts_*.rs` → every clean terminal covered by script handling or live classifier; no UI/focus hidden source.
- [ ] Format: `cargo fmt --all -- --check` → exit 0.
- [ ] Full tests: `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` → exit 0; no skipped GPU proof.
- [ ] Lints: `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0.
- [ ] Nix: `nix flake check` → exit 0.
- [ ] App functional: offscreen/window, budget/script/live quit, fatal render/audio paths green; no perf/time gate.
- [ ] Diff fence: `git diff --check && git diff --stat && git status --short` → clean whitespace; only intended files.
- [ ] Commit msg draft: `fix(rts): hash only observable frame endpoints`
