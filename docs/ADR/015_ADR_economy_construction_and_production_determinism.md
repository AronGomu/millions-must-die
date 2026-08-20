# ADR 015: Economy, construction and production determinism

- Status: Accepted
- Date: 2026-08-10
- Supplements: [ADR 013](013_ADR_phase1_scope_and_rts_entity_model.md)
- Plan: `artifacts/PLAN_2026_08_10_rts-engine-prototype.md`

## Context

The economy is where an RTS prototype either becomes a game or becomes a demo.
Phase 1 ships two resources, a worker round trip, three buildings and two
producible units. Every one of those is a state machine that runs 60 times a
second, is compared against a recorded hash, and must not allocate.

The design pillars that bind here: **"500 unit population cap"** and
**"population growth as an important mechanic"** (`docs/DESIGN.md`). Supply is
what turns a number into a mechanic.

## Decision

**(a) Two resources and a supply cap.** Crystal and Gas, harvested by the same
worker with the same loop; the only differences are which counter rises and how
much a node holds (`1 500` / `2 500`). Supply starts at the scene's
`start_supply_cap` and is bounded by `scenario::MAX_SUPPLY_CAP = 500` — the
design pillar, enforced in `Supply::new` and `Supply::grant_cap` by clamping,
not by a caller remembering.

**(b) The worker round trip is a three-phase order, not a set of flags.**
`Order::Gather { node, phase }` where phase is `ToNode`, `Mining { ticks_left }`
or `Returning { drop_off, field_slot }`. One value, hashed, so the whole loop is
pinned by `state_hash` rather than by a test poking at booleans. A worker fills
`WORKER_CARRY_CAPACITY = 8` in `GATHER_TICKS = 60` — one second — so the round
trip's length is dominated by *walking*, which is the macro decision the economy
exists to pose: put a drop-off closer.

Two reach rules, and they are different on purpose. A node is reached at
`GATHER_REACH_CELLS = 2.0` from its **centre** — a node is one cell. A drop-off
is reached at `DROP_OFF_REACH_CELLS = 1.0` from its **footprint rectangle** — an
HQ is 12 cells across, and a centre-distance rule would make a worker walk into
the middle of its own base to deliver.

**(c) Grid placement is validated in four ordered rules, and units are not one
of them.** In-bounds, then terrain (which also carries every finished building,
because a finished building is stamped into the navigation mask), then overlap
with a *site* (which is not stamped), then resource nodes. A unit standing where
you want a Depot is **not** a reason to refuse: the genre does not do that, and a
rule depending on a moving unit makes placement validity flicker frame to frame.

**(d) A site is walkable; a finished building is not.** The footprint is stamped
into `FieldPool::set_blocked` at **completion**, not at placement — that is what
lets the builder stand inside the plot while it works. Stamping invalidates
**every** cached flow field, not the ones that "look affected": a single new
obstacle can change the descent vector anywhere downstream, and a partial
invalidation is a bug that surfaces as units walking into a wall built ten
seconds ago.

Because a finished building's own cells are blocked, a flow field cannot target
them. `building_approach_cell` walks the one-cell ring around the footprint in a
**fixed** order — top edge left→right, right edge top→bottom, bottom edge
right→left, left edge bottom→top — and returns the first unblocked cell. Fixed
order rather than "nearest" because two equally near cells would make the choice
depend on a float comparison, and the state hash would stop surviving a
refactor.

**(e) Construction is attended, and extra workers do not help.** A site advances
by exactly one tick per tick while at least one worker is within
`BUILD_REACH_CELLS = 1.5` of its footprint, and by nothing otherwise. A second
attendee changes nothing; `EXTRA_BUILDERS_SPEED_UP = false` is a real constant
so the question reads as asked-and-deferred rather than unconsidered. Additive
build speed is a balance knob, and phase 1 is not a balance pass.

Cancelling a site refunds the **full** cost. Partial refunds are a balance
decision; a full refund is the one that cannot be accidentally unfair.

