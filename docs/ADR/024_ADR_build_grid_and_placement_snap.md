# ADR 024: Visible build grid, footprint re-cut and placement snap

- Status: Accepted
- Date: 2026-08-20
- Supplements: [ADR 013](013_ADR_phase1_scope_and_rts_entity_model.md), [ADR 014](014_ADR_movable_camera_texture_table_and_ui_layer.md)
- Amends the scene geometry assumed by: [ADR 023](023_ADR_combat_gate_scale_and_rebaseline.md)

## Context

Buildings placed anywhere on the cell grid. Footprints 12 / 8 / 10 / 6 cells — no common divisor, no shared lattice. Two buildings side by side left half-cell seams, and nothing on screen told the player where a building could legally start.

User feedback (round 2, item 1): units should keep moving on the true coordinate grid while a **larger visible grid** carries building placement, smallest building = one square, barracks = 2 × 2, buildings aligned.

## Decision

### One square, 8 cells, buildings only

`BUILD_SQUARE_CELLS = 8`. Footprints re-cut to whole squares: Depot 8 (1 × 1), Turret 8 (1 × 1), Barracks 16 (2 × 2), HQ 24 (3 × 3).

**Units are untouched.** Movement, bodies, separation, flow fields and gathering all keep working in true float cells; nothing about a unit snaps, and no movement code reads `BUILD_SQUARE_CELLS`. The square is a placement and rendering concept only. Any future doc sentence implying units move square to square is wrong.

### Snap at one place, validate at load

The ghost's min corner is floored to a square boundary in `snap_to_build_square`, applied inside `ghost_min_corner` — the single funnel every placement path already goes through, including the assisted search, which now enumerates squares instead of cells.

Scenario-declared buildings must be square-aligned too, checked by the scenario validator before any bounds or terrain rule, so a hand-authored scene can never contain a base the player could not have built.

### The overlay is a placement aid first

The build lattice replaces the per-cell lattice on the existing `show_grid` setting, and is additionally forced on whenever a placement ghost is pending, whatever the setting says. The ghost draws one tile per square rather than per cell.

### The 3 × 3 HQ, and what it costs

A 24-cell HQ at `hq_cell (160,160)` covers `160..184`. That swallows the tracked scene's worker spawn row (`y = 178`) — which the validator rejects — and the acceptance script's Depot plot `(180,176)`. The camera starts on the HQ centre, so the centre moving `(166,166) → (172,172)` shifts every world-space click in both tracked scripts by `+24` px on `y`.

A 2 × 2 HQ (16 cells) would have avoided all of it. The user chose 3 × 3 knowing the cost. The scene therefore moves its spawn row to `y = 190`, the Depot plot to `(184,176)`, the Barracks plot to `(144,176)`, and both tracked scripts are re-authored in the same commit.

### One re-baseline for the whole phase

The footprint change is the phase's single re-authoring of scripted coordinates and pinned counts. It happens in one commit, with every count re-verified rather than re-recorded: a count that moves must be explained in the commit message before its assertion is touched. The later combat-gate commit (ADR 023) regenerates the scene sidecar again, but by construction no scripted step or asserted count moves there.

## Consequences

- Bases line up; a player can read placement legality off the screen.
- Building footprints grow: the HQ doubles its edge, which changes how much of a small map a base occupies. Scene authors must budget for it — the validator will tell them.
- Every future scenario is on the lattice, including the sandbox scene, by validation rather than by convention.
- `MAX_GRID_LINES` shrinks by a factor of 8; the ghost's instance count drops from `edge²` cells to `squares²` tiles.
- Reversing the square size is not a constant change: it re-opens the scene layout and both scripts' coordinates. Treat it as a superseding ADR, not a tweak.
