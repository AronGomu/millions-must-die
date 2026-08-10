# ADR 013: Phase-1 scope and the RTS entity model

- Status: Accepted
- Date: 2026-08-10
- Supplements: [ADR 003](003_ADR_simulation_and_flow_field.md) — the horde
  simulation is unchanged; this adds a *second*, separate entity store beside it.
- Plan: `ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`

## Context

Phase 0 closed on functional scope: a 5 000-agent horde walks a flow field and
renders. Phase 1 in `docs/CONTEXT.md` is "RTS Engine Prototype — camera,
selection, workers, economy, building, unit production". Six systems, none of
which exists.

Two questions had to be settled before any code: how deep to cut, and where the
new entities live.

Depth was chosen with the user: a **thin vertical slice through all six**. One
worker type, two resources, three buildings, one produced unit. Every system
present end to end, none of them deep. The alternative — build the control layer
now and the economy later — was rejected because it would leave the phase's
actual question ("can this engine do RTS?") unanswered for another phase.

## Decision

**(a) The phase-1 scene is horde-free.** New scenario family
`rts_prototype_v1`, 320 × 320 cells, `hard_agent_count: 0` and
`stretch_agent_count: 0` — required to be exactly zero by its validator, not
merely permitted. Combat is phase 2; a zombie that cannot be fought is cost with
no payoff. The phase-0 gate scene keeps running untouched, and
`run --agents 5000 --frames 300` stays in the merge gate with its state hash
unchanged.

**(b) The scenario contract grows an optional block, not a new file format.**
`ScenarioSpec` gains `#[serde(default)] pub rts: Option<RtsSpec>`. The
`serde(default)` is the whole decision: without it, adding a required field
would invalidate five committed scenes and four fixtures at once, forcing a mass
regeneration of tracked `.ron` bytes and their `.sha256` sidecars. With it,
every phase-0 asset keeps its exact bytes. Presence is then validated as an
exclusive-or: the RTS family must carry the block, every other family must not.

**(c) RTS entities live in their own preallocated SoA store, not in
`Simulation`.** `sim::Simulation` is a fixed-population horde walker with one
destination and a recycle cursor; player units have per-unit orders, cargo,
construction progress and production queues. Bolting those onto it would put
RTS state inside the type every pinned phase-0 hash digests. `rts::EntityStore`
is a separate 2 048-slot store with generational ids, a LIFO free list, and
every column reserved at construction. `2 048` is derived, not guessed: the
supply cap tops out at 500 and the cheapest unit costs one supply, so at most
500 units can exist, plus buildings and the scene's nodes.

**(d) Generational ids, and a stale handle resolves to `None`.** A despawned
slot is reused; the generation counter is what stops an old `EntityId` from
silently naming the newcomer. `Selection::retain_live` checks
`store.contains(id)`, not `store.alive(slot)`, precisely so a reused slot does
not resurrect a stale selection.

**(e) Player units path on a pooled flow field, never per-unit.** The engine
rule "no per-enemy pathfinding" is extended to player units: `nav::FieldPool`
holds `NAV_FIELD_SLOTS = 8` preallocated fields keyed by destination cell,
LRU-evicted, ties to the lowest slot. Units sharing a destination share a field,
which is group cohesion for free. `FlowField::rebuild_in_place` reuses the
field's own buffers plus a caller-owned `FieldScratch` heap, so a rebuild
allocates nothing after warmup. Per-unit A* was rejected: it breaks the rule,
needs a per-tick request budget, and allocates unless a path arena is built
first — three new problems to solve a problem the existing tech already solves.

**(f) `RtsWorld::tick` has a fixed, documented system order:** commands, camera,
construction, production, orders, movement, supply recount, selection self-heal.
Written into the type's doc comment so a reordering is a visible diff rather
than an accident. Two orderings are load-bearing and stated as such: the gather
system runs **before** movement, so a worker that arrives starts mining the same
tick; and construction runs **before** production, so a Barracks that finishes
this tick can already hold a queue.

**(g) Everything is hashed.** `RtsWorld::state_hash` digests the tick index,
every live entity slot in ascending order, the order table, the selection, the
production table, resources, supply and the camera centre — `f32` as raw bits,
matching `Simulation::state_hash`. The camera is world state on purpose: a
replay that ends looking somewhere else did not reproduce.

## Consequences

- Two entity models coexist. That is the cost of not perturbing phase 0, and it
  is paid once: phase 2 combat will have to decide whether the horde moves into
  `rts::EntityStore` or stays where it is. This ADR does not pre-empt that.
- `sim/tick.rs` gains exactly one edit in this phase — `dir_from_vector` becomes
  `pub` so the RTS mover can share it. No behaviour changes; the horde's state
  hash is asserted unchanged at every ticket.
- `rts::orders::step_admissible` is a **deliberate duplicate** of
  `sim::tick::step_admissible`, pinned by a test that runs both over an
  exhaustive 5 × 5 grid with every obstacle subset of a 3 × 3 core. Sharing one
  function would mean making a hot private helper public across a module
  boundary that otherwise stays sealed; duplicating it and proving equality
  costs less than that coupling, and the proof is what keeps it honest.
- The zero-allocation invariant is restated, not weakened. The movement, systems
  and pack portion of an RTS tick allocates nothing after warmup, enforced by
  new cases in `crates/mmd-engine/tests/frame_allocations.rs`. **One documented
  exception:** a flow-field *miss* rebuilds into the reused scratch heap and may
  grow it once, at the grid's size. Tested in both directions — a cached
  acquire never grows it, and cycling 32 fresh destinations after a warm 8 does
  not grow it either.
- The 8-slot pool can thrash if a player somehow keeps more than 8 distinct live
  destinations. Thrashing stays *correct* — every miss rebuilds — but nothing
  asserts it stays cheap, and nothing can, because no performance number may
  gate a merge in this repo.
- `Supply::used` is recomputed every tick from live units plus queue
  reservations rather than incremented at five call sites. See
  [ADR 015](015_ADR_economy_construction_and_production_determinism.md).
- No performance claim is made or measured anywhere in phase 1, exactly as in
  phase 0. `no_perf_claim_in_docs` covers the new documents.
