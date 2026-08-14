# T1: Preferred centre fast path

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_8_preferred_placement_fast_path.md`  
**Depends:** none  
**Commit outcome:** `nearest_free_body_center` returns the reconstructed exact preferred centre with 0 exhaustive cell visits when that cell is legal/unexcluded/free; occupied/blocked/excluded/non-centre/out-of-bounds/negative/illegal cases keep the full scan and the prior output and tie order.

## Context (self-contained)

- Goal: free preferred placement must not scan the full map. An exact preferred cell centre that is legal + unexcluded + body-free is the unique optimum at squared distance 0 → early return. Everything else is the unchanged exhaustive search.
- This slice: the whole fix. One fn + `#[cfg(test)]` visit counter + unit tests + two integration regression cmds.
- Out of scope here: F10 overlap-repair gating; nav redesign; `anchor_component` changes; call-site edits; wall-clock/perf-number asserts; public export of the helper.
- Assumptions in force:
  - Edit only `crates/mmd-engine/src/rts/world.rs` (fn + `#[cfg(test)]` counter + `tests` mod).
  - Fast path sits **after** the `width`/`height`/`cb`/`diam2` binds and **before** `anchor_component(...)` (`world.rs:420-424`).
  - Counter under `#[cfg(test)]` only — zero shipping cost, zero shipping symbols.
  - Map edge ≤ 512 (`scenario::RTS_MAX_MAP_EDGE`) → `x as f32 + 0.5` is exact in f32 → `==` centre equality is safe.
  - Existing fixture `sealed_room_and_open_field` (W = H = 40) is reused. No new fixture file.
  - Verified facts this ticket relies on (checked against the tree at HEAD `75fe14e`):
    - Lib unit tests are nested: their harness names are `rts::world::tests::<name>` (`cargo test -p mmd-engine --locked --lib -- --list`). A bare short name with `--exact` matches **0** tests and still exits 0 — never use that form.
    - `mmd-engine` lib target currently has **56** tests; this ticket adds **9** → **65**. `rts::world::tests::` currently matches **3** tests → **12** after this ticket.
    - `crates/mmd-engine/tests/rts_production.rs` has **44** tests; its names are top-level, so short name + `--exact` is correct there.
    - `world.rs` tests mod (`:2653-2758`) already provides `W`, `H`, `idx(x, y)`, `sealed_room_and_open_field()`, and `StaticNav::from_raw(W, H, Vec<bool>, RTS_UNIT_BODY_RADIUS_CELLS)` (`static_nav.rs:248`, `#[cfg(test)] pub(crate)`).
    - `world.rs:18-21` imports `RTS_UNIT_BODY_DIAMETER_CELLS`, and the tests mod's `use super::*` re-exposes a parent module's private `use` names — so **no new import is needed** for the diameter constant.
    - `component_at` returns `None` for every `center_blocked` cell (components flood only unblocked seeds, `static_nav.rs:279-289,303-318`), so `component_at(cell).is_some()` already implies `!cb[idx]`; the fast path still spells both out (see predicate 5) so its predicate set is byte-identical to the loop's.

## Requirements

### Behaviour — `nearest_free_body_center`

Path: `crates/mmd-engine/src/rts/world.rs`  
Symbol: private fn `nearest_free_body_center(static_nav, placed, ignore, exclude, preferred) -> Option<[f32; 2]>` (doc `:374-412`, signature `:413-419`, body `:420-457`; exhaustive loop `:427-455` — the range F9 cites as `426-455`).

After the local binds `width`, `height`, `cb`, `diam2` (`world.rs:420-423`, keep current names/order), **insert the fast path**, then the existing body starting at `let anchor = anchor_component(static_nav, preferred)?;` (`world.rs:424`).

