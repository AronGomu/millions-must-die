# ADR 023: Combat gate, scale and the one-time re-baseline

- Status: Accepted
- Date: 2026-08-17
- Supplements: [ADR 022](022_ADR_combat_model_and_enemy_faction.md)

## Context

Phase 2 must prove combat end-to-end on the merge gate. The user chose to put enemies **into the existing** `assets/scenarios/rts_prototype_v1.ron` rather than a separate combat scene, accepting a one-time re-baseline of the two phase-1 tracked scripts. Enemies must spawn outside contact range so the run proves combat *detection* — first damage strictly after first spawn, caused by marching into range.

Planning found the constraint web tighter than assumed: both phase-1 scripts assert exact exit counts over their first ~1600 frames; `tests/rts_cli_contract.rs` pins tracked-scene outcomes (unit/building counts) up to 3000 frames; several engine tests run the tracked scene up to 6000 ticks. A pre-placed enemy group is **provably incompatible** with those pins: any reachable pre-placed Ghoul (0.3 cells/tick, ≤ ~230 cells from the base) arrives by ~tick 810 and ten of them kill the 400-HP HQ around tick 1170 — inside the protected windows, destroying counts that cannot be honestly re-baselined.

## Decision

### Waves only, first spawn after every pinned window

Gate scene enemy block: `pre_placed: []`, two far-edge spawn points, waves totalling **400** Ghouls — a 12-Ghoul fighting wave at tick 3000, then a horde of 150/150/88 at ticks 4100/4160/4220. First contact ≈ tick 3440. Consequences:

- Both phase-1 scripts' asserted counts are **provably unchanged** (their runs end before the first spawn). The one-time re-baseline shrinks to: regenerated scene `.sha256` + the new exit-line tokens, in the same commit as the scene edit.
- "Combat begins by marching" is pinned as `first_combat_tick > 3000` (first spawn tick).
- The `pre_placed` scenario feature still exists and is covered by fixture tests; it is only absent from the gate scene.
- Hundreds scale (300–800 settled) is met at 400.

### Exit tokens and script

Exit line appends, in order: `kills= losses= enemies_spawned= first_combat_tick= hq_alive=`. New tracked script `assets/scenarios/rts_combat_v1.script` (~4500 frames) demonstrates a soldier-ordered kill, an auto-acquire kill, a turret kill, ≥ 1 player loss, `hq_alive=1`, `enemies_spawned=400`. Acceptance tests pin exact observed-then-verified values plus two-run determinism. `docs/05-testing.md` (single source of truth) and its README mirror gain the gate line; the acceptance `RUN_DEADLINE` rises 120 s → 300 s for the longer run.

### Long-horizon tests move to a baseline fixture

The ~8 engine tests that run the tracked scene past tick 3000 (economy, nav staleness families) repoint to a byte-identical **enemy-free** baseline fixture, so they keep testing what they always tested instead of a siege. Phase-0 contracts — 5000-agent exit hash, collision scenes, render goldens, `atlas_count: 4` — do not move.

## Consequences

- The gate proves spawn → march → detection → fight → deaths on the shipped binary, deterministically.
- Phase-1 close docs stay honest: they describe runs that still exist and still pass.
- The waves-only deviation from the original "pre-placed group + waves" plan is deliberate and proven necessary; reversing it requires re-litigating the pin analysis above.
- A longer gate run costs wall-clock time on every merge — accepted, since no perf number gates anything.

## Amendment 2026-08-21 — what the shipped run pins, against what this record predicted

The "Exit tokens and script" section above was written before the script ran. It is left standing as the decision it was; the shipped run is the contract, and where the two disagree the run wins:

- The tracked script demonstrates **`kills >= 1` and `losses >= 1`, unattributed** — not "a soldier-ordered kill, an auto-acquire kill, a turret kill". Nothing on the exit line says which class killed anything (gap G6 in [the phase-2 close](../combat-prototype-functional-close.md)), so no attributed kill is provable from the gate, and the run's own total is `kills=2`. The pinned values are `kills=2 losses=5 enemies_spawned=400 first_combat_tick=3691 hq_alive=1` (`combat_tokens_exact`, `tests/rts_acceptance.rs`); `combat_begins_by_marching` asserts the weaker `>= 1` shape alongside them.
- "First contact ≈ tick 3440" was a pre-run estimate and is superseded by the observed `first_combat_tick=3691`. The claim this record leaned on is unchanged and still pinned: first damage strictly after the first spawn at tick 3000, so combat began by marching.
- `enemies_spawned=400` and `hq_alive=1` landed as decided.

Nothing else moves: waves-only, the wave table, the one-time re-baseline and the long-horizon fixture repoint stand as recorded.
