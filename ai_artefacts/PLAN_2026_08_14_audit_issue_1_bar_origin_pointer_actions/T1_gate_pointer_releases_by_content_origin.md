# T1: Gate pointer releases by content origin

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_1_bar_origin_pointer_actions.md`
**Depends:** none
**Commit outcome:** Live bar-origin/focus-interrupted left/right gestures emit no `RtsCommand`; valid in-content gestures retain current routing.

## Context (self-contained)

- Goal: Fix audit F1 (`46403b67e2f489c3`). In non-16:9 window, button-down in letterbox/pillarbox bar then release in content must do nothing.
- Current bug: `src/rts_run.rs` left release chooses `LeftClick`/`ShiftClick` when `RtsSession::press` is `None`; right release has no down-origin state.
- This slice: Put both live buttons behind pure gesture state. Consume state on release. Clear state on focus loss. Add SDL-display-free regressions plus human checklist step.
- Out of scope here: F2 stale resize viewport; F3 focus/pause behavior except pending button-state clear; `DisplayViewport` math; script input; command semantics; app-wide input refactor; other findings.
- Assumptions in force: `MappedPointer::inside_content` is canonical bar/content bit. Shift sampled on release. Motion from bars still sends `RtsCommand::Move`. Left/right states independent.

## Requirements

- [ ] R1 — left release returns command only after in-content left down plus in-content left up. Criterion: bar-origin, bar-release, stale, focus-cleared left gestures return `None`.
- [ ] R2 — right release returns command only after in-content right down plus in-content right up. Criterion: bar-origin, bar-release, stale, focus-cleared right gestures return `None`.
- [ ] R3 — valid left routing preserves plain click, shift-click, drag discrimination. Criterion: pure test gets exact existing `RtsCommand` variants/coords.
- [ ] R4 — valid right routing preserves context click. Criterion: pure test gets exact `RtsCommand::RightClick` coords.
- [ ] R5 — live focus loss clears both button states without changing pause/UI policy. Criterion: existing `rts_window::handle_focus` call keeps args/result handling; clear closure calls new state reset.
- [ ] R6 — bar motion behavior stays unchanged. Criterion: `Event::MouseMotion` still maps then applies `RtsCommand::Move(mapped.logical)` unconditionally.
- [ ] R7 — manual contract names press-in-bar/move/release trigger. Criterion: T8 checklist covers left, shift-left, right plus valid in-content controls.

## Inputs

- `src/rts_run.rs`: `RtsSession`, `RtsSession::with_sink`, `apply`, `run`, `tests`.
- `src/rts_input.rs`: existing `RtsCommand::{LeftClick,ShiftClick,Drag,RightClick}`; do not edit.
- `crates/mmd-engine/src/render/viewport.rs`: existing `MappedPointer { logical, inside_content }`; do not edit.
- `ai_artefacts/manual_test_checklist.md`: `## T8 aspect-fit-canvas` live-window checks.
- **From Depends:** none.

## TDD

- [ ] **Red 1** — append `rts_run::tests::bar_origin_cancels_left_and_right_gestures` first. Criterion: test encodes all table rows below; first run fails because `PointerButtonState`/methods do not exist.
- [ ] **Red 2** — append `rts_run::tests::focus_loss_cancels_pending_left_and_right_gestures` before impl. Criterion: test arms both buttons, calls `clear`, expects both releases `None`; first run remains red.
- [ ] **Green** — add `PointerButtonState`; route live down/up/focus through it. Criterion: both new exact tests pass; existing `rts_run::tests` pass.
- [ ] **Refactor** — none unless formatter requires layout. Criterion: no abstraction beyond one private state struct; all tests stay green.

## Test plan

Use pure `MappedPointer` fixtures inside `src/rts_run.rs::tests`:

- `bar = MappedPointer { logical: [960.0, 0.0], inside_content: false }`
- `inside = MappedPointer { logical: [960.0, 540.0], inside_content: true }`
- `drag_start = MappedPointer { logical: [800.0, 400.0], inside_content: true }`

