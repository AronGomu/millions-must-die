# Grill: rts-feedback-round2

Source goal: `feedback.md` (7 items) folded into the phase-2 plan
`artifacts/PLAN_2026_08_17_combat-prototype.md`.

## Round 1 — Feedback intake, grid re-cut, plan shape

| #   | Question | Answer | Precision |
| --- | -------- | ------ | --------- |
| 1   | Where does the feedback land vs the combat plan? | One plan — extend `PLAN_2026_08_17_combat-prototype`, feedback tickets inserted before the gate ticket | — |
| 2   | Build-square size and footprint re-cut? | Square = 8 cells: Depot 8 (1×1), Barracks 16 (2×2), HQ 24 (3×3) | — |
| 3   | When is the build grid drawn? | Build grid replaces the cell lattice on the existing `show_grid` toggle; always on when enabled, always shown while placing | Always visible when trying to build something |
| 4   | What snaps to the grid? | Ghost snaps to square boundaries **and** every scenario-declared building must be square-aligned, validated at load | — |
| 5   | Which orders show a target ring? | Order target of every selected unit — gather node, build site, follow/attack target — derived from `Order` each frame | — |
| 6   | Worker status taxonomy? | `MOVING TO MINERAL/GAS`, `COLLECTING MINERAL/GAS`, `RETURNING MINERAL/GAS`, plus `MOVING`, `BUILDING`, `IDLE`; carry amount keeps its own line | — |
| 7   | What is missing from the entity card? | HP (once combat lands) + node yield-per-trip; today's card otherwise accepted | — |
| 8   | Trapped-builder fix? | Repro first — failing test that reproduces the stuck worker, then fix what it exposes; phasing exemption is the fallback design | — |
| 9   | Move marker form? | Static move-flag prop at the destination for N ticks + dashed straight line from each selected unit to its goal | — |
| 10  | Rally / follow depth? | `Order::Follow { target }` + entity rally — rally onto a node ⇒ produced workers gather it; onto a unit/building ⇒ they follow it | Show a flag and a dashed line when a rally point is placed for a building |
| 11  | Sandbox scenario contents? | New tracked `rts_sandbox_v1.ron` — finished HQ + Depot + Barracks, ~6 workers on nodes, ~8 soldiers, enemy waves marching on the base; run with no `--frames` | — |
| 12  | Re-baseline budget? | Exactly one — sequence the grid re-cut before the combat gate ticket so a single ticket re-pins scene, sidecar and both script baselines | — |

## Facts (scout)

- Rally already ships end to end for **cells**: `CommandId::SetRally`, `Prop::RallyFlag`,
  `production.rally[slot]`, produced units get `order_move(rally)` — source:
  `crates/mmd-engine/src/rts/production.rs:224-280`, `crates/mmd-engine/src/rts/world.rs:1670-1700`,
  `crates/mmd-engine/src/rts/world.rs:2043`.
- Selection already covers units, buildings **and** neutral nodes, and one green ring is packed
  per selected entity whatever its kind — source: `crates/mmd-engine/src/rts/selection.rs:145-150`,
  `crates/mmd-engine/src/rts/selection.rs:409-411`, `crates/mmd-engine/src/rts/pack.rs:400-430`.
- Builder evacuation already ships: `finish_site` plans a destination for every body the footprint
  covers, and a plan that cannot place one evacuee aborts the finish, leaving the site walkable at
  `build_ticks - 1` — source: `crates/mmd-engine/src/rts/world.rs:1883-1968`.
- Footprints today: `HQ_FOOTPRINT_CELLS = 12`, `DEPOT_FOOTPRINT_CELLS = 8`,
  `BARRACKS_FOOTPRINT_CELLS = 10`; unit body radius `3.0` cells — source:
  `crates/mmd-engine/src/scenario.rs:98-102`, `crates/mmd-engine/src/rts/entity.rs:22`.
- A cell-lattice grid overlay exists behind `FramePackOptions::show_grid` + a settings toggle —
  source: `crates/mmd-engine/src/rts/pack.rs:134-135`, `:266-305`, `crates/mmd-engine/src/rts/hud.rs:1376`.
- Worker card prints only `CARRYING …`; soldier card is a hard-coded `IDLE` — source:
  `crates/mmd-engine/src/rts/hud.rs:941-1000`.
- The scenario is hash-pinned (`rts_prototype_v1.sha256`, verified at load) and both tracked scripts
  assert exact exit-line counts — source: `crates/mmd-engine/src/scenario.rs:254`,
  `tests/rts_acceptance.rs`.
- No combat code exists anywhere in history (`git log --all --diff-filter=A` over
  `*combat*`/`*enemy*`/`*weapon*`/`*turret*`, plus 12 dangling commits, all clean); the only combat
  artefacts are the phase-2 plan docs and ADR 022/023.

## Round 2 — Scene surgery, follow binding, sandbox waves

