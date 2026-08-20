# T8: Docs close

**Plan:** `./artifacts/PLAN_2026_08_17_combat-prototype.md`
**Depends:** T7
**Commit outcome:** Phase 2 documented as closed on functional scope: functional-close doc, CONTEXT/DESIGN/AGENT/GLOSSARY updated, doc-lint tests green.

## Context (self-contained)

- Goal: Phase 2 Combat Prototype — weapons, damage, turrets, enemy AI. T1–T7 shipped it; this ticket writes it down the way every prior phase closed.
- This slice: docs only. No code behavior change. **No `.rs`, `.ron`, `.script`, `.sha256` or `.html` file is touched.**
- Out of scope here: ADRs + architecture HTML (produced with the plan itself — reference them, don't rewrite), marketing/vision changes, perf claims of ANY kind, `README.md`, `docs/05-testing.md` (T7 owns the gate list of record), `.dev/decisions/` (ADRs own decisions), and any edit to `tests/validation_contract.rs` (see Known gap G4 below — extending its scanner lists is a code change and stays out of this fence).
- Pattern to follow: `docs/rts-feedback-polish-functional-close.md` is the freshest close (heading `# <Name> — Functional Close`, bold status line with date, proves/does-not-prove tables, collision-contract section, system → test map, proof boundaries, known gaps). `docs/CONTEXT.md` phases 0/1 show the roadmap status-block shape. `AGENT.md` Status paragraphs show the phase-paragraph shape.

### Assumptions in force (decisions settled at detailing time)

- **A1 — settled phase facts to document (from the shipped plan, verbatim contract):** enemies are RTS entities with `OWNER_ENEMY = 1` (`crates/mmd-engine/src/rts/entity.rs`, beside `OWNER_PLAYER = 0` / `OWNER_NEUTRAL = 255`), hard bodies — **ordinary hard pairs; the ADR 021 gather exception is NOT widened**, and every close sentence about collision must name which of the two contracts it means (RTS hard bodies vs horde `sim/` soft separation, ADR 009). `UnitKind::Ghoul` melee: 30 HP / 0 armor, damage 5, cooldown 30 ticks, range 8, speed 18 cells/s. Soldier 40/0, damage 6, cooldown 15, range 24. Worker 25/0, unarmed. Turret 150 HP / 1 armor, damage 10, cooldown 20, range 36 measured from its footprint rectangle, footprint 6 × 6, cost 75 crystal, no supply grant, not a drop-off, **fires silently this phase (named gap)**. HQ 400/2, Depot 150/1, Barracks 200/1. Damage per hit = `max(1, damage - armor)`, instant-hit, no projectile entities. Enemy AI: permanent `Order::AttackMove` at the enemy objective — HQ while it lives, then nearest remaining player building (lowest-slot tie-break), then Idle; all enemies descend **one shared pooled flow field** per objective (`NAV_FIELD_SLOTS = 8` stands). Auto-acquire fires only for `Idle` and `AttackMove`; plain `Move`/`Gather`/`Build` never fire; targeting = nearest in weapon range, lowest-slot tie-break. Deaths: units despawn; buildings destructible including the HQ (un-stamp → **all** pooled fields invalidated); production queue cancels with no refund; HQ death sends gatherers Idle. Sandbox — no win/lose; outcome rides exit tokens `kills= losses= enemies_spawned= first_combat_tick= hq_alive=`. Gate scene `assets/scenarios/rts_prototype_v1.ron` gained enemies (hundreds, 300–800 total), sha256 regenerated, both phase-1 scripts re-baselined **once** — name this honestly as a re-baseline, and state that the phase-1 close docs describe the pre-enemy runs. Hundreds ≠ horde (the phase-3 claim is untouched). Performance unmeasured by policy.
- **A2 — decision: DESIGN.md placement.** The new `## Combat` section is inserted immediately **before** `## Design Decisions` (`docs/DESIGN.md:245` pre-edit) — newest phase last, `Design Decisions` stays terminal. A `Detailed phase-2 designs:` link list is also added under the existing `Detailed phase-1 designs:` block (`docs/DESIGN.md:28-30`).
- **A3 — decision: glossary word forms.** The glossary skill (`.claude/skills/make-glossary-aron/SKILL.md`) wants one lowercase word per row. The brief's entries map to: `ghoul`, `turret`, `attackmove`, `autoacquire`, `wave`, `spawnpoint`, `owner` (covers `OWNER_ENEMY` — the ref column names the constant), `combattokens` (covers the kills/losses token family). New section `## RTS (combat)` inserted between `## RTS (feedback polish)` and `## Render`.
- **A4 — decision: checklist heading.** New section appended at end of `artifacts/manual_test_checklist.md` as `## T8 (combat-prototype) — Combat close: the human-only pass`, following the feedback-polish suffix convention. `manual_checklist_covers_every_human_only_flow` (`tests/validation_contract.rs:1687`) only asserts feedback-polish tokens plus `## T16` presence — pure additions cannot break it.
- **A5 — conflict vs brief, codebase wins: no attack-cursor visual exists.** T4 decision D5 ships attack-arming with *no cursor visual* (the `pending_rally` pattern). The brief's checklist item "A-mode cursor" is therefore written as a **behavior** check (arm → next click resolves as attack/attack-move) plus a named gap ("no visual affordance while armed"), not a cursor-appearance check.
- **A6 — conflict vs brief, codebase reality at detailing time: ADR 022, ADR 023 and the architecture page do not exist yet.** `docs/ADR/` ends at `021_ADR_rts_feedback_polish_and_gather_collision.md`; there is no `docs/combat-prototype-architecture.html`. They are plan-time artifacts expected to land before implementation. Every link this ticket writes to them will turn `every_doc_link_resolves` (`tests/validation_contract.rs:2072`, scans every `.md` under `docs/` recursively) **red** if they are still absent. Impl step 0 verifies them and **stops** if missing.
- **A7 — T7 outputs consumed as placeholders.** T1–T7 are unimplemented at detailing time (working tree carries only plan artifacts), so T7's exact exit-token values and the gate line's frame count are fill-in-at-close placeholders, written `«…»` below. Impl step 0 resolves every one from the repo with an exact command before any doc is written. The five tokens appear on the exit line in this pinned order: `kills=<u32> losses=<u32> enemies_spawned=<u32> first_combat_tick=<u32|none> hq_alive=<0|1>`.
- **A8 — perf-scanner discipline.** `no_perf_claim_in_docs` lives at `tests/validation_contract.rs:1928` and scans `LIVE_DOCS` (`:1758`): `README.md`, `docs/CONTEXT.md`, `docs/05-testing.md`, the four prior close docs, three architecture pages, `CONTRIBUTING.md`. Of this ticket's files only **`docs/CONTEXT.md` is scanned**; the new close doc is not (gap G4). Regardless, every file written here must stay clean against the token lists: `PERF_THRESHOLD_TOKENS` (`:55` — `p95 p99 nmad 16.67 "25 ms" frame-time "frame time" percentile median throughput latency fps`) + `EXTRA_PERF_CLAIM_TOKENS` (`:1779` — `"frames per second" frames/s "per second" hertz " hz" millisecond microsecond nanosecond real-time realtime faster slower "benchmark result"`) + any digit followed by `ms`. Unit speeds are written **`18 cells/s`** (game-mechanics stat; the form trips no token) — never "cells per second". The words "faster"/"slower" are banned from all new prose. Lines that must mention performance carry a `RETIREMENT_MARKERS` word (`:1817`), canonically "**unmeasured**".
- **A9 — overlap-invariant discipline.** `rts_overlap_invariant_names_its_gather_exception` (`tests/validation_contract.rs:1430`) scans `INVARIANT_DOCS` (`:1379`) = `AGENT.md`, `docs/DESIGN.md`, the phase-1.1 close, the polish close. This ticket edits the first two: any line whose prose contains one of `bodies merged / never penetrate / no penetration / body penetration / cannot overlap / cannot penetrate` must carry, on the same line, one of `gather / adr 021 / policy violation / body_overlaps / except / unless`. The texts below comply by construction (they say "ordinary hard pairs — the ADR 021 gather exception is not widened"); do not reword them into an unqualified no-overlap promise.
- **A10 — system → test map honesty.** No validation-contract test pins the phase-2 map (gap G4), so the only guard is this ticket: every test name written into the close doc **must be verified to exist** in the landed test file by grep (step 1.2) before the table is committed. Names below are the tickets' plans; if T1–T7 landed a renamed test, the landed name wins.

## Requirements

- New `docs/combat-prototype-functional-close.md`: systems shipped, what the script proves (named exit tokens + exact values), what it does NOT prove, every known gap, collision-contract naming discipline. Full text in impl step 1.
- `docs/CONTEXT.md` roadmap item 2 (line 101 pre-edit) gains a Status block in the phases-0/1 shape, linking close doc + ADR 022 + ADR 023 + architecture page. Full text in impl step 2.
- `docs/DESIGN.md`: new `## Combat` section (dense bold-lead-in bullet prose like the existing sections) + one phase-2 designs link list. Full text in impl step 3.
- `AGENT.md`: Status section gains the phase-2 paragraph; the fenced merge-gate block gains the combat script line copied verbatim from `docs/05-testing.md` as T7 left it; the `rts`-smoke bullet gains one sentence naming the combat script and its tokens. Full text in impl step 4.
- `artifacts/manual_test_checklist.md`: new human-only section (bars, flash, red dots, attack-arm flow, turret fires on screen). Full text in impl step 5.
- `docs/GLOSSARY.md`: new `## RTS (combat)` section with eight rows. Full text in impl step 6.
- Nothing else. `.dev/decisions/` untouched (ADRs own decisions).

## Inputs

- **From T7 (read from the repo, never from memory):**
  - `«N_COMBAT»` — the frame count on the combat gate line in `docs/05-testing.md` (T7 added `cargo run -- rts --frames <N> --inject-input-file assets/scenarios/rts_combat_v1.script` to the `## Required merge gate` block, currently lines 100–114 pre-T7).
  - `«KILLS» «LOSSES» «ENEMIES_SPAWNED» «FIRST_COMBAT_TICK»` — the exact pinned values asserted by the combat tests in `tests/rts_acceptance.rs` (T7 planned names: `combat_script_clean_exit_with_tokens`, `combat_tokens_exact`, `combat_begins_by_marching`, `combat_script_deterministic`). `hq_alive=1` by T7 contract.
- Pattern docs: `docs/rts-feedback-polish-functional-close.md` (freshest close), `docs/rts-interaction-ui-audio-hardening-functional-close.md`, `docs/CONTEXT.md:36-100` (status-block shape), `AGENT.md:13-58` (Status paragraphs), `AGENT.md:77-92` (gate block), `docs/DESIGN.md` section style, `docs/GLOSSARY.md` table format, `.claude/skills/make-glossary-aron/SKILL.md`.
- Plan-time artifacts to link (must exist — step 0 verifies): `docs/ADR/022_ADR_combat_model_and_enemy_faction.md`, `docs/ADR/023_ADR_combat_gate_scale_and_rebaseline.md`, `docs/combat-prototype-architecture.html`, both listed in `docs/ADR/README.md` (else `adr_index_lists_every_adr_file`, `tests/validation_contract.rs:1974`, is red before this ticket starts).
- Doc-lint tests that must stay green (all in `tests/validation_contract.rs`): `no_perf_claim_in_docs` (:1928), `every_doc_link_resolves` (:2072), `adr_index_lists_every_adr_file` (:1974), `rts_overlap_invariant_names_its_gather_exception` (:1430), `glossary_defines_the_feedback_polish_vocabulary` (:1639), `manual_checklist_covers_every_human_only_flow` (:1687), `required_gate_keeps_phase_smokes` (:1736 — scans `docs/05-testing.md` + `README.md`, neither touched here).
- Landed test files for the system → test map (step 1.2 verifies names): `crates/mmd-engine/tests/rts_combat.rs` (T1/T3), `crates/mmd-engine/tests/rts_enemy.rs` (T2), `crates/mmd-engine/tests/scenario_contract.rs` (T2), `crates/mmd-engine/tests/frame_allocations.rs` (T3), T4/T5/T6 additions in `crates/mmd-engine/tests/rts_hud.rs`, `rts_selection.rs`, `rts_pack.rs`, `rts_minimap.rs`, in-file `mod tests` of `src/rts_ui.rs` / `src/rts_feedback.rs`, and `tests/rts_acceptance.rs` (T7).

## TDD

1. **Red** — not unit TDD (docs ticket): run `cargo test --locked --test validation_contract` **before** writing → must be green (T7 left it green). Any pre-existing red is T7's defect: stop and report, do not paper over it with doc edits.
2. **Green** — write the six files; the same command stays green after every impl step (checkpoints in Validation).
3. **Refactor** — none.

## Test plan

| Test | Run | Expect |
| ---- | --- | ------ |
| `no_perf_claim_in_docs` | `cargo test --locked --test validation_contract no_perf_claim_in_docs` | green — the `docs/CONTEXT.md` edit adds exactly one perf-token line ("Performance stays **unmeasured**", marker on the line); nothing else in any new file carries a token from A8 |
| `every_doc_link_resolves` | `cargo test --locked --test validation_contract every_doc_link_resolves` | green — every relative link in the close doc and CONTEXT block resolves (ADR 022/023, architecture page, prior closes, `05-testing.md`) |
| `adr_index_lists_every_adr_file` | same binary | green — untouched by this ticket; proves step 0's precondition held |
| `rts_overlap_invariant_names_its_gather_exception` | `cargo test --locked --test validation_contract rts_overlap` | green — the AGENT/DESIGN combat prose qualifies every overlap sentence per A9, and both files still contain "soft separation" (pre-existing horde sections, untouched) |
| `glossary_defines_the_feedback_polish_vocabulary` | same binary | green — additions only; the four polish rows are untouched |
| `manual_checklist_covers_every_human_only_flow` | same binary | green — additions only; `## T16` and all polish tokens untouched |
| full suite | `cargo test --workspace --locked` | green — no code changed, so any new red would be an environment or T7 defect, reported not patched |

## Impl steps

- [ ] 0. **Preconditions — resolve every placeholder or stop.**
  - [ ] 0.1 Run `ls docs/ADR/022_ADR_combat_model_and_enemy_faction.md docs/ADR/023_ADR_combat_gate_scale_and_rebaseline.md docs/combat-prototype-architecture.html`. **All three must exist.** If any is missing: STOP — do not write any link to it; report "plan-time artifacts (ADR 022/023 / architecture page) absent; T8 blocked" to the orchestrator (A6).
  - [ ] 0.2 Run `grep -c '022_ADR_combat_model_and_enemy_faction.md\|023_ADR_combat_gate_scale_and_rebaseline.md' docs/ADR/README.md` — expect `2`+ hits. If not, STOP and report (the index is out of this fence; an unlisted ADR reds `adr_index_lists_every_adr_file` regardless of this ticket).
  - [ ] 0.3 Run `grep -n 'rts_combat_v1.script' docs/05-testing.md AGENT.md README.md && ls assets/scenarios/rts_combat_v1.script crates/mmd-engine/src/rts/combat.rs`. Expect: hit in `docs/05-testing.md` (T7's gate line — record its full command text verbatim; its `--frames` value is `«N_COMBAT»`), NO hit yet in `AGENT.md`, both files present. No `docs/05-testing.md` hit → T7 not landed → STOP, report.
  - [ ] 0.4 Run `grep -n 'kills\|losses\|enemies_spawned\|first_combat_tick\|hq_alive' tests/rts_acceptance.rs`. From the pinned assertions record `«KILLS»`, `«LOSSES»`, `«ENEMIES_SPAWNED»`, `«FIRST_COMBAT_TICK»` (and confirm `hq_alive=1`). Also record the asserted first-spawn tick if a `combat_begins_by_marching`-style assertion names one.
  - [ ] 0.5 Run `cargo test --locked --test validation_contract` — must be fully green before any edit (TDD step 1).
  - [ ] 0.6 `«DATE»` = today's date in `YYYY-MM-DD` (the close/commit date).
- [ ] 1. **`docs/combat-prototype-functional-close.md` (new file).**
  - [ ] 1.1 Create the file with the frame below, in this exact section order. Where `«…»` appears, substitute step 0's values. Title + status + intro:

    ```md
    # Combat Prototype — Functional Close

    **Status: closed on functional evidence, «DATE».**

    Phase 2 makes the game fight back. Phase 1 built a base with nobody to
    defend it against; this slice adds the enemy faction, the weapons on both
    sides, the deaths, and the first defensive building — on the same gate
    scene, driven end to end by a new tracked script through the shipped
    binary.

    This slice did **not** ask how fast any of it runs, and this document
    claims nothing about that. Performance stays retired to a later
    optimization phase, exactly as in phases 0, 1, 1.1 and the feedback-polish
    slice.

    What gates a merge is defined in one place only:
    [testing strategy](05-testing.md). The decisions behind the code are
    [ADR 022](ADR/022_ADR_combat_model_and_enemy_faction.md) and
    [ADR 023](ADR/023_ADR_combat_gate_scale_and_rebaseline.md); the shape of
    the slice is on the
    [architecture page](combat-prototype-architecture.html). The earlier phase
    records stand and are not rewritten as if they had included any of this:
    [phase 1](rts-engine-prototype-functional-close.md),
    [phase 1.1](rts-interaction-ui-audio-hardening-functional-close.md),
    [feedback polish](rts-feedback-polish-functional-close.md) — see
    [the re-baseline, named honestly](#the-re-baseline-named-honestly) for
    what changed under them.
    ```
  - [ ] 1.2 Verify every test name for the map before writing it: run `grep -n '^fn \|fn ' crates/mmd-engine/tests/rts_combat.rs crates/mmd-engine/tests/rts_enemy.rs crates/mmd-engine/tests/scenario_contract.rs tests/rts_acceptance.rs crates/mmd-engine/tests/frame_allocations.rs | grep -i 'combat\|ghoul\|enemy\|wave\|turret\|attack\|hp\|bar\|flash\|dot\|kill'` and `grep -rn 'turret\|attack\|enemy\|bar_\|flash' crates/mmd-engine/tests/rts_hud.rs crates/mmd-engine/tests/rts_pack.rs crates/mmd-engine/tests/rts_minimap.rs crates/mmd-engine/tests/rts_selection.rs src/rts_ui.rs src/rts_feedback.rs | grep '#\[test\]\|fn '`. Every name written in 1.4's table must appear in this output; a planned name that was renamed at landing is written as landed (A10).
  - [ ] 1.3 Append section `## What this slice proves` — a `| Claim | Status |` table (polish-close shape) with exactly these rows, each `proven — <file>` using the files confirmed in 1.2:
    - HP/armor columns exist for every entity kind; damage applies `max(1, damage - armor)` and never zeroes a hit — `crates/mmd-engine/tests/rts_combat.rs`
    - Deaths route completely: units despawn, buildings — the HQ included — un-stamp their footprint and invalidate **every** pooled field, production queues cancel with no refund, HQ death idles the gatherers — `crates/mmd-engine/tests/rts_combat.rs`
    - The enemy faction exists as RTS entities: `OWNER_ENEMY = 1`, `UnitKind::Ghoul`, never producible, never supply-counted, excluded from the drag box — `crates/mmd-engine/tests/rts_enemy.rs`
    - Scenario waves spawn deterministically at their exact tick, defer bounded when the store is full, and the validator caps total enemies at 1,200 — `crates/mmd-engine/tests/rts_enemy.rs`, `crates/mmd-engine/tests/scenario_contract.rs`
    - Enemies march on the HQ over one shared pooled flow field (hundreds of enemies, one field — `NAV_FIELD_SLOTS = 8` stands), melee the first player thing in range, retarget the nearest player building when the HQ dies, and go Idle when nothing is left — `crates/mmd-engine/tests/rts_combat.rs`
    - Auto-acquire fires only for `Idle` and `AttackMove`; plain `Move`, `Gather` and `Build` never fire; targeting is nearest-in-range with a lowest-slot tie-break — `crates/mmd-engine/tests/rts_combat.rs`
    - Attack, attack-move and Stop are player commands sharing one executor with the card; workers in an attack-move selection just move — the T4 file set confirmed in 1.2
    - The Turret is worker-built for 75 crystal under the unchanged four placement rules, grants no supply, and auto-fires the nearest enemy measured from its footprint rectangle — the T5 file set confirmed in 1.2
    - Damaged-or-selected entities carry texture-free HP bars, deaths flash a 12-frame procedural ring, enemies are red dots on the minimap — the T6 file set confirmed in 1.2
    - The combat script produces kills from all three source classes (ordered soldier fire, auto-acquire, turret), at least one player loss, and combat that provably begins by marching (`first_combat_tick` strictly after the first spawn) — `tests/rts_acceptance.rs`
    - The whole run is cross-process deterministic and allocates nothing per frame during march + fire + deaths — `tests/rts_acceptance.rs`, `crates/mmd-engine/tests/frame_allocations.rs`
    - Performance — **unmeasured.** Retired to a later optimization phase; nothing here is a speed claim
    - Anything a person can see or hear on real hardware — **unproven by the gate**; see [proof boundaries](#proof-boundaries)
  - [ ] 1.4 Append section `## System → test map` — `| System | Test binary | Named tests |` table, one row per system in 1.3's order (data model / faction + waves / weapons + enemy AI / commands / turret / feedback / gate), files exactly as confirmed in 1.2, and this sentence above the table: "No validation-contract test resolves this table yet (a named gap below); every name in it was checked by hand against a live `#[test]` fn at close time."
  - [ ] 1.5 Append section `## What this slice does not prove` with exactly these bullets:
    - **No window or sound claim.** The gate runs offscreen; nothing proves a bar, flash, dot or turret shot was *visible*, and the turret is **silent by design this phase** — the human checks live in `artifacts/manual_test_checklist.md`, section "T8 (combat-prototype)".
    - **No balance.** Every stat is a placeholder: Ghoul 30 HP/0 armor, damage 5, cooldown 30 ticks, range 8, speed 18 cells/s; Soldier 40/0, 6, 15, 24; Turret 150/1, 10, 20, 36; Worker 25/0 unarmed; HQ 400/2, Depot 150/1, Barracks 200/1. Balance is a later phase.
    - **Hundreds, not the horde.** The gate scene carries 300–800 enemies across a run. The phase-3 claim (tens of thousands) is untouched and nothing here advances it.
    - **One enemy kind, melee only.** No ranged enemy, no worker attack, no projectile entities, no damage types.
    - **No win, no lose.** The run is a sandbox; the outcome is carried by exit tokens, not by any end state.
    - **No performance claim.** Unmeasured, by policy, as in every prior phase.
    - **Unchanged absences.** No fog of war, no zoom, no save/load — as phase 1.1 left them.
  - [ ] 1.6 Append section `## The collision contract, named` with exactly this text (the discipline sentence the brief demands):

    ```md
    Two collision contracts exist, and every sentence here names the one it
    means. Enemies live under the **RTS hard-body contract**: a Ghoul is an
    ordinary hard pair with everything it touches — ADR 021's gather-worker
    exception is **not** widened to enemies, so `body_overlaps=0` on the
    combat run is the same policy claim it was in the feedback-polish close.
    The **horde contract** is the opposite one and is untouched: ADR 009's
    `sim/` soft separation steering never resolves an overlap, and no doc may
    claim horde agents cannot overlap. Nothing in phase 2 touched `sim/`.
    ```
  - [ ] 1.7 Append section `## What the script proves` with the gate line and token table:

    ```md
    The tracked script `assets/scenarios/rts_combat_v1.script` runs on the
    merge gate as
    `cargo run -- rts --frames «N_COMBAT» --inject-input-file assets/scenarios/rts_combat_v1.script`
    and its exit line pins, in order:

    | Token | Value | Meaning |
    | --- | --- | --- |
    | `kills=` | «KILLS» | enemy deaths by combat fire, all three source classes represented |
    | `losses=` | «LOSSES» | player unit + building deaths by combat fire |
    | `enemies_spawned=` | «ENEMIES_SPAWNED» | cumulative Ghouls ever spawned (pre-placed + waves) |
    | `first_combat_tick=` | «FIRST_COMBAT_TICK» | first combat-system damage; strictly after the first spawn, so combat provably began by marching |
    | `hq_alive=` | 1 | the HQ survived this scripted run |

    The values are exact, pinned by `tests/rts_acceptance.rs`, and reproduced
    cross-process; a drifted token is a red gate, not a curiosity.
    ```
  - [ ] 1.8 Append section `## The re-baseline, named honestly`:

    ```md
    Putting enemies into `assets/scenarios/rts_prototype_v1.ron` changed the
    world both phase-1 scripts run in. Their pinned counters and the scene's
    sha256 sidecar were re-baselined **once**, in the same commit as the
    scene change, after verifying each moved value by hand — this was a
    deliberate evidence update, not a way to make a red test green. The
    phase-1 and phase-1.1 close documents describe the pre-enemy runs and
    are not rewritten; their claims hold for the world they measured.
    `body_overlaps=0` still holds on both old scripts with marching enemies
    in frame — enemies are ordinary hard pairs under the ADR 021 policy.
    ```
  - [ ] 1.9 Append section `## Proof boundaries` (polish-close shape): offscreen gate — no window mapped, no pointer grabbed, no audio device opened; therefore no visibility claim for bars/flash/dots/turret fire, no audibility claim for anything (and the turret emits nothing by design); render correctness stays a development-host claim per [testing strategy](05-testing.md); no number in this document is a performance claim (unmeasured).
  - [ ] 1.10 Append section `## Known gaps` with exactly these entries:
    - **G1 — the turret is silent.** No fire SFX asset exists this phase; firing is visible (to a human) but inaudible everywhere.
    - **G2 — the Ghoul wears the Soldier's sheet.** Enemy art is out of scope; the Ghoul renders from `SLOT_RTS_SOLDIER` with the label `GHOUL`. Readability on screen is a checklist judgment, not a proven claim.
    - **G3 — no attack-cursor visual.** Arming Attack changes nothing on screen until the resolving click (same pattern as rally). A misclick while armed is easy; recorded as a feedback-phase candidate.
    - **G4 — the doc-lint scanners do not cover this page yet.** `no_perf_claim_in_docs` (`LIVE_DOCS`) and the overlap-invariant scan (`INVARIANT_DOCS`) in `tests/validation_contract.rs` were not extended — that is a code change and this close is docs-only. Until a follow-up adds this page and a phase-2 map-resolver test, the map in this page is hand-verified only.
    - **Carried forward:** ADR 017's parked single-file corridor defect, and the development host's intermittent GPU device loss under sustained runs — both unchanged from the feedback-polish close.
- [ ] 2. **`docs/CONTEXT.md` — roadmap item 2 status block.**
  - [ ] 2.1 Replace the single line `2. **Combat Prototype** — weapons, damage, turrets, enemy AI.` (line 101 pre-edit) with:

    ```md
    2. **Combat Prototype** — weapons, damage, turrets, enemy AI.
       - Status («DATE»): **closed on functional scope.** The game fights
         back on the same gate scene: an enemy faction (`OWNER_ENEMY`) of
         melee Ghouls spawns from scenario waves — hundreds across a run,
         deliberately not yet the phase-3 horde — marches on the HQ over one
         shared pooled flow field and attacks the first player thing in
         range; Soldiers auto-acquire while Idle or attack-moving, and plain
         Move never fires; damage is instant-hit `max(1, damage - armor)`;
         units despawn on death and buildings — the HQ included — are
         destructible; a worker-built Turret auto-fires, silently for now.
         Enemies collide as ordinary hard RTS bodies: ADR 021's gather
         exception is not widened, and the horde's soft separation is
         untouched. The run is a sandbox — no win or lose; the outcome rides
         five exit tokens (`kills`, `losses`, `enemies_spawned`,
         `first_combat_tick`, `hq_alive`) pinned by a new tracked combat
         script, and both phase-1 scripts plus the scene's sha256 were
         re-baselined once when the gate scene gained its enemies — the
         phase-1 close docs describe the pre-enemy runs. Performance stays
         **unmeasured**. What it proves, what it does not, and every known
         gap: [combat prototype functional close](combat-prototype-functional-close.md).
         The decisions behind it:
         [ADR 022](ADR/022_ADR_combat_model_and_enemy_faction.md) and
         [ADR 023](ADR/023_ADR_combat_gate_scale_and_rebaseline.md), with
         the shape of the slice on the
         [architecture page](combat-prototype-architecture.html).
    ```
  - [ ] 2.2 Checkpoint: `cargo test --locked --test validation_contract no_perf_claim_in_docs` and `cargo test --locked --test validation_contract every_doc_link_resolves` — both green.
- [ ] 3. **`docs/DESIGN.md` — phase-2 designs link + `## Combat` section.**
  - [ ] 3.1 After the `Detailed phase-1 designs:` list (lines 28–30 pre-edit, ending `...(phase 1.1)`), insert:

    ```md

    Detailed phase-2 designs:
    - [Combat prototype](combat-prototype-architecture.html)
    ```
  - [ ] 3.2 Immediately before the line `## Design Decisions` (line 245 pre-edit, now shifted by 3.1), insert this section followed by a blank line:

    ```md
    ## Combat

    Phase 2. One combat contract for every armed thing, player or enemy. The
    [Agent collision](#agent-collision) section above still describes the
    horde's `sim/` soft separation, which phase 2 never touched; everything
    here is the RTS side.

    - **Instant hit, armor floor.** A weapon is damage / cooldown / range
      (`crates/mmd-engine/src/rts/combat.rs`). A hit lands the tick it fires
      — no projectile entities — for `max(1, damage - armor)`, so armor
      mitigates but never zeroes a hit. Placeholder stats, balance being a
      later phase: Soldier 40 HP/0 armor, damage 6, cooldown 15 ticks,
      range 24; Ghoul 30/0, damage 5, cooldown 30, range 8, speed
      18 cells/s; Turret 150/1, damage 10, cooldown 20, range 36; Worker
      25/0, unarmed; HQ 400/2, Depot 150/1, Barracks 200/1.
    - **Targeting.** Nearest valid target inside weapon range, lowest-slot
      tie-break, range measured against the target's body circle or
      footprint rectangle rather than its centre. A unit fires only while
      `Idle`, `Attack` or `AttackMove`; plain `Move`, `Gather` and `Build`
      never fire, so a retreat order is a real retreat. Idle and
      attack-moving armed units auto-acquire and hold in place to fight;
      `Attack` chases its target; a dead target clears the order the same
      tick.
    - **Enemy objective chain.** Every `OWNER_ENEMY` unit holds a permanent
      `Order::AttackMove` at the enemy objective: the HQ while it lives,
      then the nearest remaining player building (lowest-slot tie-break),
      then Idle when no player building remains. The objective recomputes on
      a building death, never by per-tick scan, and all enemies descend one
      shared pooled flow field per objective — hundreds of enemies, one
      field, so "no per-enemy pathfinding" and `NAV_FIELD_SLOTS = 8` both
      stand.
    - **Death is routed, not special-cased.** Units despawn; buildings — the
      HQ included — un-stamp their footprint, which invalidates every pooled
      field; a dying production building cancels its queue with no refund;
      HQ death sends gatherers Idle. The run is a sandbox: no win or lose,
      the outcome rides the exit tokens `kills`/`losses`/`enemies_spawned`/
      `first_combat_tick`/`hq_alive`.
    - **Turret.** `BuildingKind::Turret`: worker-built for 75 crystal under
      the unchanged four placement rules, 6 × 6 footprint, grants no supply,
      is no drop-off, and once finished auto-fires the nearest enemy with
      range measured from its footprint rectangle — a corner Ghoul must not
      cost it three cells of reach. An unfinished site never fires. Fire is
      silent this phase (named gap in the close doc).
    - **Enemies are ordinary hard pairs.** The Ghoul takes the RTS hard-body
      contract as-is — ADR 021's gather-worker exception is not widened, so
      every enemy-touching pair is repaired, or counted by `body_overlaps`
      and reported. The horde keeps ADR 009's soft separation and may still
      overlap; never merge the two claims.
    - **Hundreds on the gate, horde later.** The gate scene carries 300–800
      enemies across a scripted run. The phase-3 claim — tens of thousands —
      is a different scale and a different slice; nothing here advances or
      spends it.

    Decisions:
    [ADR 022](ADR/022_ADR_combat_model_and_enemy_faction.md) and
    [ADR 023](ADR/023_ADR_combat_gate_scale_and_rebaseline.md). Shape of the
    slice: the [combat architecture](combat-prototype-architecture.html)
    page. What it proves and does not:
    [functional close](combat-prototype-functional-close.md).
    ```
  - [ ] 3.3 Checkpoint: `cargo test --locked --test validation_contract rts_overlap` and `cargo test --locked --test validation_contract every_doc_link_resolves` — green.
- [ ] 4. **`AGENT.md` — status paragraph + gate line + smoke sentence.**
  - [ ] 4.1 In `## Status`, after the final feedback-polish paragraph (ends `...: \`docs/rts-feedback-polish-functional-close.md\`.`, line 58 pre-edit), append a blank line and:

    ```md
    Phase 2 (combat prototype, branch `plan/combat-prototype`) is closed on
    functional scope: an enemy faction (`OWNER_ENEMY = 1`, melee
    `UnitKind::Ghoul`) spawns from scenario waves into the gate scene —
    hundreds across a run, not the phase-3 horde — marches on the HQ over
    one shared pooled flow field and melees the first player thing in range;
    Soldiers auto-acquire while Idle or attack-moving, plain Move never
    fires; damage is instant-hit `max(1, damage - armor)` with no projectile
    entities; units despawn, buildings (HQ included) are destructible with
    queue-cancel-no-refund, and a worker-built Turret (75 crystal, no
    supply) auto-fires — silently, a named gap. Enemies are ordinary hard
    bodies under the ADR 021 policy; the gather exception is not widened and
    the horde's soft separation is untouched. Sandbox — no win/lose: the
    outcome rides five exit tokens (`kills`, `losses`, `enemies_spawned`,
    `first_combat_tick`, `hq_alive`) pinned by the tracked
    `assets/scenarios/rts_combat_v1.script`, and both phase-1 scripts plus
    the scene sha256 were re-baselined once (the phase-1 close docs describe
    the pre-enemy runs). What it proves and does not:
    `docs/combat-prototype-functional-close.md`; decisions in ADR 022–023.
    ```
  - [ ] 4.2 In the fenced merge-gate block (lines 77–92 pre-edit), after the line `cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script` and before the closing ```` ``` ````, add T7's combat line **copied verbatim from `docs/05-testing.md`** (step 0.3): `cargo run -- rts --frames «N_COMBAT» --inject-input-file assets/scenarios/rts_combat_v1.script`. The two files must match byte-for-byte on this command.
  - [ ] 4.3 In the bullet starting `- The `rts` line is the interactive RTS smoke:` (line 94 pre-edit), insert before the final sentence (`Single source of truth for the gate: \`docs/05-testing.md\`.`): `The combat line is the phase-2 smoke: \`assets/scenarios/rts_combat_v1.script\` drives select → attack-move → auto-acquire → turret build → deaths, and asserts the combat tokens \`kills= losses= enemies_spawned= first_combat_tick= hq_alive=\` on the exit line.`
  - [ ] 4.4 Checkpoint: `cargo test --locked --test validation_contract` — full binary green (this file feeds both the overlap scan and, indirectly, none of the gate scans — `required_gate_keeps_phase_smokes` reads `docs/05-testing.md` + `README.md` only).
- [ ] 5. **`artifacts/manual_test_checklist.md` — human-only combat pass.**
  - [ ] 5.1 Append at end of file (after the T16 feedback-polish section, line 739):

    ```md

    ## T8 (combat-prototype) — Combat close: the human-only pass

    Run `cargo run -- rts` on the enemy-bearing gate scene and check what no
    offscreen test can prove:

    - [ ] **HP bars are visible and legible.** Damage a unit and select
          another: bars appear over damaged and selected entities only,
          vanish at full HP for unselected ones, and read clearly against
          the terrain at 1080p.
    - [ ] **The death flash is visible.** Kill a Ghoul and lose a worker:
          each death shows a brief ring flash at the death spot — noticeable
          in a melee, not just in isolation.
    - [ ] **Enemy dots are red on the minimap.** Marching Ghouls show as
          red dots distinct from player, building and node colours; a wave
          spawning at the far edge is visible on the minimap before it is
          visible on screen.
    - [ ] **Attack-arming resolves correctly — knowing there is no cursor
          visual.** Select soldiers, arm Attack from the card (or its
          positional key), then click an enemy: they attack it. Arm again
          and click open ground: they attack-move, engaging Ghouls met on
          the way. Nothing on screen marks the armed state — that is a
          named gap, not a defect; confirm only that the *next* click
          resolves as attack/attack-move and a right-click still cancels
          into normal orders.
    - [ ] **The turret visibly fires.** Build a Turret in the enemy approach
          path: once finished it engages Ghouls on screen, and its target
          dies without the turret ever moving. Confirm it is silent — no
          fire sound is expected this phase (named gap G1 in the close doc).
    - [ ] **An enemy answers a click.** Click a Ghoul: a read-only card
          shows its kind and HP; the drag box never picks it up.
    - [ ] **Hundreds read as hundreds.** Let a late wave land: the screen
          and minimap stay readable with hundreds of enemies marching — a
          judgment call, recorded here because no test makes it.
    ```
- [ ] 6. **`docs/GLOSSARY.md` — `## RTS (combat)` section.**
  - [ ] 6.1 Insert between the `## RTS (feedback polish)` table (ends line 70 pre-edit, the `gathertransition` row) and `## Render` (line 72 pre-edit):

    ```md

    ## RTS (combat)

    | word         | short description                                                    | ref in code                                                              |
    | ------------ | -------------------------------------------------------------------- | ------------------------------------------------------------------------ |
    | ghoul        | Melee enemy unit: 30 HP, damage 5, range 8                           | `crates/mmd-engine/src/rts/entity.rs`, `UnitKind::Ghoul`                 |
    | turret       | Static defense building, auto-fires, 75 crystal, no supply           | `crates/mmd-engine/src/rts/entity.rs`, `BuildingKind::Turret`            |
    | attackmove   | Move order that halts to fight anything met en route                 | `crates/mmd-engine/src/rts/orders.rs`, `Order::AttackMove`               |
    | autoacquire  | Idle or attack-moving armed unit fires at nearest target in range    | `crates/mmd-engine/src/rts/combat.rs`                                    |
    | wave         | Scenario-timed enemy spawn batch at one spawn point                  | `crates/mmd-engine/src/scenario.rs`, `struct WaveSpec`                   |
    | spawnpoint   | Map cell a wave's ghouls appear around                               | `crates/mmd-engine/src/scenario.rs`, `EnemySpec::spawn_points`           |
    | owner        | Faction byte: player 0, enemy 1, neutral 255                         | `crates/mmd-engine/src/rts/entity.rs`, `OWNER_ENEMY`                     |
    | combattokens | Exit-line combat outcome: kills, losses, enemies_spawned, first_combat_tick, hq_alive | `src/rts_run.rs`                                        |
    ```
  - [ ] 6.2 Checkpoint: `cargo test --locked --test validation_contract glossary` — green (polish rows untouched).
- [ ] 7. **Final validation + commit.** Run the Validation list below, then commit all six files in one commit.

## Outputs

- Files (exactly six, all docs):
  - `docs/combat-prototype-functional-close.md` (new)
  - `docs/CONTEXT.md` (roadmap item 2 status block)
  - `docs/DESIGN.md` (`## Combat` + phase-2 designs link)
  - `AGENT.md` (status paragraph, gate line, smoke sentence)
  - `artifacts/manual_test_checklist.md` (combat human-only section)
  - `docs/GLOSSARY.md` (`## RTS (combat)` section)
- Behavior: none. No code, no assets, no scripts, no schemas.

## Validation

- [ ] `cargo test --locked --test validation_contract` → all tests green (covers `no_perf_claim_in_docs`, `every_doc_link_resolves`, `adr_index_lists_every_adr_file`, `rts_overlap_invariant_names_its_gather_exception`, `glossary_defines_the_feedback_polish_vocabulary`, `manual_checklist_covers_every_human_only_flow`, `required_gate_keeps_phase_smokes`)
- [ ] `cargo test --workspace --locked` → green (docs-only change; a new red is a defect to report, not to patch here)
- [ ] `grep -n 'rts_combat_v1.script' AGENT.md docs/05-testing.md` → both hits carry the identical command string
- [ ] `grep -rn '«' docs/ AGENT.md artifacts/manual_test_checklist.md` → zero hits (every placeholder resolved)
- [ ] Manual link walk in rendered markdown: the close doc's links to `05-testing.md`, ADR 022, ADR 023, the architecture page and the three prior closes all open; the CONTEXT block's four links open
- [ ] `git status --short` → exactly the six files above modified/added, nothing staged beyond them
- [ ] commit msg draft: `docs: close combat prototype on functional scope`
