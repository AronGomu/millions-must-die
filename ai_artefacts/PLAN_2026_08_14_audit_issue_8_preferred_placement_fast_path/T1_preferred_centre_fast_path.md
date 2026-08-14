# T1: Preferred centre fast path

**Plan:** `./ai_artefacts/PLAN_2026_08_14_audit_issue_8_preferred_placement_fast_path.md`  
**Depends:** none  
**Commit outcome:** `nearest_free_body_center` returns reconstructed exact preferred centre with 0 exhaustive cell visits when legal/free; occupied/blocked/excluded/non-centre/illegal cases keep full scan + prior output/ties.

## Context (self-contained)

- Goal: free preferred placement must not scan full map. Exact preferred cell centre legal + free + unexcluded → unique optimum at dist 0 → early return. Else unchanged exhaustive search.
- This slice: whole fix. One fn + test instrumentation + unit tests + one integration regression cmd.
- Out of scope here: F10 overlap-repair gating; nav redesign; `anchor_component` changes; call-site edits; wall-clock/perf number asserts; public export of helper.
- Assumptions in force:
  - Edit only `crates/mmd-engine/src/rts/world.rs` (fn + `#[cfg(test)]` counter + `tests` mod).
  - Fast path **before** `anchor_component(...)` and exhaustive double loop.
  - Counter under `#[cfg(test)]` only — zero shipping cost / zero shipping symbols.
  - Map edge ≤512 (`scenario` contract) → `x as f32 + 0.5` exact; `==` safe for centre equality.
  - Existing sealed-room fixture `sealed_room_and_open_field` (W=H=40) reused. No new fixture file.

## Requirements

### Behaviour — `nearest_free_body_center`

Path: `crates/mmd-engine/src/rts/world.rs`  
Symbol: private fn `nearest_free_body_center(static_nav, placed, ignore, exclude, preferred) -> Option<[f32; 2]>`

After local binds `width`, `height`, `cb`, `diam2` (keep current names/order), **insert fast path**, then existing body:

```rust
// --- FAST PATH (exact preferred cell is unique optimum) ---
// Predicates, all required, short-circuit order fixed:
// 1. finite preferred coords
// 2. exact cell-centre equality + non-negative floor
// 3. in-bounds cell (no clamp)
// 4. component/legal: static_nav.component_at(cell).is_some()
//    (equiv !cb[idx] for on-grid cells; use component_at, not raw cb only)
// 5. exclusion clear (same predicate as exhaustive arm)
// 6. body-clear vs placed/ignore (same predicate as exhaustive arm)
// Hit → return reconstructed centre. Miss → fall through unchanged.
if preferred[0].is_finite() && preferred[1].is_finite() {
    let fx = preferred[0].floor();
    let fy = preferred[1].floor();
    if preferred[0] == fx + 0.5
        && preferred[1] == fy + 0.5
        && fx >= 0.0
        && fy >= 0.0
    {
        let x = fx as u32;
        let y = fy as u32;
        if x < width && y < height {
            let cell = Cell { x, y };
            if static_nav.component_at(cell).is_some() {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                let excluded = exclude.is_some_and(|(min, edge)| {
                    !circle_clear_of_cell_rect(p, RTS_UNIT_BODY_RADIUS_CELLS, min, edge)
                });
                if !excluded {
                    let blocked_by_body = placed.iter().enumerate().any(|(j, &q)| {
                        Some(j) != ignore && dist2(p, q) < diam2
                    });
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
        // cfg(test) visit counter ++ HERE (every iteration, before continues)
        ...
    }
}
best.map(|(_, idx)| [(idx % width) as f32 + 0.5, (idx / width) as f32 + 0.5])
```

**Locked decisions (zero choice):**

| Topic | Decision |
| --- | --- |
| Early-return predicates | finite → exact centre `== floor+0.5` → floor≥0 → `x,y` in-bounds → `component_at.is_some()` → exclude clear → body clear |
| Centre reconstruct | `Some([x as f32 + 0.5, y as f32 + 0.5])` always; never return `preferred` directly |
| Component vs cb | use `component_at(cell).is_some()` only (covers legal + on-grid) |
| Exclude predicate | identical to loop: `exclude.is_some_and(\|(min,edge)\| !circle_clear_of_cell_rect(p, RTS_UNIT_BODY_RADIUS_CELLS, min, edge))` |
| Body predicate | identical to loop: `placed.iter().enumerate().any(\|(j,&q)\| Some(j) != ignore && dist2(p,q) < diam2)` with `diam2 = RTS_UNIT_BODY_DIAMETER_CELLS^2` |
| Fast path vs anchor | fast path first; on hit skip `anchor_component` and full scan |
| Fallback | exhaustive loop body **byte-identical** predicates/tie (`d < bd \|\| (d == bd && idx < bi)`); only add visit counter line under cfg(test) |
| `ignore` | same as loop — self index not obstacle; enables repair/self cases on fast path |
| Non-centre preferred | never fast path even if floor cell free |
| Illegal/blocked centre | `component_at` None → no fast path → anchor+scan |
| NaN/Inf preferred | fail finite check → fallback (existing clamp/scan behaviour) |
| Shipping code | no counter, no test-only branches in release path beyond cfg elision |

