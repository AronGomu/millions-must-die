# T1: Gate pointer releases by content origin

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_1_bar_origin_pointer_actions.md`
**Depends:** none
**Commit outcome:** Live bar-origin/focus-interrupted left/right gestures emit no action `RtsCommand`; valid in-content gestures retain current routing.

## Context (self-contained)

- Goal: Fix audit F1 (`46403b67e2f489c3`). In non-16:9 window, button-down in letterbox/pillarbox bar then release in content must do nothing.
- Current bug: `src/rts_run.rs` left release chooses `LeftClick`/`ShiftClick` when `RtsSession::press` is `None`; right release has no down-origin state.
- This slice: Extract one SDL-free router used by every live pointer event arm. Track both buttons there. Extract focus held-input clear used by live `WindowEvent::FocusLost`. Test those same seams, not parallel test-only state.
- Out of scope here: F2 stale resize viewport; F3 focus/pause behavior except held-input clear; `DisplayViewport` math; script input; command semantics; app-wide input refactor; other findings.
- Assumptions in force: `MappedPointer::inside_content` is canonical bar/content bit. Shift sampled on release. Motion from bars still emits `RtsCommand::Move`. Left/right states stay independent. Current `src/rts_run.rs::tests` count is 3; this ticket adds exactly 2 → module count 5.

## Requirements

- [ ] R1 — every live `MouseMotion`, left down/up, right down/up arm delegates mapped input to `LivePointerRouter::route`. Criterion: event match constructs none of `RtsCommand::{Move, LeftClick, ShiftClick, Drag, RightClick}` directly.
- [ ] R2 — left release emits action only after in-content left down plus in-content left up. Criterion: bar-origin, bar-release, stale, focus-cleared left sequences emit no left action.
- [ ] R3 — right release emits action only after in-content right down plus in-content right up. Criterion: bar-origin, bar-release, stale, focus-cleared right sequences emit no right action.
- [ ] R4 — every down overwrites same-button state, including invalid down. Criterion: `left_down(inside) → left_down(bar) → motion(inside) → left_up(inside)` plus right equivalent emit only preserved `Move`; no stale action.
- [ ] R5 — buttons stay independent under interleave. Criterion: releasing left does not disarm right; releasing right does not disarm left.
- [ ] R6 — valid routing preserves plain click, shift-click, drag, right click. Criterion: routed command vectors equal exact variants/coords below.
- [ ] R7 — live focus loss calls `clear_held_input`, which clears router ownership, keyboard hold, drag preview, world keyboard pan, world edge pan without changing pause/UI policy.
- [ ] R8 — bar motion stays unconditional. Criterion: every bar-origin/bar-release regression includes `Motion`; exact output retains `Move(mapped.logical)` while excluding pointer action.
- [ ] R9 — every regression assertion has exact descriptive failure msg from Test plan. Criterion: no unlabeled `assert_eq!`, `assert!`, or grouped row whose failed behavior cannot be identified.
- [ ] R10 — manual contract names press-in-bar/move/release trigger. Criterion: T8 checklist covers left, shift-left, right plus valid in-content controls.

## Inputs

- `src/rts_run.rs`: `RtsSession`, `RtsSession::with_sink`, `apply`, `run`, `tests`; edit here.
- `src/rts_input.rs`: existing `RtsCommand::{Move,LeftClick,ShiftClick,Drag,RightClick}`; read only.
- `src/rts_window.rs`: existing `handle_focus` clear closure contract; read only.
- `crates/mmd-engine/src/render/viewport.rs`: existing `MappedPointer { logical, inside_content }`; read only.
- `crates/mmd-engine/src/rts/world.rs`: existing `RtsWorld::{set_keyboard_pan_dir,keyboard_pan_dir,set_edge_pan_dir,edge_pan_dir}`; read only.
- `assets/scenarios/rts_prototype_v1.ron`: tracked world fixture for focus-clear test; read only.
- `ai_artefacts/manual_test_checklist.md`: `## T8 aspect-fit-canvas`; edit one item.
- **From Depends:** none.

## TDD

- [ ] **Red 1** — append both exact tests below before seam exists. Run exact F1 test. Criterion: nonzero exit names missing `LivePointerEvent`/`LivePointerRouter`/`clear_held_input`; 0 matching tests does not count as red evidence.
- [ ] **Red 2** — extract current live behavior into `LivePointerRouter::route` plus `clear_held_input`; wire all five pointer arms plus focus-loss closure to them without fixing ownership yet. Criterion: app compiles; tests execute same functions live arms call; bar-origin/right/focus assertions fail with their descriptive msgs.
- [ ] **Green** — implement exact state contracts below. Criterion: exactly 2 new tests pass; full `rts_run::tests` count is exactly 5.
- [ ] **Refactor** — none beyond deleting superseded inline routing plus `RtsSession::press`. Criterion: no second pointer FSM, test-only router, SDL event wrapper, or public API.