Opened because specifying the round-1 answer to Q2 surfaced two collisions that were not visible
when Q2 was asked.

| #   | Question | Answer | Precision |
| --- | -------- | ------ | --------- |
| 1   | The 24-cell HQ swallows the spawn row and the scripted Depot plot | Keep HQ 24 (3×3): move the six spawn cells south, re-author the Depot step's plot, shift every world click by `+24` px, re-pin | — |
| 2   | How does a player order a follow? | Right-click a friendly unit or building issues Follow; right-click an enemy stays Attack from the combat ticket | — |
| 3   | Sandbox waves | Finite but long: ~30 authored waves in the sandbox file, no schema change | — |

## Facts (scout, round 2)

- `rts_prototype_v1.ron` seeds RTS workers from `spawn_cells` — six cells at
  `y = 178`, `x = 162..167` — source: `crates/mmd-engine/src/rts/world.rs:680-681`,
  `assets/scenarios/rts_prototype_v1.ron:11-18`.
- The scenario validator rejects any spawn cell or resource node inside the HQ footprint —
  source: `crates/mmd-engine/src/scenario.rs:915-930`.
- With `hq_cell: (160,160)` and a 24-cell edge the HQ covers `160..184` on both axes, which
  swallows all six spawn cells (`y = 178`) **and** the cell the acceptance script places its Depot
  on (min corner `(180,176)`) — source: `assets/scenarios/rts_acceptance_v1.script:27`.
- Camera start = HQ centre (`world.rs:634`), and every world-space click in both scripts is a
  screen pixel computed from that origin: moving the HQ centre by `(+6,+6)` cells shifts every
  world-targeting click by exactly `(0, +24)` px under `tile_w = 8`, `tile_h = 4`; HUD clicks are
  screen-fixed and do not move — source: `tests/rts_cli_contract.rs:8-18`.
- The plan's own T4 and T5 tickets both claimed props-sheet cell `(4,0)`; they are parallel
  children of T3, so the collision is real — source: `T4_player-combat-commands.md` D13,
  `T5_turret.md` step 1.6. Corrected while merging to `IconAttack = 16`, `IconStop = 17`,
  `IconBuildTurret = 18`, `MoveMarker = 19`.

## Shared understanding

- **Goal:** fold the seven items of `feedback.md` into the phase-2 plan
  (`artifacts/PLAN_2026_08_17_combat-prototype.md`) as executable tickets, without re-opening any
  combat decision already settled by ADR 022/023.
- **Settled:**
  - One plan. Feedback tickets are T7–T11 and T13, inserted before the gate; old T7/T8 became
    T12/T14. T1–T6 keep their content.
  - Build square = 8 cells; Depot 8 (1×1), Turret 8 (1×1), Barracks 16 (2×2), HQ 24 (3×3).
    Units keep the true float cell grid; no movement code reads the square.
  - The ghost snaps its min corner; scenario-declared buildings must be square-aligned, validated
    at load. The build lattice replaces the per-cell lattice on `show_grid` and is forced on while
    a ghost is pending.
  - The 3×3 HQ's scene surgery is accepted: spawn row → `y = 190`, Depot plot → `(184,176)`,
    Barracks plot → `(144,176)`, camera centre → `(172,172)`, both tracked scripts re-authored and
    every pinned count re-verified in one commit.
  - Target ring and card status are derived per frame from the live `Order`; status vocabulary is
    the user's (`MINERAL`), leaving a recorded mismatch with `kind_label`'s `CRYSTAL`.
  - Builder trap is repro-first; the phasing exemption is the fallback, not the opening move.
  - Ground orders plant a bounded, hashed, self-expiring flag; selected movers draw a dashed
    straight bearing, not the flow-field route.
  - `Order::Follow { target }` with stand-off + `FOLLOW_REPATH_CELLS`; `RallyTarget::{Cell,Entity}`;
    one right-click dispatch discriminated by ownership (enemy ⇒ attack, friendly ⇒ follow,
    node ⇒ gather, ground ⇒ move).
  - Sandbox scene `rts_sandbox_v1.ron`: prebuilt base, 8 soldiers, 30 finite waves, untimed, off
    the merge gate; needs a small `serde`-defaulted scenario schema addition.
  - Exactly one re-baseline of scripted coordinates and pinned counts, in T7.
- **Assumptions (logged, not asked):**
  - Feedback item 3 is treated as already shipped except HP (T6) and node `YIELD` (T9), because
    selection, rings and node cards were verified present in code.
  - Item 6 is treated as half shipped: cell rally exists end to end; entity rally and follow are new.
  - The sandbox's 30 waves stay inside T2's `≤ 1200` enemy validator cap.
  - `MoveMarker` takes props-sheet cell `(4,3)` = index 19.
- **Out of scope:** endless wave spawner, flow-field-traced path lines, animated move markers,
  balance, any change to how units move between cells, and every phase-3+ item the combat plan
  already fenced out.