| Test | Input | Expect |
| --- | --- | --- |
| `bar_origin_cancels_left_and_right_gestures` | `left_down(bar)` → `left_up(inside, false)` | `None` |
| same | `left_down(bar)` → `left_up(inside, true)` | `None` |
| same | `right_down(bar)` → `right_up(inside)` | `None` |
| same | `left_down(inside)` → `left_up(bar, false)` | `None`; origin consumed |
| same | `right_down(inside)` → `right_up(bar)` | `None`; arm consumed |
| same | fresh second up after either canceled release | `None`; no stale action |
| same | `left_down(inside)` → `left_up(inside, false)` | `Some(RtsCommand::LeftClick([960.0, 540.0]))` |
| same | `left_down(inside)` → `left_up(inside, true)` | `Some(RtsCommand::ShiftClick([960.0, 540.0]))` |
| same | `left_down(drag_start)` → `left_up(inside, false)` | `Some(RtsCommand::Drag([800.0, 400.0], [960.0, 540.0]))` |
| same | `right_down(inside)` → `right_up(inside)` | `Some(RtsCommand::RightClick([960.0, 540.0]))` |
| `focus_loss_cancels_pending_left_and_right_gestures` | arm both at `inside` → `clear()` → release both at `inside` | both `None` |

## Exact design

Add private state directly before `RtsSession` in `src/rts_run.rs`:

```rust
#[derive(Debug, Default)]
struct PointerButtonState {
    left_origin: Option<[f32; 2]>,
    right_armed: bool,
}

impl PointerButtonState {
    fn left_down(&mut self, pointer: MappedPointer);
    fn right_down(&mut self, pointer: MappedPointer);
    fn left_up(&mut self, pointer: MappedPointer, shift: bool) -> Option<RtsCommand>;
    fn right_up(&mut self, pointer: MappedPointer) -> Option<RtsCommand>;
    fn clear(&mut self);
}
```

Method contracts, no alternate design:

- [ ] `left_down`: overwrite `left_origin` with `pointer.inside_content.then_some(pointer.logical)`. Criterion: invalid down cancels stale origin.
- [ ] `right_down`: overwrite `right_armed` with `pointer.inside_content`. Criterion: invalid down cancels stale arm.
- [ ] `left_up`: consume origin first with `let origin = self.left_origin.take()?;`; return `None` when release outside; else return `ShiftClick(pointer.logical)` when `shift`, `Drag(origin, pointer.logical)` when `is_drag(origin, pointer.logical)`, else `LeftClick(pointer.logical)`. Criterion: valid origin required before all left variants.
- [ ] `right_up`: consume arm with `std::mem::take(&mut self.right_armed)`; return `Some(RightClick(pointer.logical))` only when consumed arm plus `pointer.inside_content`; else `None`. Criterion: valid origin plus valid release required.
- [ ] `clear`: `*self = Self::default()`. Criterion: both buttons disarm together.

## Impl steps

