# T1: Schedule RTS hashes only at observable endpoints

**Plan:** `./artifacts/PLAN_2026_08_14_audit_issue_6_rts_per_frame_hash.md`

**Depends:** none

**Commit outcome:** Production RTS loop computes only initial, successful-frame-1, and final-required hashes; last-rendered semantics hold for budget/script/live stops; fatal beats quit; same-host endpoints stay exact.

## Context (self-contained)

- Goal: stop calling `world.state_hash()` after every successful RTS draw. `RtsWorld::state_hash` (`crates/mmd-engine/src/rts/world.rs`) traverses full state and allocates a live-slot `Vec`; only `frame0` + clean-exit hashes are observed.
- Current flow (`src/rts_run.rs`):
  - `run` hashes loaded world once → `RunState::{first_hash,last_hash}`.
  - `step_frame` hashes after every successful draw; frame 1 feeds `rts: frame0`; latest feeds `finish`.
  - Live `Event::Quit` / key-quit use `break 'running` inside the event match → skips post-event `session.take_audio_fatal()` check.
  - Script `quit` returns from `RtsScript::drain_frame` before draw; no hash update on that frame.
- This slice: endpoint scheduler; script quit preflight; live batch pre-capture; post-event fatal-then-quit spine; production-seam call-count + semantic tests; same-host CLI equality without portable literals.
- Out of scope: `RtsWorld::state_hash` body; horde `src/run.rs`; input binding changes; F8; perf/time thresholds; public API; CLI flags/env; gameplay; unconditional cross-host hash constants in `tests/`.
- Assumptions in force:
  - Frame = successfully rendered frame. Failed draw never increments `RunState::frames`.
  - `frame0` hash = world immediately after first successful render (after that frame's tick/pack/draw).
  - Clean-exit hash = last successfully rendered world; if zero renders → loaded initial world.
  - Script `quit` still stops `drain_frame` before later same-frame script cmds apply and before draw.
  - No new `RtsCommand::Quit` key binding.
  - `Event::AppTerminating` stays ignored.
  - Baseline `src/rts_run.rs::tests` count is **3** settings tests. This ticket adds the exact tests named below.

## Requirements

### R1 — Endpoint call budget (production seams only)

| Stop path | Last successful render | Final hash source | `RtsWorld::state_hash` calls via production `EndpointHashes::compute` / `new` |
| --- | ---: | --- | ---: |
| Script `1:quit` | 0 | cached initial | **1** |
| `--frames 1` | 1 | cached frame-1 | **2** |
| `--frames N` (`N > 1`) | N | `final_hash` computes current world once | **3** |
| Script `K:quit` (`K > 2`), no live mutation after last render | K-1 | `final_hash` computes current (= last rendered) once | **3** |
| Live `Event::Quit` / key-derived Quit after exactly 1 render | 1 | cached frame-1 (via `live_quit` or first) | **2** |
| Live `Event::Quit` / key-derived Quit after `R > 1` renders | R | pre-batch `capture_live_quit` snapshot | **3** |
| Script quit on next frame after live mutation (`R > 1`) | R | pre-batch capture from script preflight (before mutation) | **3** |
| Ordinary interactive audio fatal after `R >= 1` (no quit) | R | no clean exit | **2** if `R >= 1` (initial+frame1 only; no final compute) |
| Audio fatal + quit in same live batch after `R > 1` | R | terminal-candidate pre-batch snapshot **allowed** | **3**; `finish` unreachable; `Err` |
| Audio fatal + quit after `R == 1` | 1 | no new compute beyond frame1 | **2**; `finish` unreachable; `Err` |
| Audio fatal + quit after `R == 0` | 0 | initial only | **1**; `finish` unreachable; `Err` |
| Render error on first draw | 0 | no clean exit | **1** (initial only) |
| Render error on later draw after `R_ok >= 1` success | `R_ok` | no clean exit | **2** (initial+frame1; no final compute) |

Criteria:

- [ ] Sole production `world.state_hash()` call sites in `src/rts_run.rs` are inside `EndpointHashes::new` and `EndpointHashes::compute`.
- [ ] Successful frames `>= 2` call neither.
- [ ] Counter is per-scheduler instance, `#[cfg(test)]` only — no process-global atomic.

### R2 — Script quit preflight before live batch

- [ ] Add `RtsScript::requests_quit_on_frame(&self, frame: u64) -> bool` in `src/rts_script.rs`.
- [ ] True iff some entry has `!fired && entry.frame == frame && entry.cmd == RtsCommand::Quit`.
- [ ] Must **not** mark entries fired and must **not** append commands.
- [ ] Live window loop, **after** `let events: Vec<Event> = pump.poll_iter().collect()` and **before** `for event in events`, calls production `preserve_endpoint_before_live_batch` with `next_frame = state.frames + 1`.
- [ ] Capture runs when `script.requests_quit_on_frame(next_frame) || events.iter().any(live_event_requests_quit)`.

### R3 — Explicit live terminal classifier

`live_event_requests_quit(event: &Event) -> bool` is true only for:

1. `Event::Quit { .. }`
2. `Event::KeyDown { keycode: Some(kc), repeat: false, .. }` where `rts_input::command_from_keycode(kc) == Some(RtsCommand::Quit)`

Helper: `key_command_requests_quit(command: Option<RtsCommand>, repeat: bool) -> bool` = `!repeat && command == Some(RtsCommand::Quit)`.

False for: key repeat Quit; Escape; `None`; mouse; focus; resize; `AppTerminating`.

### R4 — Last-rendered preservation

- [ ] When capture runs with `rendered_frames == 0` → `live_quit = Some(initial)`.
- [ ] `rendered_frames == 1` → `live_quit = Some(first_rendered.expect(...))` (no new compute).
- [ ] `rendered_frames > 1` → compute once into `live_quit` unless already `Some` (never overwrite).
- [ ] `final_hash` prefers `live_quit` over computing current world.
- [ ] Ordinary non-terminal batches never capture.

### R5 — Fatal beats quit (exact contract)

- [ ] Direct/key quit arms set `session.quit` / `state.quit` then `break` **inner** event loop only — never `break 'running` from inside the match.
- [ ] After the event `for`, production `live_post_events(session) -> Result<LivePostEvents, RunError>` runs:
  1. If `session.take_audio_fatal()` is `Some(e)` → `Err(RunError::Failed(format!("interactive audio failure: {e}")))`.
  2. Else if `session.quit` → `Ok(LivePostEvents::Quit)`.
  3. Else → `Ok(LivePostEvents::Continue)`.
- [ ] Window loop on `Err`: existing `release_window` then `return Err` (same message shape as today).
- [ ] On `Quit`: `break 'running` (before `step_frame`).
- [ ] On `Continue`: existing `step_frame` path.
- [ ] Fatal+quit after `R > 1`: pre-batch capture may perform call #3; still `Err`; no `finish`; no `rts: clean exit`.
- [ ] Test `live_quit_does_not_mask_audio_fatal` locks this with exact call count **3** after two successful renders.

### R6 — Same-host endpoints only (no portable literal gate)

- [ ] Do **not** add unconditional `INITIAL_HASH` / `FRAME_*_HASH` constants to `tests/rts_cli_contract.rs` as workspace asserts.
- [ ] CLI proofs use offscreen/`SDL_VIDEODRIVER=offscreen` controlled runs and **relative** equality (dual run or budget-vs-script).
- [ ] App-unit exact digests come from direct same-process `RtsWorld` oracle (`load` + `tick` N times + `state_hash`), not pasted host literals.
- [ ] Optional attestation only (comment/plan note, not gate):

| State (tracked scene, default settings; Linux/Vulkan plan-time note) | Digest |
| --- | --- |
| 0 ticks / 0 renders | `a5862bf12c393ebfa6a63b0e3c90d3e149556520385ec4c9ff4f3f47336d3930` |
| 1 tick / frame 1 | `34f5314bc221f94fc0f25ebff74df18255fb0843e0faf1ab92174637c48894fc` |
| 2 ticks / frame 2 | `8c2ea9b57b7655afcb04f1fea2928cc37b7018f3822be23fbfc1ec1e25a5bf7d` |
| 3 ticks / frame 3 | `3a07a50808798e18f8882379f121d77371fb70add5447dd8a59dae72dcf4b7b2` |
| 5 ticks / frame 5 | `1dff55254b069ff74514490e7301d238251d114f3f7496db5a3d69a357cea1ca` |
| 50 ticks / frame 50 | `6be744e0375c56a277e984fec6b1d4f9e0a9d5ba294c2bf8f8832ace2acf28b2` |
| Acceptance 1559 renders / 1519 ticks | `684d69aafa1195e8a02c3f18ea0f8d8af5b56fdd8aac52023e0c30fce3381a46` |

### R7 — No uncontrolled real-window hash equality

- [ ] `the_window_is_released_before_it_drops` keeps release/frames/quit/mode checks only.
- [ ] Do **not** assert final hash equals a literal on the real-driver branch.
- [ ] Exact hash equality lives on offscreen CLI tests and app-unit oracle/production-seam tests only.

### R8 — Quit-source completeness

Current writers (must remain covered; rerun at end):

1. `apply`: only `RtsCommand::Quit` sets `session.quit = true`.
2. `RtsScript::drain_frame`: bare `quit` → returns true before later cmds; `step_frame` sets quit and `Ok(None)`.
3. Live: `Event::Quit`; key path via `command_from_keycode` + `apply` + quit check.
4. `KEY_BINDINGS`: no Quit today.
5. Mouse/UI/focus never write quit; frame budget exits without quit.
6. Draw/`present_error`/startup failures are fatal, not clean quit.

```sh
rg -n "RtsCommand::Quit|session\.quit|state\.quit|Event::Quit|break 'running" src/rts_*.rs
```

### R9 — stdout contract comment only

- [ ] Module docs state: no `frame0` on pre-frame-1 quit; `frame0` hash is post-draw frame 1; clean hash is last successful draw else initial.
- [ ] Line format/fields unchanged. No new stdout/stderr field. No external doc/checklist edit.

## Inputs

- `src/rts_run.rs` — `RunState`, `run`, live `'running` loop, `run_offscreen`, `step_frame`, `finish`, `apply`, `Scratch`, `tests`
- `src/rts_script.rs` — `RtsScript`, `Entry`, `drain_frame`
- `src/rts_input.rs` — `RtsCommand`, `command_from_keycode`, `KEY_BINDINGS` (read)
- `crates/mmd-engine/src/rts/world.rs` — `RtsWorld::load`, `tick`, `state_hash` (read only)
- `crates/mmd-engine/src/alloc_guard.rs` — `MeasureGuard` (hash-path alloc optional proof)
- `crates/mmd-engine/src/render/error.rs` — `RenderError::Sdl` for draw injection
- `tests/rts_cli_contract.rs` — `Cli`, `rts`, `or_skip`, `a_quit_on_frame_one_renders_nothing`, `the_run_is_deterministic`, `the_window_is_released_before_it_drops`
- `assets/scenarios/rts_prototype_v1.ron` — tracked fixture
- **From Depends:** none

## Exact design

### `src/rts_script.rs`

```rust
impl RtsScript {
    /// True when an unfired `quit` is scheduled on `frame`. Peek only.
    pub fn requests_quit_on_frame(&self, frame: u64) -> bool {
        self.entries
            .iter()
            .any(|e| !e.fired && e.frame == frame && e.cmd == RtsCommand::Quit)
    }
}
```

### `src/rts_run.rs` — scheduler

Place private types directly above `RunState`:

```rust
struct EndpointHashes {
    initial: [u8; 32],
    first_rendered: Option<[u8; 32]>,
    live_quit: Option<[u8; 32]>,
    #[cfg(test)]
    hash_calls: u64,
}

enum LivePostEvents {
    Continue,
    Quit,
}
```

Exact methods (final behavior — after green schedule change):

| Method | Behavior |
| --- | --- |
| `EndpointHashes::new(world) -> Self` | `let initial = Self::hash_world(world)` count+digest; `first_rendered=None`; `live_quit=None` |
| `hash_world(world) -> [u8; 32]` private associated | sole wrapper body: `world.state_hash()`; used by `new`/`compute` |
| `compute(&mut self, world) -> [u8; 32]` | `#[cfg(test)] self.hash_calls += 1`; `world.state_hash()` |
| `record_rendered(&mut self, frame, world)` | if `frame == 1` { let h=compute; first_rendered=Some(h); } else { /* no hash */ } |
| `capture_live_quit(&mut self, rendered_frames, world)` | if live_quit.is_some() return; set live_quit from 0→initial / 1→first / >1→compute |
| `final_hash(&mut self, rendered_frames, world) -> [u8; 32]` | if let Some(h)=live_quit {h} else match rendered_frames {0=>initial, 1=>first.expect, _=>compute} |
| `#[cfg(test)] call_count(&self) -> u64` | `hash_calls` |

Counting rule: `new` counts as call #1 (implementation: `new` uses same increment path as `compute`, e.g. body calls `compute` on a partially-init temp **or** increments then `state_hash`). Final observable counts in R1 include that initial call. **Lock:** `new` must increment the test counter exactly once and perform exactly one `state_hash`.

Simplest locked impl:

```rust
fn new(world: &RtsWorld) -> Self {
    let mut s = Self {
        initial: [0; 32],
        first_rendered: None,
        live_quit: None,
        #[cfg(test)]
        hash_calls: 0,
    };
    s.initial = s.compute(world);
    s
}
```

### Production helpers

```rust
fn key_command_requests_quit(command: Option<RtsCommand>, repeat: bool) -> bool;

fn live_event_requests_quit(event: &Event) -> bool;

fn preserve_endpoint_before_live_batch(
    events: &[Event],
    script: &RtsScript,
    next_frame: u64,
    state: &mut RunState,
    world: &RtsWorld,
) {
    if script.requests_quit_on_frame(next_frame)
        || events.iter().any(live_event_requests_quit)
    {
        state.hashes.capture_live_quit(state.frames, world);
    }
}

fn live_post_events(session: &mut RtsSession) -> Result<LivePostEvents, RunError> {
    if let Some(e) = session.take_audio_fatal() {
        return Err(RunError::Failed(format!("interactive audio failure: {e}")));
    }
    if session.quit {
        Ok(LivePostEvents::Quit)
    } else {
        Ok(LivePostEvents::Continue)
    }
}
```

### `RunState`

Replace `first_hash`/`last_hash` with `hashes: EndpointHashes`.

### `run` / `step_frame` / `finish` wiring

1. After world load: `let mut state = RunState { frames:0, quit:false, expected_ticks:0, hashes: EndpointHashes::new(&world) };`
2. `step_frame` after successful `draw` + `maintain_audio`: `state.hashes.record_rendered(frame, world);` then `state.frames = frame;` … **delete** unconditional `state_hash` / first/last assigns.
3. Frame0 print: `hex::encode(state.hashes.first_rendered.expect("frame 1 rendered"))`.
4. Live loop:
   ```rust
   let events: Vec<Event> = pump.poll_iter().collect();
   preserve_endpoint_before_live_batch(&events, &script, state.frames + 1, &mut state, &world);
   for event in events {
       match event {
           Event::Quit { .. } => {
               session.quit = true;
               state.quit = true;
               break; // inner only
           }
           // KeyDown quit arm: apply; if session.quit { state.quit=true; break; }
           // …other arms unchanged…
       }
   }
   match live_post_events(&mut session) {
       Err(e) => {
           release_window(&renderer, window);
           return Err(e);
       }
       Ok(LivePostEvents::Quit) => break 'running,
       Ok(LivePostEvents::Continue) => {}
   }
   // existing step_frame …
   ```
5. `finish(..., state: &mut RunState, ...)`: after unfired + tick checks pass, `let hash = state.hashes.final_hash(state.frames, world);` print `hex::encode(hash)`. Failed checks must not call `final_hash`.

### Offscreen path

`run_offscreen` needs no pre-batch capture (no live events). Scripted quit / budget finals use `final_hash` current-world compute when `frames > 1` and `live_quit` empty — correct because offscreen applies no post-render live mutation before quit/budget stop.

## TDD (locked order — seam-first, then red call-counts)

### Phase A — compile-green behavior-preserving seam (no red yet)

- [ ] **A1** Add `RtsScript::requests_quit_on_frame` + unit test `script_requests_quit_on_frame_preflight` (pure peek). Green immediately.
- [ ] **A2** Introduce `EndpointHashes` with **rolling-compatible temporary** `record_rendered`: still `compute` every successful frame into an internal `rolling_last: Option<[u8;32]>` field used only in this phase; `final_hash` returns `live_quit.or(rolling_last).unwrap_or(initial)` style so stdout matches HEAD while production sites already call `new`/`record_rendered`/`final_hash`.
- [ ] **A3** Wire `RunState`, `step_frame`, `finish(&mut RunState)`, frame0 print through scheduler. Keep live `break 'running` **unchanged** in this phase so behavior stays HEAD.
- [ ] **A4** `cargo test --locked --bin millions_must_die rts_run::tests -- --nocapture` → existing 3 settings tests pass. `cargo check --locked --bin millions_must_die` green.
- [ ] **A5 criterion:** app compiles; no intended new behavioral red yet; rolling still calls `state_hash` per frame.

### Phase B — reds that execute and fail on rolling schedule / missing spine

Add tests **before** endpoint cutover / fatal spine. Each must **compile and run**, fail on assertion (call-count or fatal precedence or pre-mutation hash) — **not** fail by missing symbols.

- [ ] **B1** Add production helpers `key_command_requests_quit`, `live_event_requests_quit`, `preserve_endpoint_before_live_batch`, `live_post_events` with final semantics; wire preserve + inner-break quit + `live_post_events` into live loop (fatal-wins spine lands here so fatal test can go red/green on real path). Keep **rolling** `record_rendered` so call-count tests stay red.
- [ ] **B2** Add all app-unit tests listed in Test plan (except already-green preflight). Run focused filters:
  - Call-count tests **fail** with expected `2`/`3` vs actual `1+N` rolling counts.
  - `scripted_quit_after_live_mutation_keeps_pre_mutation_hash` / `live_event_quit_keeps_last_rendered_hash`: under rolling + capture wiring may already preserve hash (capture or rolling_last) — if already green on hash bytes, that is OK **only if** paired call-count assertion still red on same test or sibling count test.
  - `live_quit_does_not_mask_audio_fatal`: **must fail on HEAD-equivalent outer-break**. After B1 spine wire it goes green on precedence even while counts still rolling — lock expected call count to endpoint budget so it stays red until Phase C if capture+rolling over-counts; expected final count is **3** after two renders with quit+fatal batch (initial+frame1+capture). Under rolling, frame2 also hashed → count **4** → still red. Good.
- [ ] **B3** Add/adjust CLI tests (relative equality). Endpoint byte tests may stay green under rolling; that is acceptable — call-count app-unit reds drive the schedule change. Do **not** add portable literals.
- [ ] **B4 criterion:** `cargo test … endpoint_call_counts_via_step_frame_and_finish -- --exact --nocapture` exits nonzero; output shows assert on call counts; summary not `0 passed` vacuously from compile skip.

### Phase C — green endpoint schedule

- [ ] **C1** Change `record_rendered` to hash **only** `frame == 1`. Remove `rolling_last`.
- [ ] **C2** `final_hash` exact final design (live_quit / 0 / 1 / compute).
- [ ] **C3** All app-unit + CLI tests pass with R1 counts.
- [ ] **C4** Refactor: `rg -n "state_hash\(\)" src/rts_run.rs` → only `EndpointHashes::compute` (+ test oracle helpers under `#[cfg(test)]`).

## Test plan

### Shared fixtures (`src/rts_run.rs::tests`)

```rust
fn tracked_world() -> RtsWorld {
    RtsWorld::load(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/scenarios/rts_prototype_v1.ron"),
    )
    .expect("tracked RTS scenario must load")
}

fn test_scratch() -> Scratch {
    Scratch {
        frame_buf: RtsFrame::new(),
        cmd_buf: Vec::with_capacity(8),
    }
}

fn fresh_state(world: &RtsWorld) -> RunState {
    RunState {
        frames: 0,
        quit: false,
        expected_ticks: 0,
        hashes: EndpointHashes::new(world),
    }
}

fn ok_draw() -> impl FnMut(ScenePass<'_>) -> Result<(), RenderError> {
    |_| Ok(())
}

fn oracle_hash_after_ticks(ticks: u64) -> [u8; 32] {
    let mut w = tracked_world();
    for _ in 0..ticks {
        w.tick();
    }
    w.state_hash()
}
```

Production-seam rule: every call-count/semantic test drives **`EndpointHashes` through `step_frame` and/or `preserve_endpoint_before_live_batch` and/or `finish` / `live_post_events`** — not a parallel reimplementation of the schedule.

### App-unit tests (exact names)

| Test | Drive path | Expect |
| --- | --- | --- |
| `script_requests_quit_on_frame_preflight` | `RtsScript::parse("1:lclick:1,1;3:quit")` | `requests_quit_on_frame(3)` true; `(1)`/`(2)` false; after `drain_frame(3,…)` peek on 3 false; drain still returns true once |
| `endpoint_call_counts_via_step_frame_and_finish` | For each shape: `fresh_state` + `step_frame`/`finish` with empty or quit script | See rows below |
| `scripted_quit_after_live_mutation_keeps_pre_mutation_hash` | 2× `step_frame` ok; `preserve` with empty events + script `3:quit`; `apply LeftClick([960.0, 518.0])` mutates; `step_frame` → `Ok(None)`; `finish` ok | final digest == `oracle_hash_after_ticks(2)`; != post-click `world.state_hash()`; `call_count()==3`; stdout clean exit not required beyond `finish` Ok |
| `live_event_quit_keeps_last_rendered_hash` | 2× `step_frame`; `preserve` with `[Event::Quit{timestamp:0}]`; `apply LeftClick([960.0,518.0])`; set `session.quit=true`; `state.quit=true`; **no** further `step_frame`; `finish` | final == oracle ticks 2; count==3 |
| `live_quit_does_not_mask_audio_fatal` | 2× `step_frame`; `preserve` with `[Event::Quit{timestamp:0}]` (capture call #3); latch `session.audio_fatal=Some(AudioError("injected".into()))`; `session.quit=true`; `live_post_events` | `Err` with `"interactive audio failure:"` + `"injected"`; **do not** call `finish`; `call_count()==3` |
| `ordinary_audio_fatal_skips_finish` | 2× `step_frame`; no preserve quit; latch fatal; `live_post_events` | `Err`; count==2 |
| `render_error_skips_finish_and_final_hash` | 1× ok `step_frame`; next `step_frame` draw `Err(RenderError::Sdl("injected draw".into()))` | `Err`; frames==1; count==2; never `finish` |
| `event_and_key_quit_sources_are_classified` | pure helpers + `Event::Quit` | Quit true; `key_command_requests_quit(Some(Quit), false)` true; repeat true→false; Escape/`None` false |

#### `endpoint_call_counts_via_step_frame_and_finish` rows

Use fresh world/state/session/script/scratch each row. `finish` only on clean paths. Assert `state.hashes.call_count()` and final digest vs oracle where noted.

| Row msg | Actions | count | digest |
| --- | --- | --- | --- |
| `frame1 script quit uses initial only` | script `1:quit`; `step_frame` → `Ok(None)`; `finish` | 1 | oracle 0 ticks |
| `one frame budget` | empty script; 1× ok `step_frame`; `finish` | 2 | oracle 1 tick |
| `five frame budget` | 5× ok `step_frame`; `finish` | 3 | oracle 5 ticks |
| `script quit before frame 3` | script `3:quit`; 2× ok then third `Ok(None)`; `finish` | 3 | oracle 2 ticks |

Exact asserts use labeled messages equal to the row msg column.

### CLI tests (`tests/rts_cli_contract.rs`)

All new/extended equality via existing `rts()` helper (offscreen). **No new portable hash constants.**

| Test | Change | Expect |
| --- | --- | --- |
| `a_quit_on_frame_one_renders_nothing` | keep frames/quit/no-frame0; add second offscreen `1:quit` run | both `final_hash()` equal (same-host) |
| **new** `budget_and_scripted_quit_endpoints_match_same_host` | `rts --frames 1`, `2`, `5`; `rts --frames 9 --inject-input 3:quit` | all `frame0` hashes equal across runs that print frame0; frames1 final==its frame0; frames2 final==`3:quit` final; frames2 `quit=false` vs quit run `quit=true` frames=2 tick=2; frames5 final equals second frames5 run |
| `the_run_is_deterministic` | keep dual 50-frame equality only | **no** literal `FRAME_50` assert |
| `the_window_is_released_before_it_drops` | unchanged hash-wise | still frames=3 quit=true release; **no** final-hash literal |

Serial GPU note: never parallelize CLI GPU tests; `GPU_LOCK` is process-local.

## Impl steps

- [ ] 1. Read `src/rts_run.rs`, `src/rts_script.rs`, `src/rts_input.rs`, `tests/rts_cli_contract.rs`. Rerun quit-source `rg`. Stop if new quit writer appears outside classifier/script.
- [ ] 2. Phase A1: add `requests_quit_on_frame` + `script_requests_quit_on_frame_preflight`. Verify green.
- [ ] 3. Phase A2–A4: add rolling-compatible `EndpointHashes` + `RunState` wire + `finish(&mut _)` + frame0 from `first_rendered`. Compile green; existing 3 tests pass; offscreen smoke still prints same endpoints as HEAD on this host.
- [ ] 4. Phase B1: add classifier/preserve/`live_post_events`; live loop inner-break + post-event spine; call `preserve_endpoint_before_live_batch` immediately after `poll_iter().collect()`.
- [ ] 5. Phase B2: add all remaining app-unit tests with exact names/msgs. Run call-count test → red on counts.
- [ ] 6. Phase B3: CLI relative endpoint test + extend frame1 quit dual-run; do not touch window test hash; do not add literals.
- [ ] 7. Phase C: endpoint-only `record_rendered`; final `final_hash`; drop rolling field.
- [ ] 8. Run every Validation cmd; fix only in-scope failures.
- [ ] 9. Diff fence: only `src/rts_run.rs`, `src/rts_script.rs`, `tests/rts_cli_contract.rs` unless proven otherwise. No `world.rs` hash edits. No perf asserts. No portable literals.
- [ ] 10. Commit draft msg: `fix(rts): hash only observable frame endpoints`.

## Outputs

- `src/rts_script.rs` — `RtsScript::requests_quit_on_frame` + preflight unit test (in script tests module if present, else `rts_run` may host peek test only — **lock:** put `script_requests_quit_on_frame_preflight` in `src/rts_script.rs` `#[cfg(test)]` module next to existing script tests).
- `src/rts_run.rs` — `EndpointHashes`, `LivePostEvents`, classifiers, `preserve_endpoint_before_live_batch`, `live_post_events`, updated `RunState`/`run`/`step_frame`/`finish`/live loop, production-seam unit tests, stdout comment.
- `tests/rts_cli_contract.rs` — relative endpoint test(s); dual-run frame1 quit; no portable hash constants; window test hash-free.
- Public API: none (`requests_quit_on_frame` is inherent on existing `RtsScript` used only by app).
- CLI/stdout fields: unchanged set/order.
- Config/assets/docs/checklist: none.

## Validation

- [ ] `cargo test --locked --bin millions_must_die script_requests_quit_on_frame_preflight -- --exact --nocapture` → exit 0; `running 1 test`; 1 passed.
- [ ] `cargo test --locked --bin millions_must_die rts_run::tests::endpoint_call_counts_via_step_frame_and_finish -- --exact --nocapture` → exit 0; counts 1/2/3/3; oracle digests match.
- [ ] `cargo test --locked --bin millions_must_die rts_run::tests::scripted_quit_after_live_mutation_keeps_pre_mutation_hash -- --exact --nocapture` → exit 0; pre-mutation hash; count 3.
- [ ] `cargo test --locked --bin millions_must_die rts_run::tests::live_event_quit_keeps_last_rendered_hash -- --exact --nocapture` → exit 0; count 3.
- [ ] `cargo test --locked --bin millions_must_die rts_run::tests::live_quit_does_not_mask_audio_fatal -- --exact --nocapture` → exit 0; `Err`; count **3**; finish not ok-path.
- [ ] `cargo test --locked --bin millions_must_die rts_run::tests::ordinary_audio_fatal_skips_finish -- --exact --nocapture` → exit 0; count 2.
- [ ] `cargo test --locked --bin millions_must_die rts_run::tests::render_error_skips_finish_and_final_hash -- --exact --nocapture` → exit 0; count 2; frames 1.
- [ ] `cargo test --locked --bin millions_must_die rts_run::tests::event_and_key_quit_sources_are_classified -- --exact --nocapture` → exit 0.
- [ ] `bash -o pipefail -c 'out=$(cargo test --locked --bin millions_must_die "rts_run::tests::" -- --nocapture 2>&1); printf "%s\n" "$out"; echo "$out" | rg -q "test result: ok\\. [0-9]+ passed"'` → all `rts_run` tests pass (3 baseline settings + new).
- [ ] `MMD_REQUIRE_GPU=1 cargo test --locked --test rts_cli_contract a_quit_on_frame_one_renders_nothing -- --exact --nocapture` → exit 0; dual final hashes equal.
- [ ] `MMD_REQUIRE_GPU=1 cargo test --locked --test rts_cli_contract budget_and_scripted_quit_endpoints_match_same_host -- --exact --nocapture` → exit 0; relative equalities hold.
- [ ] `MMD_REQUIRE_GPU=1 cargo test --locked --test rts_cli_contract the_run_is_deterministic -- --exact --nocapture` → exit 0; dual equal only.
- [ ] `MMD_REQUIRE_GPU=1 cargo test --locked --test rts_cli_contract the_window_is_released_before_it_drops -- --exact --nocapture` → exit 0; no hash-literal assert added.
- [ ] `SDL_VIDEODRIVER=offscreen cargo run --locked --quiet -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` → exit 0; `frames=1559` `tick=1519` `quit=true`; suffix counters unchanged; hash equals same-host prior acceptance on this machine (do not hard-fail CI on foreign hosts via pasted literal in code — operator compares locally).
- [ ] `rg -n "state_hash\(\)" src/rts_run.rs` → production digest only inside `EndpointHashes::compute`.
- [ ] `rg -n "RtsCommand::Quit|session\.quit|state\.quit|Event::Quit|break 'running" src/rts_*.rs` → quit paths covered; no hidden writer.
- [ ] `rg -n "a5862bf12c393ebf|34f5314bc221f94f|FRAME_[0-9]+_HASH|INITIAL_HASH" tests/` → **no matches** (no portable literal gate).
- [ ] `cargo fmt --all -- --check` → exit 0.
- [ ] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` → exit 0.
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0.
- [ ] `nix flake check` → exit 0.
- [ ] No wall-time/perf threshold asserts anywhere in diff.
- [ ] `git diff --check && git diff --stat && git status --short` → clean whitespace; only intended files.
- [ ] Commit msg draft: `fix(rts): hash only observable frame endpoints`

## Residual risks (accepted)

- Key-derived Quit not reachable via current `KEY_BINDINGS`; classifier still mandatory.
- Real-window path can still receive uncontrolled OS input; hash equality not claimed there.
- `pack_frame` allocations out of F7 scope; proof is hash call-count.
- Acceptance hash attestation is host-local; workspace gate is relative/determinism tests + call-counts.