## Test plan

Fixtures inside `src/rts_run.rs::tests`:

```rust
let bar = MappedPointer {
    logical: [960.0, 0.0],
    inside_content: false,
};
let inside = MappedPointer {
    logical: [960.0, 540.0],
    inside_content: true,
};
let drag_start = MappedPointer {
    logical: [800.0, 400.0],
    inside_content: true,
};
```

Add test-only `route` helper with exact signature. It must call production `LivePointerRouter::route` for every event; no copied routing logic:

```rust
fn route(
    router: &mut LivePointerRouter,
    events: impl IntoIterator<Item = LivePointerEvent>,
) -> Vec<RtsCommand>;
```

### `bar_origin_cancels_left_and_right_gestures`

Each row starts with fresh `LivePointerRouter::default()` unless row says shared. Call `assert_eq!(actual, expect, "<Assertion msg>")` using exact msg.

| Assertion msg | Routed events | Exact emitted command vector |
| --- | --- | --- |
| `left bar origin must preserve motion but emit no click` | `LeftDown(bar)`, `Motion(inside)`, `LeftUp { pointer: inside, shift: false }` | `vec![RtsCommand::Move(inside.logical)]` |
| `shift-left bar origin must preserve motion but emit no shift-click` | `LeftDown(bar)`, `Motion(inside)`, `LeftUp { pointer: inside, shift: true }` | `vec![RtsCommand::Move(inside.logical)]` |
| `right bar origin must preserve motion but emit no right-click` | `RightDown(bar)`, `Motion(inside)`, `RightUp(inside)` | `vec![RtsCommand::Move(inside.logical)]` |
| `left bar release must consume ownership and block stale second release` | `LeftDown(inside)`, `Motion(bar)`, `LeftUp { pointer: bar, shift: false }`, `LeftUp { pointer: inside, shift: false }` | `vec![RtsCommand::Move(bar.logical)]` |
| `right bar release must consume ownership and block stale second release` | `RightDown(inside)`, `Motion(bar)`, `RightUp(bar)`, `RightUp(inside)` | `vec![RtsCommand::Move(bar.logical)]` |
| `invalid left down must overwrite stale valid origin` | `LeftDown(inside)`, `LeftDown(bar)`, `Motion(inside)`, `LeftUp { pointer: inside, shift: false }` | `vec![RtsCommand::Move(inside.logical)]` |
| `invalid right down must overwrite stale valid ownership` | `RightDown(inside)`, `RightDown(bar)`, `Motion(inside)`, `RightUp(inside)` | `vec![RtsCommand::Move(inside.logical)]` |
| `valid plain left gesture must emit move then left-click` | `LeftDown(inside)`, `Motion(inside)`, `LeftUp { pointer: inside, shift: false }` | `vec![RtsCommand::Move(inside.logical), RtsCommand::LeftClick(inside.logical)]` |
| `valid shift-left gesture must emit move then shift-click` | `LeftDown(inside)`, `Motion(inside)`, `LeftUp { pointer: inside, shift: true }` | `vec![RtsCommand::Move(inside.logical), RtsCommand::ShiftClick(inside.logical)]` |
| `valid drag gesture must emit move then exact drag endpoints` | `LeftDown(drag_start)`, `Motion(inside)`, `LeftUp { pointer: inside, shift: false }` | `vec![RtsCommand::Move(inside.logical), RtsCommand::Drag(drag_start.logical, inside.logical)]` |
| `valid right gesture must emit move then right-click` | `RightDown(inside)`, `Motion(inside)`, `RightUp(inside)` | `vec![RtsCommand::Move(inside.logical), RtsCommand::RightClick(inside.logical)]` |
| `left release must not disarm interleaved right gesture` | shared router: `LeftDown(drag_start)`, `RightDown(inside)`, `Motion(inside)`, left up non-shift, `RightUp(inside)` | `vec![RtsCommand::Move(inside.logical), RtsCommand::Drag(drag_start.logical, inside.logical), RtsCommand::RightClick(inside.logical)]` |
| `right release must not disarm interleaved left gesture` | shared router: `RightDown(inside)`, `LeftDown(inside)`, `Motion(inside)`, `RightUp(inside)`, left up with Shift | `vec![RtsCommand::Move(inside.logical), RtsCommand::RightClick(inside.logical), RtsCommand::ShiftClick(inside.logical)]` |

### `focus_loss_cancels_pending_left_and_right_gestures`

