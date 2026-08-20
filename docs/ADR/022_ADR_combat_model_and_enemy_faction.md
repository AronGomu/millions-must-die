# ADR 022: Combat model and enemy faction

- Status: Accepted
- Date: 2026-08-17
- Supplements: [ADR 013](013_ADR_phase1_scope_and_rts_entity_model.md), [ADR 015](015_ADR_economy_construction_and_production_determinism.md), [ADR 017](017_ADR_rts_hard_collision_navigation_and_formations.md), [ADR 021](021_ADR_rts_feedback_polish_and_gather_collision.md)

## Context

Phase 2 needs weapons, damage, turrets, enemy AI. Two candidate homes for enemies existed: the frozen horde `sim/` (soft separation, fixed population walker, pinned phase-0 hash) or the RTS `EntityStore` (hard bodies, orders, state hash). Combat depth had a fork too: projectiles vs instant-hit, damage types vs flat stats. Balance pass is explicitly a later phase, so every stat is placeholder.

## Decision

### Enemies are RTS entities

New owner `OWNER_ENEMY = 1` beside `OWNER_PLAYER = 0` / `OWNER_NEUTRAL = 255`. One new melee kind `UnitKind::Ghoul = 2` (tag `0x12`, discriminant appended). Enemies live in the same `EntityStore`, use the same hard-body movement (proposal/commit, push chains), descend the same pooled flow fields. They are **ordinary hard pairs** — ADR 021's gather exception is not widened. Enemies never gather, build, produce, or enter `Supply::used` (recount filters `owner == OWNER_PLAYER`). Horde `sim/` untouched; hundreds here claim nothing about phase 3's tens of thousands.

### Instant-hit on cooldown, flat stats

No projectile entities. An attack resolves the same tick: target in range + cooldown 0 → `apply_damage`, cooldown resets. Damage per hit = `max(1, damage - armor)`. Flat HP (u32 store column), armor as a kind table (pure function of kind, not a column), integer cooldown ticks. No damage types, regen, crits. Placeholder stats: Worker 25 HP/0 armor unarmed; Soldier 40/0, dmg 6, cd 15, range 24; Ghoul 30/0, dmg 5, cd 30, range 8, speed 18 c/s; Turret 150/1, dmg 10, cd 20, range 36; Hq 400/2; Depot 150/1; Barracks 200/1.

### Death

`DamageResult` = `Damaged | Killed | Indestructible | NoTarget`; resource nodes indestructible. Units despawn, no corpse. Buildings destructible **including the HQ**: footprint un-stamped (invalidates every cached field, same rule as stamping), production queue cancels with no refund, supply grant revoked via existing `Supply::revoke_cap`, HQ death clears `start_hq` and idles gatherers needing drop-off. Selection prune stays last in the tick. HP column, cooldown column and kill/loss counters enter `state_hash` fixed-width.

### Orders and targeting

Two appended `Order` variants: `Attack { target, field }` (tag 4), `AttackMove { goal, field }` (tag 5). Firing rules: `Idle` and `AttackMove` auto-acquire and fire in place; `Attack` closes distance then fires; `Move`/`Gather`/`Build` never fire. Target = nearest valid enemy-of-owner in range, lowest-slot tie-break, ascending-slot iteration. Range to units = center distance − body radius; to buildings = distance to footprint rect. Turret joins as a static firer via `building_weapon(kind)`; an unfinished site never fires.

### Enemy AI

One rule: march on the objective, attack the first player thing in weapon range. Objective = **approach cell** of the nearest player building (a pooled field cannot target a blocked footprint cell), starting at the HQ; HQ dead → nearest remaining player building, lowest-slot tie-break; none → Idle. Objective cached, recomputed on death events only. Every idle enemy gets `AttackMove` at the objective — hundreds of enemies share one pooled field. No per-enemy pathfinding, `NAV_FIELD_SLOTS = 8` stands.

### Player surface

Commands `cmd_attack_target` / `cmd_attack_move` / `cmd_stop`; A = card slot 3, Stop = slot 4 (positional keys, zero new script tokens). Right-click enemy = attack. Attack accepts voice as `VoiceCue::Move`, rejects as `voice_reject` — no new bus. Enemy click-select is read-only (two-line card). HP bars = texture-free overlay line instances over damaged ∪ selected entities; death flash = procedural ring, 12 frames; minimap enemy dots red. None of it enters the world hash.

## Consequences

- Combat is deterministic, clock-free, allocation-free — testable through `RtsHarness` like every other system.
- Enemy scale is capped by `MAX_ENTITIES = 2048`; the scenario validator caps total enemies at 1200.
- Turret fire is silent and unanimated this phase — named gap, not an accident.
- A future ranged enemy, worker attack, or damage-type matrix appends kinds/stats without moving hashes retroactively (discriminants append-only).