```rust
// --- FAST PATH (the exact preferred cell is the unique optimum) ---
// Every predicate is required. Short-circuit order is fixed, and after the
// bounds check the predicates are the exhaustive loop's own, in the loop's
// own order:
//  1. finite preferred coords
//  2. exact cell-centre equality (`== floor + 0.5`) on both axes
//  3. non-negative floor
//  4. in-bounds cell (no clamp) — must precede any `cb` index
//  5. `!cb[idx]`                          == loop arm 1 (`world.rs:430-432`)
//  6. `component_at(cell).is_some()`      == loop arm 2 (`world.rs:433-435`);
//     a legal preferred cell is its own anchor, so `== Some(anchor)` here
//     would compare a value with itself
//  7. exclusion clear                     == loop arm 3 (`world.rs:437-442`)
//  8. body clear vs `placed`/`ignore`     == loop arm 4 (`world.rs:443-448`)
// Hit → return the reconstructed centre. Miss → fall through unchanged.
if preferred[0].is_finite() && preferred[1].is_finite() {
    let fx = preferred[0].floor();
    let fy = preferred[1].floor();
    if preferred[0] == fx + 0.5 && preferred[1] == fy + 0.5 && fx >= 0.0 && fy >= 0.0 {
        let x = fx as u32;
        let y = fy as u32;
        if x < width && y < height {
            let idx = (x + y * width) as usize;
            let cell = Cell { x, y };
            if !cb[idx] && static_nav.component_at(cell).is_some() {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                let excluded = exclude.is_some_and(|(min, edge)| {
                    !circle_clear_of_cell_rect(p, RTS_UNIT_BODY_RADIUS_CELLS, min, edge)
                });
                if !excluded {
                    let blocked_by_body = placed
                        .iter()
                        .enumerate()
                        .any(|(j, &q)| Some(j) != ignore && dist2(p, q) < diam2);
                    if !blocked_by_body {
                        return Some(p);
                    }
                }
            }
        }
    }
}

// --- EXISTING FALLBACK — do not change control flow/predicates/tie rule ---
let anchor = anchor_component(static_nav, preferred)?;
let mut best: Option<(f32, u32)> = None;
for y in 0..height {
    for x in 0..width {
        // cfg(test) visit counter ++ HERE (every iteration, before any continue)
        ...
    }
}
best.map(|(_, idx)| [(idx % width) as f32 + 0.5, (idx / width) as f32 + 0.5])
```

**Locked decisions (zero choice):**

| Topic | Decision |
| --- | --- |
| Early-return predicate order | finite → `== floor + 0.5` both axes → floor ≥ 0 → `x < width && y < height` → `!cb[idx]` → `component_at(cell).is_some()` → exclude clear → body clear. Documented in the comment above, in this order, so the bounds check always precedes the `cb` index |
| `!cb[idx]` | Required, spelled out, even though `component_at(...).is_some()` implies it — the fast path is the one place that would silently place a body on a blocked centre if that invariant ever changed. `idx = (x + y * width) as usize`, same expression as the loop (`world.rs:429`) |
| Component vs anchor | `component_at(cell).is_some()`; never `== Some(anchor)` (self-comparison), never call `anchor_component` on the fast path |
| Centre reconstruct | `Some([x as f32 + 0.5, y as f32 + 0.5])` always; never return the `preferred` bits |
| Exclude predicate | identical to the loop: `exclude.is_some_and(\|(min, edge)\| !circle_clear_of_cell_rect(p, RTS_UNIT_BODY_RADIUS_CELLS, min, edge))` |
| Body predicate | identical to the loop: `placed.iter().enumerate().any(\|(j, &q)\| Some(j) != ignore && dist2(p, q) < diam2)` with `diam2 = RTS_UNIT_BODY_DIAMETER_CELLS * RTS_UNIT_BODY_DIAMETER_CELLS` |
| Fast path vs anchor | fast path first; on a hit skip both `anchor_component` and the full scan |
| Fallback | exhaustive loop stays byte-identical in predicates and tie rule (`d < bd \|\| (d == bd && idx < bi)`, `world.rs:451`); the only added line is the cfg(test) visit counter |
| `ignore` | same as the loop — the named index is not an obstacle to itself, so repair/self cases can hit the fast path |
| Non-centre preferred | never fast-path, even when the floor cell is free |
| Illegal/blocked centre | `cb[idx]` true (equivalently `component_at` `None`) → no fast path → anchor + scan |
| Out-of-bounds / negative preferred | fail the bounds or `>= 0` check → fallback, which clamps as before. `+inf` survives the centre-equality check and casts to `u32::MAX` (saturating cast, no UB), then fails `x < width` |
| NaN/Inf preferred | fail the `is_finite` check → fallback. The guard is kept as the cheapest, clearest gate; it is **not** load-bearing (`NaN == NaN + 0.5` is false; `±inf` fails bounds or `>= 0`), so no test is owed for it |
| Shipping code | no counter, no test-only branch in the release path beyond cfg elision |

