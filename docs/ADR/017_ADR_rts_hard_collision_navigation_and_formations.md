# ADR 017: RTS hard collision, radius-aware navigation, and formations

- Status: Proposed
- Date: 2026-08-10
- Supplements: [ADR 013](013_ADR_phase1_scope_and_rts_entity_model.md), [ADR 015](015_ADR_economy_construction_and_production_determinism.md)
- Scoped contrast: [ADR 009](009_ADR_agent_separation_and_collision.md) remains authoritative for horde `sim/`
- Plan: `ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`

## Context

RTS movement currently commits each unit independently. Units merge forever. Scenario collision radius only drives pick/ring geometry; `rts/` has no collision pass.

User requires hard non-overlap for every current/future RTS-world unit owner, radius 3 cells, including clearance from terrain, nodes, finished buildings, static obstacles, map edges.

This reverses an old project-wide shorthand in `AGENT.md` for player units. It does **not** reverse ADR 009's horde choice: dense 5,000-agent `sim/` remains soft-separation/overlap-capable.

## Decision

### Body source

```rust
UnitKind::body_radius_cells() -> f32
```

Worker + Soldier return 3.0. Contact distance=6.0. Future enum variants must decide via exhaustive match. RTS movement never reads horde `Scenario::collision_radius_q8`.

Touching accepted (`distance >= sum radii`). Penetration strict `<`. No epsilon claim.

### Static navigation

`RtsWorld` owns `StaticNav`:

- raw solids: terrain, resource 1×1 footprints, finished buildings;
- unfinished sites excluded until completion;
- center-blocked mask = positions where 3-cell circle intersects solid/map edge;
- pooled fields rebuild over center-blocked mask;
- exact continuous sweep guards every candidate step.

Placement keeps raw rules. Body-inflated nav must not enlarge building-placement exclusion.

Field scratch reserves worst-case heap pushes at load. Cold field miss allocates nothing.

### Interaction reach

Workers never enter target solids. Gather/build/drop-off reach = body radius + 0.5-cell nav-center tolerance, measured from target footprint rectangle. Routes target deterministic legal approach centers, never blocked target center.

### Unit collision

Movement is rotated sequential proposal/commit:

1. collect all live RTS units ascending;
2. rotate start by tick index;
3. derive candidate from shared flow/terminal slot;
4. sweep against static world + all unit current/final positions;
5. accept whole candidate or stay.

Earlier units expose final positions; later units old positions. Induction: valid start → valid end. Sweep point-to-segment test prevents tunneling at 30/24 cells/s.

Implementation intentionally uses O(U²) checks first. Proof simplicity wins; no perf threshold/claim.

### Formation without per-unit pathfinding

One group order acquires one pooled anchor field. Each member gets distinct deterministic 6-cell lattice slot around anchor.

- IDs canonical ascending.
- Candidate slots Chebyshev rings, row-major ties.
- Slot body-clear, reachable in anchor field, anchor→slot static sweep-clear, unreserved.
- Atomic reject if insufficient slots.
- Outside terminal capture: descend shared field.
- Inside: direct slot vector; on rejection, shared-field fallback.

Bounded terminal steering is local formation placement, not A*/waypoint pathfinding. `NAV_FIELD_SLOTS=8` stands.

Rotated movement priority is choke fairness. No random/choke detector.

### Spawn and world transitions

- Scenario spawn: deterministic nearest legal cell; squared distance then flat index.
- Production: ready head spawns at nearest legal position; no space → head stays ready/paid/reserved.
- Site completion: atomically preplan all body evacuations before solid stamp. Failure → site stays final pre-complete tick/walkable.
- New spawns participate in same tick collision sweep.

Raw store mutation becomes crate-private/testkit-only.

## Consequences

- Current six adjacent workers relocate on load; tracked script pixels change.
- 5-cell corridors become unreachable for 3-cell body.
- Resource nodes become solids; gather route/reach changes.
- Group order/state hash includes anchor + slot.
- Production can visibly wait at 100% when exit blocked.
- Construction can visibly wait when site cannot evacuate.
- Hard invariant is RTS-only. Docs must always name scope.

## Rejected

- Soft separation: cannot guarantee no merge.
- Iterative push: bounded passes cannot prove no penetration.
- Occupancy-stamp units into `FieldPool`: invalidates fields every tick.
- Unique field per formation slot: de facto per-unit pathfinding/8-slot thrash.
- Spawn then resolve: temporarily violates hard invariant.
- Per-unit A*: violates engine rule.

## Validation contract

Planned tests prove exact contact, head-on/crossing no penetration, all owners/idle bodies, static sweep, edge/corridor rules, one field/group, distinct deterministic slots, fair choke progress, blocked production/completion wait, reproducible hashes, zero per-frame alloc, no `sim/` diff.

## Amendment (T4 implementation)

Implemented literally — candidate accepted whole or the mover stands still —
this froze the game: a pooled flow field is body-blind, so a unit whose descent
points at a stationary body is stuck for good, and the tracked scene seeds
three workers in a row exactly one body diameter apart. 26 tests, including the
merge-gate acceptance run, showed it. Three bounded mechanisms were added on
top of the rotated sequential proposal/commit above:

1. **Push-aside.** A mover displaces the bodies its candidate touches, along
   their contact normals.
2. **Bounded push chain.** A displaced body may in turn displace what it would
   land on, to `MAX_PUSH_DEPTH = 3` links and `MAX_PUSHED_BODIES = 8` bodies,
   iteratively — never recursively. Each body moves at most once per tick.
3. **Deflection fallback.** Only after a push chain is rejected, the mover
   tries its descent rotated `-45°, +45°, -90°, +90°` (fixed order, first legal
   wins). Needed because a body pinned against a building's static clearance
   cannot legally be shoved in any direction.

This narrows, but does not reverse, the "iterative push" entry under
**Rejected** above: what is rejected there is *relaxation* — pushing bodies
apart in bounded passes and hoping the result is separated. Nothing here
relaxes. A push is committed only when every displaced body's final position is
already proven legal against the static world, the mover's candidate, every
non-displaced body and every other displaced body; one illegal link rejects the
mover's whole step and nothing moves. The invariant, its induction and the
no-epsilon rule are unchanged: no completed tick leaves two RTS unit bodies
merged.
