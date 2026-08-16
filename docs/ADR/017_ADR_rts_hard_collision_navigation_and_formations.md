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
- **Every one of those relocations is confined to one connected region of legal
  body centres.** Raw Euclidean distance alone would pick the legal centre one
  cell across a wall whenever it is nearer than anything on the body's own
  side, teleporting an evacuated, produced or overlap-repaired unit into a
  pocket it could never have walked to. `StaticNav` therefore labels
  connected components of `center_blocked` on every mask rebuild, using the
  same 8-neighbour, no-corner-cut rule the pooled fields integrate with, so
  "same component" means exactly what "reachable" means to a walk — answered in
  O(1) instead of by building a field per relocation. A body standing where no
  body may legally stand (a building finished on top of it) has no component of
  its own; the anchor is then the region holding the nearest legal centre to
  it, which is its own pocket.

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

### Known defect, pinned not fixed: a parked body plugs a single-file corridor

An earlier revision of this ADR blamed a "nav channel the centre mask and the
sweep disagree about". That was wrong, and is corrected here. `center_blocked`
and `sweep_clear` **agree**: over every free cell of the region that was
accused (x 188..201, y 170..177 on the tracked scene), every centre-to-centre
step between two free neighbours is swept clear, and a body walks the same
route to completion when no other body is standing in it.

The real defect is in **this ADR's own push rule**. A finished Depot at
`(180, 176)` and the crystal node at `(196, 178)` inflate toward each other
until the only legal body centres between them are the two columns `x = 191`
and `x = 192`. Two 3-cell-radius bodies cannot stand six cells apart in two
columns, so that corridor is single-file — which is fine — and one idle body
parked in it blocks every other body **for good**, which is not:

- the mover's candidate passes `step_admissible` and `sweep_clear`, and is
  refused only by `body_sweep_hit`;
- `try_push_chain` cannot rescue it, because the parked body's contact-normal
  push target lands on a `center_blocked` cell inside the Depot's clearance,
  and the chain is all-or-nothing;
- the mover then takes its south-east `MOVE_DEFLECTIONS` entry, and the field
  vector at the deflected cell points back south-west, so it ping-pongs
  between two cells forever holding a live order.

This is a regression of phase 1.1 — hard 3-cell bodies are new here, and a
point-sized phase-1 unit walked straight past — owned by the collision/push
rule, **not** by the navigation mask. It is pinned by a positive, un-ignored
test, `rts_nav_staleness::a_parked_body_plugs_the_single_file_depot_corridor`,
whose second half walks the identical route with the corridor clear and
asserts arrival.

The deferred fix, prototyped and known to resolve both reproducers: when the
contact-normal push target is statically illegal, retry the whole chain
pushing along the *mover's own heading* (exact circle-exit solve, same
all-or-nothing legality gate). It is deferred because it perturbs unit
positions across the tracked 1,600-frame acceptance run and therefore re-bases
that script and its contracted exit line — a ticket of its own, not a
review-fix. Nothing on the tracked acceptance path routes through that
corridor.

## Amendment (F10: the overlap repair pass is testkit-only)

The movement system's phase 2 — "repair any penetration the world was handed" —
ran on **every** tick, scanning every ordered pair of live unit bodies before a
single unit moved. It was unreachable work by this ADR's own rules: seeding,
movement, the push chain, production placement and completion evacuation all
place bodies body-safely, and the decision above already states that raw store
mutation is crate-private/testkit-only. The only two mutations that can hand
the world a merged pair are `RtsWorld::force_position_for_test` and
`RtsWorld::entities_mut`, and neither exists without the `testkit` feature.

The pass is therefore `#[cfg(feature = "testkit")]` and, inside a testkit
build, runs only on ticks where one of those two hooks armed it
(`repair_armed`). Arming is sticky until a pass ends clean, so an overlap the
grid cannot repair still stashes `TickError::UnrepairableOverlap` and is
re-reported every tick instead of going quiet after one. `last_tick_error` is
now cleared at the top of `movement` rather than inside the pass, so it still
describes the last tick and never an older one in a build that has no pass at
all; in a shipping build it is always `None`.

What this does not change: the hard-body invariant, its induction, the
no-epsilon rule, the arrival check's own `body_penetrates_any` call (a
per-mover check inside `step_one_unit`, not a per-tick pair scan), and every
valid-state hash — a repair-capable run and a repair-disabled run of the same
contended world hash identically
(`rts_collision::arming_overlap_repair_on_a_valid_world_changes_no_state`).

The claim is pinned by deterministic work counts, never timings:
`RtsWorld::overlap_repair_runs()` counts executed passes, and the seed,
movement, production and construction runs assert it stays `0`
(`rts_collision::movement_never_runs_overlap_repair`,
`rts_production::production_never_runs_overlap_repair`,
`rts_build::a_completion_never_runs_overlap_repair`), while
`rts_collision::forced_overlap_is_repaired` and
`rts_collision::an_unrepairable_overlap_is_reported_on_every_tick` keep the
forced-overlap behaviour green.

One further arming source, added when this amendment met the one below: a
gather exit that spends its whole bound and finds nowhere to relocate
(`separate_exiting_pairs` → `relocate_stuck_pair`) hands the world exactly the
thing this pass exists to clear — a hard merged pair — so it arms the pass too,
under the same `#[cfg(feature = "testkit")]`. That is a *failure* path, not a
shipping one: no normal tick reaches it, so the `overlap_repair_runs() == 0`
invariants above are untouched. In a shipping build, which compiles no pass at
all, the failure is still reported the tick it happens
(`TickError::UnrepairableOverlap`) and stays counted by `body_overlaps` for as
long as it lasts — the exit line is the shipping-visible signal, and it does not
go quiet.

## Amendment 2026-08-15 — the invariant is no longer unconditional

Superseded in part by
[ADR 021](021_ADR_rts_feedback_polish_and_gather_collision.md). The decision
above stands as written for every pair it still covers; what changed is its
scope, so it is amended here rather than rewritten.

The unconditional form — *no completed tick leaves two RTS unit bodies merged*
— is now narrowed to:

> Every penetrating RTS-unit pair after a tick is either both active gather
> workers, or the exact remembered pair inside its bounded 12-attempt
> gather-exit transition. Every other pair is non-penetrating, or is counted
> by `body_overlaps` and reported through `TickError::UnrepairableOverlap`.

Everything else in this record is unchanged: static terrain, map edges, nodes
and finished buildings stay hard for every unit; a pair with a non-worker or
non-gathering member stays hard; formation slots, spawn, production,
construction evacuation and placement relocation stay all-body-free. The
parked single-file corridor defect below is still open and still out of scope.