### Test instrumentation

In `crates/mmd-engine/src/rts/world.rs` (module level near helper, not inside `RtsWorld` impl):

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

Inside exhaustive `for y` / `for x` loop, **first statement** each iteration:

```rust
#[cfg(test)]
{
    NEAREST_FREE_BODY_CENTER_CELL_VISITS.set(
        NEAREST_FREE_BODY_CENTER_CELL_VISITS.get().saturating_add(1),
    );
}
```

Fast path must **not** touch counter.  
`anchor_component` must **not** touch counter (out of counter scope).

### Doc comment

Extend existing `nearest_free_body_center` doc with one short note:

> When `preferred` is exactly a legal free unexcluded body centre, that centre is the unique optimum (squared distance 0) and is returned without scanning the map.

No perf numbers in docs (repo `no_perf_claim_in_docs` rule).

## Inputs

- `crates/mmd-engine/src/rts/world.rs:375-457` — `nearest_free_body_center` current body.
- `crates/mmd-engine/src/rts/world.rs:459-493` — `anchor_component` (read only; do not edit).
- `crates/mmd-engine/src/rts/world.rs:2654-2758` — existing `tests` mod + `sealed_room_and_open_field`.
- `crates/mmd-engine/src/rts/static_nav.rs` — `component_at`, `center_blocked`, `circle_clear_of_cell_rect`.
- `crates/mmd-engine/src/rts/entity.rs` — `RTS_UNIT_BODY_RADIUS_CELLS=3.0`, `RTS_UNIT_BODY_DIAMETER_CELLS=6.0`.
- `crates/mmd-engine/src/rts/orders.rs` — `dist2`.
- `crates/mmd-engine/src/scenario.rs` — `Cell`.
- Call sites (do not edit): seed `:562`, production `:1789`, evac `:1711`, repair `:2107`.
- Finding: F9 / issue #8 — free preferred still full-scans.
- **From Depends:** none.

## TDD

1. **Red** — add counter hooks (counter stays 0 until loop exists) + all new tests below. Run tests → fast-path work test **fails** (`visits` == 1600 not 0) because no early return yet. Output-only tests may already pass.
2. **Green** — insert fast path block exactly as specified. Fast-path visits → 0. All listed tests pass.
3. **Refactor** — only if needed for clippy; keep green. No abstraction beyond counter helpers.

## Test plan

All new tests live in `crates/mmd-engine/src/rts/world.rs` `#[cfg(test)] mod tests`, same module as `sealed_room_and_open_field`.

Shared setup facts:

- `W = 40`, `H = 40`, `FULL = (W * H) as u64` = `1600`.
- `nav = sealed_room_and_open_field()` — room legal centre only `(5,5)` → `[5.5,5.5]`; open field legal centres include `[28.5, 20.5]` (cell 28,20 clear of solids `[20,38)×[2,38)` and radius-clear interior).
- Before each measured call: `reset_nearest_free_body_center_cell_visits()`.
- After call: `nearest_free_body_center_cell_visits()`.

| Test | Input | Expect |
| --- | --- | --- |
| `exact_preferred_centre_returns_without_full_scan` | `nav`, `placed=&[]`, `ignore=None`, `exclude=None`, `preferred=[5.5,5.5]` | `Some([5.5,5.5])`; **visits == 0** |
| `exact_preferred_centre_ignores_self_body_on_fast_path` | `placed=&[[5.5,5.5]]`, `ignore=Some(0)`, `preferred=[5.5,5.5]` | `Some([5.5,5.5])`; **visits == 0** |
| `occupied_preferred_falls_back_with_full_scan` | open-field `preferred=[28.5,20.5]`, `placed=&[[28.5,20.5]]`, others None | `Some(p)` with `p != [28.5,20.5]`; body-clear vs placed; **visits == 1600**; second oracle call without counter constraint not required — single call asserts output via recompute: result must equal exhaustive optimum (assert `p` is legal free centre; `dist2(p, preferred)` minimal — simplest lock: assert `visits==1600` and `result == nearest_free_body_center` stability by comparing to known first alternative). **Locked oracle:** after result `got`, assert `got.is_some()`; assert `got != Some([28.5,20.5])`; assert `!units_overlap` not needed if using diam2; assert `dist2(got.unwrap(), [28.5,20.5])` is finite; assert no body overlap: `dist2(got.unwrap(), [28.5,20.5]) >= diam2`; assert `component_at` of got cell is Some. Primary: **visits==1600**. |
| `blocked_preferred_centre_does_not_fast_path` | `preferred=[2.5,2.5]` (blocked in room), `placed=&[]` | `Some([5.5,5.5])` (existing illegal-anchor behaviour); **visits == 1600** |
| `excluded_preferred_centre_does_not_fast_path` | `preferred=[28.5,20.5]`, `placed=&[]`, `exclude=Some((Cell{x:28,y:20}, 1))` | `got != Some([28.5,20.5])` and `got.is_some()`; **visits == 1600** |
| `non_centre_preferred_does_not_fast_path` | `preferred=[5.25,5.5]`, `placed=&[]` | `Some([5.5,5.5])`; **visits == 1600** |
| `empty_grid_still_returns_none` | build nav all solids via `StaticNav::from_raw(W,H,vec![true;W*H], RADIUS)`, `preferred=[5.5,5.5]` | `None`; visits == 0 **or** 1600 acceptable — **locked:** fast path fails `component_at`; fallback `anchor_component` → None **before** loop → **visits == 0** |

