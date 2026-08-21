# ADR 025: Order feedback, follow orders and entity rally

- Status: Accepted
- Date: 2026-08-20
- Supplements: [ADR 019](019_ADR_hud_minimap_and_input_routing.md), [ADR 021](021_ADR_rts_feedback_polish_and_gather_collision.md)
- Interacts with: [ADR 022](022_ADR_combat_model_and_enemy_faction.md) (right-click on an enemy)

## Context

Orders were legible only through motion. A selected worker walking somewhere looked the same whether it was gathering, building or wandering; nothing marked what it was heading for or where a ground order landed; and a rally point could only be a cell, so "rally to the minerals" meant "walk to a spot near the minerals".

User feedback (round 2, items 2, 3, 5, 6) asked for: a circle on the thing a unit is moving toward that survives reselection, worker status text through the gather round trip, a flag and a dashed line for ground orders, and rally points that can name an entity — with produced units following it.

## Decision

### Feedback is derived, not stored

The target ring and the status line are computed **per frame from the live `Order`**. Nothing records "the last thing you clicked". This is what makes both survive deselect → reselect without a single byte of extra state, and it means an order changed by any path — script, rally, production, combat — is described correctly for free.

Ring targets come from entity-targeting orders only: `Gather { node }`, `Build { site }`, `Follow { target }`, and the combat orders from ADR 022. A ground `Move` has no entity to ring; it gets a marker instead. The ring is the selection ring's colour at half its band width, so "selected" and "targeted" never read alike.

Status vocabulary follows the user's words rather than the code's: `MOVING TO MINERAL / GAS`, `COLLECTING …`, `RETURNING …`, plus `MOVING`, `BUILDING`, `IDLE`, `FOLLOWING`. The card therefore says `CARRYING CRYSTAL 8` above `COLLECTING MINERAL`. That inconsistency is recorded as a known gap, not smoothed over silently: unifying it is a one-token change whenever the product vocabulary settles.

### Ground orders get a bounded, hashed marker

A ground order plants a self-expiring flag at its destination: at most `MAX_MOVE_MARKERS`, oldest overwritten, each expiring after `MOVE_MARKER_TICKS`. Markers are world state and enter `state_hash` like everything else deterministic — a feedback affordance that two processes could disagree about is a determinism hole, not a nicety.

Only *player ground orders* plant one. A unit walking to a rally point plants nothing: the flag means "you told someone to go here".

The dashed line is a **straight bearing** from each selected unit to its own formation slot — not the flow-field route. It answers "where is this going", not "which way around the rock". Tracing the real route per selected unit per frame is a field walk this phase does not need; recorded as a named gap.

### Follow is a real order, not a repeated move

`Order::Follow { target, goal, field }`, appended to the enum. A follower descends its cached field to the target's approach cell, holds at interaction reach, and re-paths only once the target has drifted more than `FOLLOW_REPATH_CELLS` from the goal it last pathed to — bounded field churn, no per-tick rebuild. A dead target ends the order as `Idle`, the same discipline a mined-out node already gets.

### Rally points name a cell or an entity

`RallyTarget::{ Cell, Entity }`, with a discriminant byte in the production hash so the two can never hash alike. On production the hand-off dispatches on the target: a cell moves, a **node gathers**, a unit or building is followed. Rallying to minerals therefore does what an RTS player means by it.

### One right-click, decided by ownership

Right-click dispatch stays a single site, discriminated by what was picked and who owns it: enemy ⇒ attack (ADR 022), friendly unit or building ⇒ follow, resource node ⇒ gather, ground ⇒ move plus a marker. Building-only selections keep their existing rally semantics. Adding a second dispatch path for follow was rejected: two right-click code paths is how ownership rules drift apart.

## Consequences

- The player can read intent off the screen: what a unit is doing, what it is doing it to, and where it was sent.
- The state hash changes twice in the phase — once for markers, once for the new order variant and rally discriminant. Both are deliberate; neither is a re-baseline of any asserted count.
- `Order` gains a variant, so every exhaustive match over it — status labels, hashing, the mover, the target-ring pass — must handle it. That is the intended forcing function for the next order added.
- Ring, marker and dash budgets are all bounded and pre-reserved, so packing stays allocation-free per frame.