1. Load `RtsWorld` from `Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/scenarios/rts_prototype_v1.ron")`; failure msg `tracked RTS scenario must load for focus-clear regression`.
2. Create `RtsSession::default()`.
3. Arm router with `LeftDown(drag_start)`, `RightDown(inside)`, then route `Motion(inside)` through production seam; pass emitted `Move` to `apply` → drag preview exists.
4. Call `apply(..., RtsCommand::PanStart([1.0, 0.0]))`; set world edge pan to `[0.0, 1.0]`.
5. Call production `clear_held_input(&mut world, &mut session)` — same fn live focus-loss closure uses.
6. Route left/right inside releases through session router; collect emitted commands.
7. Use these exact assertions/messages:

| Assertion | Exact expect | Assertion msg |
| --- | --- | --- |
| release command vector | `Vec::<RtsCommand>::new()` | `focus clear must disarm pending left and right gestures` |
| `session.keyboard_held` | `[0.0, 0.0]` | `focus clear must reset held keyboard pan` |
| `session.drag` | `None` | `focus clear must remove drag preview` |
| `world.keyboard_pan_dir()` | `[0.0, 0.0]` | `focus clear must stop world keyboard pan` |
| `world.edge_pan_dir()` | `[0.0, 0.0]` | `focus clear must stop world edge pan` |

## Exact design

Add private enum plus router directly before `RtsSession` in `src/rts_run.rs`:

```rust
#[derive(Debug, Clone, Copy)]
enum LivePointerEvent {
    Motion(MappedPointer),
    LeftDown(MappedPointer),
    LeftUp {
        pointer: MappedPointer,
        shift: bool,
    },
    RightDown(MappedPointer),
    RightUp(MappedPointer),
}

#[derive(Debug, Default)]
struct LivePointerRouter {
    left_origin: Option<[f32; 2]>,
    right_armed: bool,
}

impl LivePointerRouter {
    fn route(&mut self, event: LivePointerEvent) -> Option<RtsCommand>;
    fn left_origin(&self) -> Option<[f32; 2]>;
    fn clear(&mut self);
}

fn clear_held_input(world: &mut RtsWorld, session: &mut RtsSession);
```

`LivePointerRouter::route` contracts, no alternate design:

- [ ] `Motion(pointer)` → `Some(RtsCommand::Move(pointer.logical))`; mutate no ownership.
- [ ] `LeftDown(pointer)` → overwrite `left_origin` with `pointer.inside_content.then_some(pointer.logical)`; return `None`.
- [ ] `RightDown(pointer)` → overwrite `right_armed` with `pointer.inside_content`; return `None`.
- [ ] `LeftUp { pointer, shift }` → consume first via `let origin = self.left_origin.take()?`; outside release returns `None`; inside release returns `ShiftClick(pointer.logical)` when `shift`, else `Drag(origin, pointer.logical)` when `is_drag(origin, pointer.logical)`, else `LeftClick(pointer.logical)`.
- [ ] `RightUp(pointer)` → consume first via `let armed = std::mem::take(&mut self.right_armed)`; return `Some(RightClick(pointer.logical))` only when `armed && pointer.inside_content`; else `None`.
- [ ] `left_origin()` → return field copy; used only by `apply`'s drag preview.
- [ ] `clear()` → `*self = Self::default()`.

`clear_held_input` exact body effects, in order:

1. `session.keyboard_held = [0.0, 0.0];`
2. `session.pointer_router.clear();`
3. `session.drag = None;`
4. `world.set_keyboard_pan_dir([0.0, 0.0]);`
5. `world.set_edge_pan_dir([0.0, 0.0]);`

## Impl steps