**Keep green (no edit unless broken by accident):**

| Existing test | Module |
| --- | --- |
| `the_sealed_room_holds_exactly_one_legal_centre` | `world.rs` tests |
| `a_relocation_never_crosses_into_a_disconnected_region` | `world.rs` tests |
| `an_illegal_start_still_anchors_to_its_own_region` | `world.rs` tests |
| `a_produced_unit_spawns_beside_its_building` | `crates/mmd-engine/tests/rts_production.rs` |
| `production_uses_nearest_free_body_position` | `crates/mmd-engine/tests/rts_production.rs` |

### Exact assert snippets (copy)

```rust
#[test]
fn exact_preferred_centre_returns_without_full_scan() {
    let nav = sealed_room_and_open_field();
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[], None, None, [5.5, 5.5]);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(got, Some([5.5, 5.5]));
    assert_eq!(visits, 0, "valid free preferred centre must skip exhaustive scan");
}

#[test]
fn exact_preferred_centre_ignores_self_body_on_fast_path() {
    let nav = sealed_room_and_open_field();
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[[5.5, 5.5]], Some(0), None, [5.5, 5.5]);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(got, Some([5.5, 5.5]));
    assert_eq!(visits, 0);
}

#[test]
fn occupied_preferred_falls_back_with_full_scan() {
    let nav = sealed_room_and_open_field();
    let preferred = [28.5f32, 20.5];
    let diam2 = RTS_UNIT_BODY_DIAMETER_CELLS * RTS_UNIT_BODY_DIAMETER_CELLS;
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[preferred], None, None, preferred);
    let visits = nearest_free_body_center_cell_visits();
    let p = got.expect("open field still has free centres");
    assert_ne!(p, preferred);
    assert!(dist2(p, preferred) >= diam2);
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
    let preferred = [28.5f32, 20.5];
    let exclude = Some((Cell { x: 28, y: 20 }, 1u32));
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[], None, exclude, preferred);
    let visits = nearest_free_body_center_cell_visits();
    assert!(got.is_some());
    assert_ne!(got, Some(preferred));
    assert_eq!(visits, (W * H) as u64);
}

#[test]
fn non_centre_preferred_does_not_fast_path() {
    let nav = sealed_room_and_open_field();
    reset_nearest_free_body_center_cell_visits();
    let got = nearest_free_body_center(&nav, &[], None, None, [5.25, 5.5]);
    let visits = nearest_free_body_center_cell_visits();
    assert_eq!(got, Some([5.5, 5.5]));
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
    assert_eq!(visits, 0, "anchor None must return before exhaustive loop");
}
```

Import note: tests mod already `use super::*` — pulls `Cell`, `dist2`, `StaticNav`, `nearest_free_body_center`, counter fns, `RTS_UNIT_BODY_DIAMETER_CELLS` via entity import already present (`RTS_UNIT_BODY_RADIUS_CELLS`). **Add** `use crate::rts::entity::RTS_UNIT_BODY_DIAMETER_CELLS;` beside existing radius import **or** use `super::RTS_UNIT_BODY_DIAMETER_CELLS` if re-exported through `use super::*` from world imports — world.rs already `use super::entity::{..., RTS_UNIT_BODY_DIAMETER_CELLS, ...}` at top, so `use super::*` exposes it. No extra import if `*` works; if not, add explicit use.

Confirm open-field cell (28,20) is legal centre before relying:

```rust
// optional debug assert inside excluded/occupied tests:
assert!(!nav.center_blocked()[idx(28, 20)], "fixture must expose legal open-field centre");
```

