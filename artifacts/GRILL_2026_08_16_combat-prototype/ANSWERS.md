# Grill: combat-prototype

## Round 1 — All branches

| #   | Question | Answer | Precision |
| --- | -------- | ------ | --------- |
| 1   | What is an enemy, in code? | RTS entities with an enemy owner id | — |
| 2   | How does damage travel? | Instant-hit on cooldown | — |
| 3   | Who fights in this slice? | Soldier (ranged) + Turret vs one new melee enemy kind | — |
| 4   | How deep is the damage model? | Flat HP + flat damage + integer cooldown **+ flat armor subtracted per hit** | — |
| 5   | What is the turret? | New `BuildingKind::Turret`: worker-built, crystal cost, no supply, auto-fires nearest enemy in range | — |
| 6   | What does enemy AI do? | March on the HQ; attack the first player thing in weapon range | — |
| 7   | How do enemies enter? | Scenario-declared: pre-placed group + timed waves from spawn points | — |
| 8   | Combat commands? | Attack (A: click enemy = target, click ground = attack-move) + Stop + right-click enemy attacks | — |
| 9   | 0 HP? | Units despawn; buildings destructible **including the HQ**, footprint un-stamped on death | — |
| 10  | Defeat/victory? | Sandbox: no win/lose screen; kills and losses counted on the exit line | — |
| 11  | HP display? | Bar over damaged + selected entities, hidden at full HP | — |
| 12  | Merge gate home? | **Extend existing `rts_prototype_v1` scene** with enemies, add a new script | — |

## Round 2 — Gate compatibility, targeting, scale

| #   | Question | Answer | Precision |
| --- | -------- | ------ | --------- |
| 1   | Enemies vs exact-count phase-1 scripts? | Accept early contact; **re-baseline both old scripts' asserted counts + scene sha256 once** | Spawn enemies outside direct contact range — combat must begin by marching into range, so combat *detection* is provably correct |
| 2   | Auto-acquire? | Idle + attack-moving soldiers auto-acquire nearest enemy in range; plain Move never fires | — |
| 3   | Enemy scale? | **Hundreds** — waves totalling 300–800 across the run | — |

## Facts (scout)

- `assets/scenarios/rts_prototype_v1.ron` has a `.sha256` sidecar, verified at load (`crates/mmd-engine/src/scenario.rs:254`) — editing the scene means regenerating the sidecar. Phase-1 artifact, not phase-0 frozen.
- `tests/rts_acceptance.rs` asserts **exact** exit-line counts (`music_starts=1`, `voice_select=8`, …) for both tracked scripts over their 1600-frame runs; the 64-hex `hash` field is parsed and used for determinism comparison, not pinned as a constant in the test.
- `RtsWorld::tick` pinned system order (`world.rs:1730`): commands, camera, construction, production, orders/gather, movement, supply recount, selection prune. Combat inserts as a new numbered system.
- `EntityStore`: `MAX_ENTITIES = 2048`, `OWNER_PLAYER = 0`, `OWNER_NEUTRAL = 255`; `EntityKind::tag()` reserves nibble space; discriminants appended, never inserted.
- `UnitKind::body_radius_cells()` exhaustive match — a new enemy kind must declare its own radius.

## Shared understanding

- **Goal:** Phase 2 Combat Prototype — weapons, damage, turrets, enemy AI — as a vertical slice on the existing RTS world, closing with a scripted end-to-end combat gate line.
- **Settled:**
  - Enemies are RTS entities in `EntityStore` with a new enemy owner id — hard bodies, pooled flow fields, state hash. Horde `sim/` untouched, stays frozen.
  - One new melee enemy `UnitKind`; combatants this slice: Soldier (ranged, instant-hit), `BuildingKind::Turret` (worker-built, crystal cost, no supply, auto-fires nearest enemy in range), melee enemy.
  - Damage: instant-hit on cooldown; flat HP, flat damage, flat armor subtracted per hit, integer cooldown ticks. No damage types, no regen, no crits.
  - Enemy AI: march on the HQ down one shared pooled flow field; attack the first player thing in weapon range. No per-enemy pathfinding.
  - Enemy entry: scenario-declared — pre-placed group + timed waves from spawn points, spawned outside direct contact range so combat provably begins by marching into range. Total across run: **hundreds (300–800)**.
  - Commands: Attack (A: click enemy = target, click ground = attack-move), Stop, right-click enemy = attack. Idle + attack-moving soldiers auto-acquire nearest in range; plain Move never fires.
  - Death: units despawn; buildings destructible **including HQ**; footprint un-stamped on death (invalidates cached fields). No corpse state.
  - No win/lose screen — sandbox; kills/losses counted on the exit line.
  - HP bar over damaged + selected entities, hidden at full HP, texture-free overlay primitive.
  - Gate: extend `rts_prototype_v1.ron` (regen sha256), add new tracked combat script + exit-line combat tokens; **re-baseline both existing scripts' asserted counts once** since contact may land inside their 1600-frame windows.
- **Assumptions (mine, logged not asked):**
  - Enemy owner id: `OWNER_ENEMY = 1`.
  - Enemies consume no supply and cost nothing; they never gather, build, or produce.
  - Stat magnitudes are placeholders chosen at plan time (balance pass out of scope, phase 5).
  - Turret has no tech prerequisite; placed under the existing four placement rules; footprint from scenario constant.
  - Building dies → its production queue cancels, no refund (SC1 behavior); supply recount handles reservations automatically next tick.
  - HQ dies → gather orders needing drop-off go `Idle`; play continues.
  - HP bars apply to buildings under the same damaged/selected rule.
  - Waves are a finite scenario list; no endless spawner this phase.
  - Melee enemy shares `RTS_UNIT_BODY_RADIUS_CELLS = 3.0` unless collision at hundreds-scale forces a decision, which would surface in the plan.
- **Out of scope:** horde-scale enemies (phase 3), walls/multiple entrances (phase 4), balance (phase 5), missions/win-lose framework (phase 6), zoom, fog of war, save/load, performance claims of any kind.