- [ ] 1. Import `MappedPointer` beside `DisplayViewport` in `src/rts_run.rs`. Criterion: pure signatures compile without SDL event types.
- [ ] 2. Add exact `LivePointerEvent`, `LivePointerRouter`, method signatures, `clear_held_input`, test helper, two tests, fixtures, sequences, assertion msgs. Criterion: exactly 2 `#[test]` fns added; tests use production seams.
- [ ] 3. Run exact F1 test while red. Criterion: nonzero exit from compile/assert failure; output must not say `0 passed` with success.
- [ ] 4. Implement no-behavior-change extraction checkpoint: move current inline pointer decisions into `route`; replace all five live pointer arms with calls to `route`; replace focus clear closure body with `clear_held_input`. Keep current broken left/right origin rules only for this red checkpoint. Criterion: failing assertions now name bar-origin/right/focus behavior while stack points into production seam used by event arms.
- [ ] 5. Implement `LivePointerRouter` state contracts exactly. Criterion: invalid downs overwrite stale state; releases consume before every return; button branches mutate only own field.
- [ ] 6. Replace `RtsSession::press` with `pointer_router: LivePointerRouter`; init via `LivePointerRouter::default()` in `RtsSession::with_sink`. Criterion: no `press` field/reference remains.
- [ ] 7. Update `apply`'s `RtsCommand::Move` arm to use `session.pointer_router.left_origin()` for drag preview. Criterion: existing `is_drag` threshold plus `DragBox { a, b: p }` behavior unchanged.
- [ ] 8. In `Event::MouseMotion`, map once, route `LivePointerEvent::Motion(mapped)`, apply returned command. Criterion: bar motion still always produces `Move`.
- [ ] 9. In left/right down arms, map once, route matching `LeftDown`/`RightDown`, `debug_assert!` returned value is `None`; dispatch nothing. Criterion: both downs overwrite ownership via tested seam.
- [ ] 10. In left up arm, map once, set `session.drag = None`, sample Shift exactly as current code, route `LeftUp`, apply/commit pending settings only inside `if let Some(cmd)`. Criterion: canceled gesture cannot apply command or commit setting; successful settings click keeps current claim/reclaim/save block unchanged.
- [ ] 11. In right up arm, map once, route `RightUp`, apply only returned command. Criterion: no direct `RightClick` construction remains in event match.
- [ ] 12. In `WindowEvent::FocusLost`, keep `handle_focus` args/result arms unchanged; closure body becomes only `clear_held_input(&mut world, &mut session)`. Criterion: leaving old `session.press = None` fails compile because field is removed; tested helper owns both pointer clear plus all prior held-input effects.
- [ ] 13. Search live event match. Criterion: all five pointer arms call `session.pointer_router.route`; focus-loss closure calls `clear_held_input`; no duplicate pointer action decision stays inline.
- [ ] 14. Add one unchecked item directly after T8 bar-click item in `ai_artefacts/manual_test_checklist.md`: press left in bar, move into content, release over unit/HUD/placement; repeat with Shift plus right button; expect no selection/placement/HUD/cancel/order; repeat down/move/up fully inside; expect normal actions.
- [ ] 15. Inspect diff for scope. Criterion: implementation touches only `src/rts_run.rs`, `ai_artefacts/manual_test_checklist.md`; no viewport/script/focus-policy refactor.

## Outputs

- `src/rts_run.rs`: private `LivePointerEvent`; private `LivePointerRouter`; private `clear_held_input`; live pointer/focus delegation; exactly two unit regressions.
- `ai_artefacts/manual_test_checklist.md`: one T8 bar-origin gesture check.
- Public API: none.
- Persisted config/migration/assets: none.
- Script behavior: unchanged.

## Validation

- [ ] Run `cargo test -p millions_must_die --bin millions_must_die --locked rts_run::tests::bar_origin_cancels_left_and_right_gestures -- --exact`. Criterion: exit 0; exact summary contains `1 passed; 0 failed`; all 13 labeled sequence assertions execute. `0 passed` is failure.
- [ ] Run `cargo test -p millions_must_die --bin millions_must_die --locked rts_run::tests::focus_loss_cancels_pending_left_and_right_gestures -- --exact`. Criterion: exit 0; exact summary contains `1 passed; 0 failed`; all 5 labeled post-clear assertions execute. `0 passed` is failure.
- [ ] Run `bash -o pipefail -c 'out=$(cargo test -p millions_must_die --bin millions_must_die --locked "rts_run::tests::" -- --nocapture 2>&1); printf "%s\n" "$out"; grep -Eq "test result: ok\\. 5 passed; 0 failed;" <<<"$out"'`. Criterion: exit 0; exact `rts_run::tests` count is 5 = 3 baseline + 2 new. Any 0-match/filter drift fails grep.
- [ ] Run `cargo test -p millions_must_die --locked`. Criterion: exit 0; all app unit/integration tests pass.
- [ ] Run `cargo fmt --all -- --check`. Criterion: exit 0; no fmt diff.
- [ ] Run `cargo clippy -p millions_must_die --all-targets --all-features --locked -- -D warnings`. Criterion: exit 0; no warnings.
- [ ] Run `git grep -n -E 'LivePointerEvent::(Motion|LeftDown|LeftUp|RightDown|RightUp)|clear_held_input' -- src/rts_run.rs`. Criterion: live event match shows all 5 route variants; focus-loss closure calls `clear_held_input`; tests call same symbols.
- [ ] Review `ai_artefacts/manual_test_checklist.md` T8 addition. Criterion: human can reproduce letterbox/pillarbox origin cases plus valid controls; do not claim live result unless human runs it.
- [ ] Confirm app functional. Criterion: valid plain/Shift/drag/right commands proven; motion/edge-pan preserved; scripted input untouched; package tests compile green.
- [ ] Commit msg draft: `fix(rts): require content-origin pointer gestures`. Criterion: one compile-green commit after all automated checks.
