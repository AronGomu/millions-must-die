# ADR 017: RTS hard collision, radius-aware navigation, and formations

- Status: Accepted
- Date: 2026-08-10
- Accepted: 2026-08-12 (T18, on landed phase-1.1 evidence)
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

## Amendment (T3, T5, T6 implementation)

Three further corrections to the decision text above, all forced by landed
code and all narrower than what they replace.

1. **Interaction reach is adaptive, not fixed.** "Reach = body radius +
   0.5-cell tolerance" is a floor, not the rule. Dense inflated terrain can
   push *every* legal ring cell past that flat distance, and a unit routed to
   such a cell would then never finish its order. The landed rule is
   `adaptive_reach(kind, chosen_cell_dist) = max(interaction_reach(kind),
   chosen_cell_dist + NAV_CENTER_TOLERANCE_CELLS)` in
   `crates/mmd-engine/src/rts/orders.rs`, where `interaction_reach` is still
   `body_radius + NAV_CENTER_TOLERANCE_CELLS` (3.0 + 0.5) and
   `entity_approach_cell` returns `(Cell, distance)` so the chosen cell's own
   rect distance is available to widen it. Workers still never enter a target
   solid: the reach widens toward the cell the router actually picked, it does
   not move the cell.
2. **Arrival is slot-based.** The RTS `ARRIVAL_RADIUS_CELLS` constant is gone.
   A unit stops when it is within `FORMATION_ARRIVAL_CELLS = 0.25` of *its own*
   formation slot centre (`crates/mmd-engine/src/rts/formation.rs`,
   `arrival_is_measured_from_the_slot_centre`). A shared radius around a shared
   destination cannot express "six units arrived" once each of them owns a
   distinct slot. The horde's `sim::ARRIVAL_RADIUS` is untouched.
3. **Production drains through `ProductionQueue` self-methods.** The queue owns
   `tick_head`, `head_ready` and `pop_ready`; the world asks the queue, and
   only pops when a legal body position exists. There is no EntityId-keyed
   production API: the queue is per-building state in `ProductionTable`, and
   keying the drain by entity would have put the wait/resume decision outside
   the type that holds the payment (`production_waits_when_no_spawn_is_free`,
   `waiting_production_resumes_once`).

### Known defect, pinned not fixed

`StaticNav::center_blocked` marks a 1–2 cell diagonal channel of the tracked
scene (y ≈ 172..174, x ≈ 190..199) as legal centres, but no step through it
survives `sweep_clear` + `step_admissible`: the field claims the channel is
reachable and a 3-cell body wedges in it. The two tests disagree by
construction — the centre mask is a per-cell circle test, the sweep is
continuous — and reconciling them is a navigation change, not a docs one. It
is pinned as an `#[ignore]`d reproducer,
`rts_nav_staleness::a_body_wedges_in_the_narrow_eastern_channel`, and carried
forward in the phase-1.1 close's known gaps.