### Test instrumentation

In `crates/mmd-engine/src/rts/world.rs`, at module level, **immediately after the closing brace of `nearest_free_body_center` (`world.rs:457`) and before the `anchor_component` doc comment (`world.rs:459`)**:

```rust
#[cfg(test)]
thread_local! {
    static NEAREST_FREE_BODY_CENTER_CELL_VISITS: std::cell::Cell<u64> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_nearest_free_body_center_cell_visits() {
    NEAREST_FREE_BODY_CENTER_CELL_VISITS.set(0);
}

#[cfg(test)]
fn nearest_free_body_center_cell_visits() -> u64 {
    NEAREST_FREE_BODY_CENTER_CELL_VISITS.get()
}
```

Inside the exhaustive `for y` / `for x` loop, as the **first statement** of each iteration (before `let idx = ...`):

```rust
#[cfg(test)]
{
    NEAREST_FREE_BODY_CENTER_CELL_VISITS.set(
        NEAREST_FREE_BODY_CENTER_CELL_VISITS.get().saturating_add(1),
    );
}
```

The fast path must **not** touch the counter. `anchor_component` must **not** touch the counter (its own rescan loop is out of counter scope by design — the counter measures `nearest_free_body_center`'s map scan and nothing else). The counter is thread-local and every measured call is preceded by a reset in the same test, so the default parallel harness is safe.

### Doc comment

Extend the existing `nearest_free_body_center` doc (`world.rs:374-412`) with one short note:

> When `preferred` is exactly a legal free unexcluded body centre, that centre is the unique optimum (squared distance 0) and is returned without scanning the map.

No perf numbers, measurements or speed claims in that sentence — phase 0 proves behaviour, not speed (`AGENT.md`). The `no_perf_claim_in_docs` test scans only the markdown files in `LIVE_DOCS`, so it will not catch a violation here; the rule is enforced by review of this one sentence.

## Inputs

- `crates/mmd-engine/src/rts/world.rs:374-412` — `nearest_free_body_center` doc comment (extend by one sentence).
- `crates/mmd-engine/src/rts/world.rs:413-457` — `nearest_free_body_center` (signature `:413-419`, binds `:420-423`, `anchor_component` call `:424`, loop `:427-455`, result map `:456`).
- `crates/mmd-engine/src/rts/world.rs:464-494` — `anchor_component` (read only; do not edit).
- `crates/mmd-engine/src/rts/world.rs:2653-2758` — existing `#[cfg(test)] mod tests`: `W`, `H`, `idx`, `sealed_room_and_open_field`, three existing tests.
- `crates/mmd-engine/src/rts/static_nav.rs` — `component_at` `:279`, `center_blocked` `:147`, `from_raw` `:248`, `position_clear` `:162`, `circle_clear_of_cell_rect` `:370`.
- `crates/mmd-engine/src/rts/entity.rs:22-24` — `RTS_UNIT_BODY_RADIUS_CELLS = 3.0`, `RTS_UNIT_BODY_DIAMETER_CELLS = 6.0`.
- `crates/mmd-engine/src/rts/orders.rs:254` — `dist2`.
- `crates/mmd-engine/src/scenario.rs` — `Cell`, `RTS_MAX_MAP_EDGE = 512`.
- Call sites (do not edit): seed `:562`, evac `:1711`, production `:1789` (its exact cell centre is built at `:1787`), repair `:2107`.
- Finding: F9 / issue #8 — a free preferred centre still full-scans.
- **From Depends:** none.

## TDD

Order is authoritative and matches Impl steps 1-4. The counter only increments inside the exhaustive loop, so the loop instrumentation must land **before** the tests are run, or the work asserts are vacuous.

1. **Red** — Impl steps 1 + 2 + 3 together: counter helpers, the in-loop increment, and all 9 new tests. Run the red cmds below. Expected: the two fast-path tests **fail** on the visit assert (`left: 1600, right: 0`) because there is no early return yet; the other 7 new tests already pass (they assert fallback output and 1600 visits, which is what the unchanged fn does).
   - `cargo test -p mmd-engine --locked --lib rts::world::tests::` → `running 12 tests` → `test result: FAILED. 10 passed; 2 failed; 0 ignored; 0 measured; 53 filtered out`.
   - Failing names must be exactly `rts::world::tests::exact_preferred_centre_returns_without_full_scan` and `rts::world::tests::exact_preferred_centre_ignores_self_body_on_fast_path`.
2. **Green** — Impl step 4: insert the fast-path block exactly as specified. The two failing tests now report `visits == 0`; all 12 world tests pass.
3. **Refactor** — only what `cargo fmt` / `clippy -D warnings` demand; keep green. No abstraction beyond the counter helpers.

## Test plan

All 9 new tests live in `crates/mmd-engine/src/rts/world.rs` `#[cfg(test)] mod tests`, beside `sealed_room_and_open_field`. No new imports: `use super::*` already provides `Cell`, `dist2`, `StaticNav`, `nearest_free_body_center`, the counter helpers, and `RTS_UNIT_BODY_DIAMETER_CELLS`; `RTS_UNIT_BODY_RADIUS_CELLS` is already explicitly imported by the mod.

Shared setup facts (all recomputed against the fixture geometry, not assumed):

- `W = 40`, `H = 40`, full scan = `(W * H) as u64` = `1600`.
- `nav = sealed_room_and_open_field()`. Room (`[2,9)²` open, solid elsewhere): the sole legal centre is `(5,5)` → `[5.5,5.5]`. Open field (`[20,38) × [2,38)` open): legal centres are `x ∈ [23,34]`, `y ∈ [5,34]`; `(28,20)` is legal.
- Before each measured call: `reset_nearest_free_body_center_cell_visits()`. After the call: `nearest_free_body_center_cell_visits()`.
- Oracles below are the exhaustive loop's own answer (min squared distance to `preferred`, ties to the lower flat index `x + y * 40`), computed from the fixture — every fallback test pins an exact `Some([..])`, so a wrong fallback cannot pass.

| Test | Input (`nav`, `placed`, `ignore`, `exclude`, `preferred`) | Expected result | visits |
| --- | --- | --- | --- |
| `exact_preferred_centre_returns_without_full_scan` | `&[]`, `None`, `None`, `[5.5,5.5]` | `Some([5.5,5.5])` | `0` |
| `exact_preferred_centre_ignores_self_body_on_fast_path` | `&[[5.5,5.5]]`, `Some(0)`, `None`, `[5.5,5.5]` | `Some([5.5,5.5])` | `0` |
| `occupied_preferred_falls_back_with_full_scan` | `&[[28.5,20.5]]`, `None`, `None`, `[28.5,20.5]` | `Some([28.5,14.5])` | `1600` |
| `blocked_preferred_centre_does_not_fast_path` | `&[]`, `None`, `None`, `[2.5,2.5]` | `Some([5.5,5.5])` | `1600` |
| `excluded_preferred_centre_does_not_fast_path` | `&[]`, `None`, `Some((Cell{x:28,y:20},1))`, `[28.5,20.5]` | `Some([28.5,16.5])` | `1600` |
| `non_centre_preferred_does_not_fast_path` | `&[]`, `None`, `None`, `[5.25,5.5]` | `Some([5.5,5.5])` | `1600` |
| `out_of_bounds_preferred_centre_does_not_fast_path` | `&[]`, `None`, `None`, `[45.5,20.5]` | `Some([34.5,20.5])` | `1600` |
| `negative_preferred_centre_does_not_fast_path` | `&[]`, `None`, `None`, `[-3.5,-2.5]` | `Some([5.5,5.5])` | `1600` |
| `empty_grid_still_returns_none` | all-solid `StaticNav::from_raw`, `&[]`, `None`, `None`, `[5.5,5.5]` | `None` | `0` |

Oracle derivations (locked, do not recompute at impl time):

- **Occupied.** The one body sits on `preferred`, so any answer needs `dist2 ≥ diam2 = 36`; the minimum is exactly `36`. Legal open-field centres at squared distance 36 are `(28,14)` idx `588`, `(34,20)` idx `834`, `(28,26)` idx `1068` — `(22,20)` is not a legal centre. Lowest flat index wins → `[28.5,14.5]`.
- **Excluded.** `exclude = ((28,20), 1)` is the closed rect `[28,29] × [20,21]`; a body centre is rejected when its squared distance to that rect is `< 9`. The nearest surviving legal centres are all at squared distance `16` from `preferred`: `(28,16)` idx `668`, `(24,20)` idx `824`, `(32,20)` idx `832`, `(28,24)` idx `988`. Lowest flat index wins → `[28.5,16.5]`. This test also pins the tie rule.
- **Out of bounds.** `[45.5,20.5]` is an exact centre but off-grid; `anchor_component` clamps to `(39,20)`, which is blocked, so its rescan anchors to the open field, and the scan's nearest legal centre is the field's right edge at that row → `[34.5,20.5]`.
- **Negative.** `[-3.5,-2.5]` clamps to `(0,0)`, which is blocked; the nearest legal centre to it is the room's `(5,5)` (squared distance `145`, versus `793` for the nearest open-field centre) → anchor is the room → `[5.5,5.5]`.
- **Empty grid.** Fast path fails `!cb[idx]`; `anchor_component` returns `None` **before** the loop, so `?` returns `None` with `0` visits.

The out-of-bounds and negative tests exist to pin the guard order: an implementation that indexes `cb` before the bounds check, or reconstructs from a wrapped cell, panics or misplaces instead of passing.

**Keep green (do not edit):**

| Existing test | Harness name |
| --- | --- |
| `the_sealed_room_holds_exactly_one_legal_centre` | `rts::world::tests::the_sealed_room_holds_exactly_one_legal_centre` |
| `a_relocation_never_crosses_into_a_disconnected_region` | `rts::world::tests::a_relocation_never_crosses_into_a_disconnected_region` |
| `an_illegal_start_still_anchors_to_its_own_region` | `rts::world::tests::an_illegal_start_still_anchors_to_its_own_region` |
| `a_produced_unit_spawns_beside_its_building` | `rts_production` integration binary, top-level name |
| `production_uses_nearest_free_body_position` | `rts_production` integration binary, top-level name |

### Exact assert snippets (copy verbatim)

```rust
#[test]
fn exact_preferred_centre_returns_without_full_scan() {
    let nav = sealed_room_and_open_field();
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[], None, None, [5.5, 5.5]);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(got, Some([5.5, 5.5]));
    assert_eq!(
        visits, 0,
        "a legal free preferred centre is the unique optimum and must skip the \
         exhaustive scan"
    );
}

#[test]
fn exact_preferred_centre_ignores_self_body_on_fast_path() {
    let nav = sealed_room_and_open_field();
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[[5.5, 5.5]], Some(0), None, [5.5, 5.5]);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(got, Some([5.5, 5.5]));
    assert_eq!(
        visits, 0,
        "the ignored index is not an obstacle to itself on the fast path either"
    );
}

#[test]
fn occupied_preferred_falls_back_with_full_scan() {
    let nav = sealed_room_and_open_field();
    let preferred = [28.5f32, 20.5];
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[preferred], None, None, preferred);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(
        got,
        Some([28.5, 14.5]),
        "an occupied preferred centre falls back to the exhaustive optimum: \
         three legal centres tie one body diameter away and the lowest flat \
         index (588) wins"
    );
    assert_eq!(visits, (W * H) as u64);
}

#[test]
fn blocked_preferred_centre_does_not_fast_path() {
    let nav = sealed_room_and_open_field();
    assert!(nav.center_blocked()[idx(2, 2)]);
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[], None, None, [2.5, 2.5]);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(got, Some([5.5, 5.5]));
    assert_eq!(visits, (W * H) as u64);
}

#[test]
fn excluded_preferred_centre_does_not_fast_path() {
    let nav = sealed_room_and_open_field();
    assert!(!nav.center_blocked()[idx(28, 20)]);
    let preferred = [28.5f32, 20.5];
    let exclude = Some((Cell { x: 28, y: 20 }, 1u32));
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[], None, exclude, preferred);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(
        got,
        Some([28.5, 16.5]),
        "the excluded preferred centre falls back to the nearest centre clear \
         of the rectangle, ties going to the lowest flat index (668)"
    );
    assert_eq!(visits, (W * H) as u64);
}

#[test]
fn non_centre_preferred_does_not_fast_path() {
    let nav = sealed_room_and_open_field();
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[], None, None, [5.25, 5.5]);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(got, Some([5.5, 5.5]));
    assert_eq!(
        visits,
        (W * H) as u64,
        "a point that is not a cell centre is not the optimum by inspection"
    );
}

#[test]
fn out_of_bounds_preferred_centre_does_not_fast_path() {
    let nav = sealed_room_and_open_field();
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[], None, None, [45.5, 20.5]);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(
        got,
        Some([34.5, 20.5]),
        "an exact centre off the grid is bounds-rejected before any cell index \
         and answered by the clamped fallback"
    );
    assert_eq!(visits, (W * H) as u64);
}

#[test]
fn negative_preferred_centre_does_not_fast_path() {
    let nav = sealed_room_and_open_field();
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[], None, None, [-3.5, -2.5]);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(
        got,
        Some([5.5, 5.5]),
        "a negative exact centre is rejected before any cast and answered by \
         the clamped fallback"
    );
    assert_eq!(visits, (W * H) as u64);
}

#[test]
fn empty_grid_still_returns_none() {
    let solids = vec![true; (W * H) as usize];
    let nav = StaticNav::from_raw(W, H, solids, RTS_UNIT_BODY_RADIUS_CELLS);
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[], None, None, [5.5, 5.5]);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(got, None);
    assert_eq!(visits, 0, "a None anchor returns before the exhaustive loop");
}
```

## Impl steps

- [ ] 1. Add the `thread_local` counter plus `reset_nearest_free_body_center_cell_visits` / `nearest_free_body_center_cell_visits` under `#[cfg(test)]` at `crates/mmd-engine/src/rts/world.rs` between the end of `nearest_free_body_center` (`:457`) and the `anchor_component` doc (`:459`). *Criterion:* `cargo test -p mmd-engine --locked --lib -- --list` still prints `56 tests, 0 benchmarks` and the crate compiles. `dead_code` warnings on the two helpers are expected until step 3 uses them; the clippy gate runs only in step 6, after step 4.
- [ ] 2. Add the cfg(test) counter increment as the first statement of each `(x, y)` iteration of the exhaustive loop (`world.rs:428-429`, before `let idx = ...`). *Criterion:* still `56 tests, 0 benchmarks`; no other loop line changed (`git diff` inside the loop is exactly the 6 added lines).
- [ ] 3. Add all 9 tests from the Test plan verbatim into `mod tests`. *Criterion (Red):* `cargo test -p mmd-engine --locked --lib rts::world::tests::` → `running 12 tests` → `test result: FAILED. 10 passed; 2 failed; 0 ignored; 0 measured; 53 filtered out`, the two failures being `exact_preferred_centre_returns_without_full_scan` and `exact_preferred_centre_ignores_self_body_on_fast_path`, each failing on `assert_eq!(visits, 0)` with `left: 1600`.
- [ ] 4. Insert the fast-path block after the `width`/`height`/`cb`/`diam2` binds and before `anchor_component`, exactly as in Requirements. *Criterion (Green):* the same batch cmd → `running 12 tests` → `test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 53 filtered out`.
- [ ] 5. Extend the fn doc comment by the one sentence above. *Criterion:* the sentence is present in `world.rs:374-412` and states no number, measurement or speed claim.
- [ ] 6. Run every Validation cmd below and record the printed counts. *Criterion:* each matches its expected line exactly.
- [ ] 7. Commit the implementation (not this plan): `perf(rts): skip map scan when preferred body centre free`, DCO `git commit -s`.

## Outputs

- Modified: `crates/mmd-engine/src/rts/world.rs` only.
- Public API: none.
- Behaviour: output-identical for every prior input; strictly less work iff the exact preferred centre is already legal, unexcluded and free.
- Migrate/config: none. State hashes unchanged (identical placement results).

## Validation

Every lib line uses the **full harness path** `rts::world::tests::<name>` with `--exact`; a bare short name plus `--exact` matches 0 tests and exits 0, which reads as a pass. Integration names are top-level in their binary, so short name plus `--exact` is correct there. Each line states the counts that must be printed — a `running 0 tests` line fails the step no matter the exit code.

Red (after Impl step 3, before step 4):

- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::exact_preferred_centre_returns_without_full_scan -- --exact` → `running 1 test` → `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 64 filtered out`, panic on `assert_eq!(visits, 0)` with `left: 1600`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::exact_preferred_centre_ignores_self_body_on_fast_path -- --exact` → `running 1 test` → `test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 64 filtered out`, same assert
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::` → `running 12 tests` → `test result: FAILED. 10 passed; 2 failed; 0 ignored; 0 measured; 53 filtered out`

Green (after Impl step 4). Each single-test line must print `running 1 test` then `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 64 filtered out`:

- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::exact_preferred_centre_returns_without_full_scan -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::exact_preferred_centre_ignores_self_body_on_fast_path -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::occupied_preferred_falls_back_with_full_scan -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::blocked_preferred_centre_does_not_fast_path -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::excluded_preferred_centre_does_not_fast_path -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::non_centre_preferred_does_not_fast_path -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::out_of_bounds_preferred_centre_does_not_fast_path -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::negative_preferred_centre_does_not_fast_path -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::empty_grid_still_returns_none -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::the_sealed_room_holds_exactly_one_legal_centre -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::a_relocation_never_crosses_into_a_disconnected_region -- --exact`
- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::an_illegal_start_still_anchors_to_its_own_region -- --exact`

Batches and regressions:

- [ ] `cargo test -p mmd-engine --locked --lib rts::world::tests::` → `running 12 tests` → `test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 53 filtered out`
- [ ] `cargo test -p mmd-engine --locked --lib` → `running 65 tests` → `test result: ok. 65 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out` (56 before this ticket + 9 new)
- [ ] `cargo test -p mmd-engine --locked --test rts_production a_produced_unit_spawns_beside_its_building -- --exact` → `running 1 test` → `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 43 filtered out`
- [ ] `cargo test -p mmd-engine --locked --test rts_production production_uses_nearest_free_body_position -- --exact` → `running 1 test` → `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 43 filtered out`
- [ ] `cargo test -p mmd-engine --locked --test rts_production` → `running 44 tests` → `test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out`

Repo gate subset this change can break (run before the commit in step 7):

- [ ] `cargo fmt --all -- --check` → no output, exit 0
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0, no warning naming `world.rs`
- [ ] `cargo test --workspace --locked` → exit 0; the `mmd-engine` lib binary line reads `running 65 tests` and every binary reports `0 failed`
- [ ] app functional — the production spawn path is unchanged, proved by the two `rts_production` cmds above
- [ ] commit msg draft: `perf(rts): skip map scan when preferred body centre free`

### Expected counts

| Case | visits |
| --- | --- |
| fast-path hit (`exact_*` tests) | `0` |
| any exhaustive fallthrough on the 40×40 fixture | `1600` |
| empty grid, `None` from `anchor_component` before the loop | `0` |

### Work-reduction proof (no wall-clock)

- Pre: every call runs `width * height` loop iterations after `anchor_component`.
- Post hit: one centre-equality test, one `cb` read, one `component_at`, at most one exclude check, one `placed` scan; loop iterations **0**.
- Post miss: identical loop iteration count, `width * height`.
- The tests encode the proof as integer equality on a counter — never a timing, and no perf number gates anything.