**(f) Supply is reserved at enqueue, not at completion.** This is the load-bearing
choice of the whole production system. Charging supply when a unit pops would
let a player queue five Soldiers into two free supply and get all five — the cap
would be decoration. `enqueue_unit` checks supply, *then* debits resources, then
pushes, then reserves; the order matters, because a supply-blocked enqueue must
never take the player's money.

**(g) `Supply::used` is recomputed every tick, never incremented.** Step 7 of
the tick sums `supply_cost` over every live unit and adds
`reserved_supply()`. An incremental counter needs adjusting at five call sites —
spawn, despawn, enqueue, cancel, store-full recovery — and one missed site is a
slow drift the player only notices when production mysteriously stops. Recounting
is a linear pass over one `Vec<EntityKind>` with no branch worth naming, and it
makes the counter *derived* rather than maintained.

**(h) A full store does not steal a paid-for unit.** If `spawn` returns `None`
at completion, the entry goes back to the **front** of the queue and production
retries next tick. The player keeps what they paid for rather than losing it to
a silent drop.

**(i) The build tree is two edges.** HQ → Worker, Barracks → Soldier. A Depot
produces nothing: it exists to raise the supply cap, and a building that both
raised supply and produced would make the supply mechanic unobservable.

## Consequences

- Every constant in this record is public, named and asserted by a test that
  quotes the number, so a retune is a visible diff and a mutation is caught.
  Costs: Worker `50C`, Soldier `50C 25G`, HQ `400C`, Depot `100C`,
  Barracks `150C 25G`. Times, in ticks at 60 Hz: Worker `300`, Soldier `360`,
  HQ `600`, Depot `180`, Barracks `300`. Supply: Worker `1`, Soldier `2`;
  grants HQ `10`, Depot `10`, Barracks `0`.
- **None of these numbers is balanced.** They are placeholders chosen to make
  every system observable inside a ~1 600-frame acceptance run. Phase 5 is the
  economy tuning pass.
- The Soldier has no weapon, no health and no combat behaviour. Its selection
  panel line reads `IDLE`, because saying anything else would be a claim the
  code cannot support.
- Buildings cannot be destroyed in phase 1, so `Supply::revoke_cap` and the
  "cap fell below usage" saturation are exercised by unit tests only. Both are
  written and tested anyway — the arithmetic that underflows is the arithmetic
  nobody looked at.
- A worker holding cargo when its drop-off disappears stops and **keeps** the
  cargo. It does not re-target: the only other drop-off might be across the map,
  and silently sending a worker there is worse than stopping visibly.
- Depleted nodes are not despawned. They stay as scenery and draw a distinct
  depleted sprite, so a player can see where the seam ran out.
- The whole economy is allocation-free per tick and reproducible: two harnesses
  given the same orders land on the same `state_hash` — over 4 000 ticks in
  `the_economy_is_reproducible`, 3 000 in `production_is_reproducible`. Both
  properties are asserted.
- No performance claim is made. The recount, the attendance pass and the
  production sweep each walk the live-slot list once per tick; no number is
  measured, published, or gated on.
- **Corrected during implementation.** T10 narrowed the mover's early stop to
  `Order::Move`. It had been generic, so the arrival-radius test against the
  destination cell centre — and the "the field says stop" zero-vector case —
  also ended `Gather` and `Build` orders. That is wrong once decision (d) puts
  an approach cell *outside* the footprint: a worker could be frozen a cell
  short of the drop-off or the site, with its order cleared before the reach
  test that actually completes it ever ran. `Gather` and `Build` now complete
  only on their own reach rules — `DROP_OFF_REACH_CELLS` from the footprint
  rectangle, `BUILD_REACH_CELLS` from the site — and
  `a_gatherer_still_delivers_after_the_hq_is_stamped`,
  `a_builder_walks_to_a_far_site` and `delivery_uses_the_footprint_not_the_centre`
  guard the narrowing. The fix landed with T10; this record names it because
  the two reach rules in decision (b) only hold once the generic stop is gone.