If `(28,20)` ever blocked (should not — interior of `[20,38)×[2,38)` with radius 3 needs ≥3 cells from solid; solid border at x=19 and x=38 → legal x in [23,34] roughly). **Recheck:** solids false on `[20,38)×[2,38)`; outside true. `position_clear` needs ≥radius from solid and map edge. Map edge 0/40; solid at x=19 and x=38.  
Distance from centre x=28.5 to solid column x=19 (right edge 20) = 8.5 > 3; to solid x=38 = 9.5 > 3. y similar. **(28,20) legal. LOCKED.**

If implementer prefers belt-and-suspenders, scan first legal open-field centre with x≥20 — **not allowed**. Use `[28.5, 20.5]` exactly.

## Impl steps

- [ ] 1. Add `thread_local` counter + `reset_nearest_free_body_center_cell_visits` + `nearest_free_body_center_cell_visits` under `#[cfg(test)]` in `crates/mmd-engine/src/rts/world.rs` near `nearest_free_body_center`. *Criterion:* symbols compile under `cargo test -p mmd-engine --locked --lib nearest_free -- --list` or full lib test build.
- [ ] 2. In exhaustive double loop of `nearest_free_body_center`, first line each `(x,y)`: cfg(test) saturating_add 1 on counter. *Criterion:* calling helper on sealed room without fast path later would report 1600 — verified once tests exist.
- [ ] 3. Write all 7 new tests from Test plan into `mod tests` (copy snippets). *Criterion:* `cargo test -p mmd-engine --locked --lib exact_preferred_centre_returns_without_full_scan -- --exact` **fails** red on `visits == 0` (gets 1600).
- [ ] 4. Insert fast path block **after** `width/height/cb/diam2`, **before** `anchor_component`, exactly per Requirements code. *Criterion:* red test now passes.
- [ ] 5. Extend doc comment one sentence (no perf numbers). *Criterion:* comment present above fn.
- [ ] 6. Run full validation cmds below. *Criterion:* all exit 0.
- [ ] 7. Commit impl (not this plan): `perf(rts): skip map scan when preferred body centre free` with DCO `-s`.

## Outputs

- Modified: `crates/mmd-engine/src/rts/world.rs` only.
- Public API: none.
- Behaviour: output-identical for all prior inputs; less work iff exact preferred centre already legal/free/unexcluded.
- Migrate/config: none. State hashes unchanged (same placement results).

## Validation

- [ ] `cargo test -p mmd-engine --locked --lib exact_preferred_centre_returns_without_full_scan -- --exact` → pass; visits assert 0
- [ ] `cargo test -p mmd-engine --locked --lib exact_preferred_centre_ignores_self_body_on_fast_path -- --exact` → pass
- [ ] `cargo test -p mmd-engine --locked --lib occupied_preferred_falls_back_with_full_scan -- --exact` → pass; visits 1600
- [ ] `cargo test -p mmd-engine --locked --lib blocked_preferred_centre_does_not_fast_path -- --exact` → pass
- [ ] `cargo test -p mmd-engine --locked --lib excluded_preferred_centre_does_not_fast_path -- --exact` → pass
- [ ] `cargo test -p mmd-engine --locked --lib non_centre_preferred_does_not_fast_path -- --exact` → pass
- [ ] `cargo test -p mmd-engine --locked --lib empty_grid_still_returns_none -- --exact` → pass
- [ ] `cargo test -p mmd-engine --locked --lib a_relocation_never_crosses_into_a_disconnected_region -- --exact` → pass
- [ ] `cargo test -p mmd-engine --locked --lib an_illegal_start_still_anchors_to_its_own_region -- --exact` → pass
- [ ] `cargo test -p mmd-engine --locked --lib the_sealed_room_holds_exactly_one_legal_centre -- --exact` → pass
- [ ] `cargo test -p mmd-engine --locked --test rts_production a_produced_unit_spawns_beside_its_building -- --exact` → pass
- [ ] `cargo test -p mmd-engine --locked --test rts_production production_uses_nearest_free_body_position -- --exact` → pass
- [ ] Batch: `cargo test -p mmd-engine --locked --lib nearest_free -- --nocapture` filters optional; prefer exact list above
- [ ] `cargo test -p mmd-engine --locked --lib` → all lib unit tests pass (includes sealed-room suite)
- [ ] app functional — production spawn path unchanged (regression cmd above)
- [ ] commit msg draft: `perf(rts): skip map scan when preferred body centre free`

### Expected counts

| Case | visits |
| --- | --- |
| fast path hit | `0` |
| any exhaustive fallthrough on 40×40 fixture | `1600` |
| empty grid None via anchor | `0` |

### Work-reduction proof (no wall-clock)

- Pre: every call runs `width*height` loop iterations after O(1) or O(map) anchor.
- Post hit: 1 centre equality + 1 `component_at` + ≤1 exclude check + 1 `placed` scan; loop iterations **0**.
- Post miss: identical loop iteration count `width*height`.
- Tests encode proof as integer equality on counter — not timings.