- [ ] 1. Import `MappedPointer` beside `DisplayViewport` in `src/rts_run.rs`. Criterion: pure state signatures compile without SDL event types.
- [ ] 2. Add both exact tests under `src/rts_run.rs::tests` using fixtures/assertions from Test plan. Criterion: test names match exactly; test body constructs no SDL context/window/event pump.
- [ ] 3. Run exact F1 test while red. Criterion: nonzero exit cites missing `PointerButtonState`/methods; record failure as expected TDD evidence.
- [ ] 4. Add private `PointerButtonState` plus five methods exactly per Exact design. Criterion: every up consumes own state before returning; no `unwrap`; no cross-button mutation.
- [ ] 5. Replace `RtsSession::press` with `pointer_buttons: PointerButtonState`; initialize with `PointerButtonState::default()` in `RtsSession::with_sink`. Criterion: session owns both button states in one field.
- [ ] 6. Update `apply`'s `RtsCommand::Move` arm to read `session.pointer_buttons.left_origin` for drag preview. Criterion: existing `is_drag` threshold plus `session.drag = Some(DragBox { a, b: p })` stay unchanged.
- [ ] 7. In `WindowEvent::FocusLost` clear closure, replace `session.press = None` with `session.pointer_buttons.clear()`. Criterion: keyboard/drag/world pan clears plus `handle_focus` args/result branches remain byte-for-byte behavior-equivalent.
- [ ] 8. Replace live left down assignment with `session.pointer_buttons.left_down(viewport.map_pointer([x, y]))`. Criterion: down outside clears old origin; no command dispatches on down.
- [ ] 9. Add live `Event::MouseButtonDown { mouse_btn: MouseButton::Right, ... }` arm calling `right_down(viewport.map_pointer([x, y]))`. Criterion: right action now records origin ownership; no command dispatches on down.
- [ ] 10. Rewrite live left up branch to map pointer, set `session.drag = None`, sample Shift as today, call `left_up`, apply/commit pending settings only inside `if let Some(cmd)`. Criterion: no command/settings commit for canceled gesture; successful settings click keeps existing claim/reclaim/save block unchanged.
- [ ] 11. Rewrite live right up branch to apply only `if let Some(cmd) = session.pointer_buttons.right_up(mapped)`. Criterion: both origin plus release gate right order/cancel behavior; `apply` remains sole semantic dispatcher.
- [ ] 12. Leave `Event::MouseMotion` unchanged. Criterion: bar motion still applies clamped `RtsCommand::Move` → edge-pan preserved.
- [ ] 13. Add one unchecked item directly after T8 bar-click item in `ai_artefacts/manual_test_checklist.md`: press left in bar, move into content, release over unit/HUD/placement; repeat with Shift plus right button; expect no action; repeat with down/up inside content; expect normal actions. Criterion: item names no-selection/no-placement/no-HUD/no-cancel/no-order result plus valid controls.
- [ ] 14. Inspect diff for scope. Criterion: touched files only `src/rts_run.rs`, `ai_artefacts/manual_test_checklist.md`; no viewport/script/focus-policy refactor.

## Outputs

- `src/rts_run.rs`: new private `PointerButtonState`; live left/right down/up routing; focus clear; two unit regressions.
- `ai_artefacts/manual_test_checklist.md`: one T8 bar-origin gesture check.
- Public API: none.
- Persisted config/migration/assets: none.
- Script behavior: unchanged.

## Validation

- [ ] Run `cargo test -p millions_must_die --locked rts_run::tests::bar_origin_cancels_left_and_right_gestures -- --exact`. Criterion: exit 0; `1 passed; 0 failed`; bar-origin left, shift-left, right return no command; valid controls pass.
- [ ] Run `cargo test -p millions_must_die --locked rts_run::tests::focus_loss_cancels_pending_left_and_right_gestures -- --exact`. Criterion: exit 0; `1 passed; 0 failed`; clear cancels both armed buttons.
- [ ] Run `cargo test -p millions_must_die --locked rts_run::tests -- --nocapture`. Criterion: exit 0; all `rts_run` unit tests pass.
- [ ] Run `cargo test -p millions_must_die --locked`. Criterion: exit 0; all app unit/integration tests pass.
- [ ] Run `cargo fmt --all -- --check`. Criterion: exit 0; no fmt diff.
- [ ] Run `cargo clippy -p millions_must_die --all-targets --all-features --locked -- -D warnings`. Criterion: exit 0; no warnings.
- [ ] Review `ai_artefacts/manual_test_checklist.md` T8 addition. Criterion: human can reproduce both letterbox/pillarbox origin case plus valid control; do not claim live result unless human runs it.
- [ ] Confirm app functional. Criterion: valid plain/Shift/drag/right commands proven; scripted input untouched; package tests compile green.
- [ ] Commit msg draft: `fix(rts): require content-origin pointer gestures`. Criterion: one compile-green commit after all automated checks.
