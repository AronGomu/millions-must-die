# Design

Consolidated from former `01-technical-architecture.md`, `04-design-decisions.md`.

## Technology Architecture

- Language: Rust
- Platform: SDL3
- Engine: Custom
- Rendering: Batched GPU sprite renderer
- Simulation: Data-oriented
- Navigation: Flow fields
- Spatial partition: Uniform grid — preallocated, rebuilt every tick as the agent-separation neighbour index (see [Agent collision](#agent-collision))
- Native first, browser/WebAssembly later.

Rules:
- No per-frame allocations in simulation.
- No per-enemy pathfinding.
- Benchmark every major system.

Detailed phase-0 designs:
- [Technical prototype](technical-prototype-architecture.html)
- [Simulation and navigation](simulation-navigation-architecture.html)
- [Sprite renderer](sprite-renderer-architecture.html)
- [Local validation lab](local-validation-lab-architecture.html)
- [Architecture decision records](ADR/README.md)

Detailed phase-1 designs:
- [RTS engine prototype](rts-engine-prototype-architecture.html)
- [RTS interaction, UI and audio hardening](rts-interaction-ui-audio-hardening-architecture.html) (phase 1.1)

Detailed phase-2 designs:
- [Combat prototype](combat-prototype-architecture.html)
- [RTS feedback round 2](rts-feedback-round2-architecture.html)

## Agent collision

This section is about the **horde** (`crates/mmd-engine/src/sim`) only. Player
RTS units have hard bodies and a different contract entirely — see
[RTS bodies, navigation and formations](#rts-bodies-navigation-and-formations).

Bodies are scenario data: each scenario declares a collision radius and a
separation strength in Q8 fixed point. The model is soft separation
*steering*, not resolution — it never guarantees agents cannot overlap. The
neighbour index is a preallocated uniform grid, rebuilt every tick with no
allocation. The coincidence tie-break — two agents landing on the exact same
position — is a 16-entry table keyed on the index pair, so the outcome is
deterministic rather than order-dependent. Each agent accumulates at most
eight neighbours per tick. A blended step must be admissible on exactly the
terms a flow-field step is: its target cell walkable, and — when it carries the
centre diagonally between cells — both shared cardinal neighbours clear, the
flow field's own no-corner-cut rule. An inadmissible blended step falls back to
the pure descent step rather than wedging the agent in place, so separation can
neither pin an agent against a wall nor steer one into a walkable-but-
unreachable corner pocket.

Three scenario knobs — `separation_phases`, `mass_class_count`,
`separation_threads` — gate how often the scan and grid rebuild run, how the
push is weighted, and how many threads run it, each with an identity default
of `1` at which the engine's walk is unchanged. See
[ADR 009](ADR/009_ADR_agent_separation_and_collision.md),
[ADR 010](ADR/010_ADR_separation_amortisation_and_push_priority.md),
[ADR 011](ADR/011_ADR_parallel_separation_and_the_allocation_invariant.md), the
[agent collision architecture](agent-collision-architecture.html) page and the
[horde sim headroom architecture](horde-sim-headroom-architecture.html) page.

## RTS entity model

Player entities live in their own store, beside the horde rather than inside
it. `sim::Simulation` is a fixed-population walker with one destination and a
recycle cursor; putting orders, cargo, construction progress and production
queues into it would put RTS state inside the type every pinned phase-0 hash
digests.

- **Entity store.** `rts::EntityStore` is a preallocated SoA store of
  `MAX_ENTITIES = 2 048` slots with generational ids and a LIFO free list.
  Every column is reserved at construction, so a spawn never allocates, and a
  stale `EntityId` resolves to `None` instead of naming whoever reused its
  slot.
- **Orders.** One `Order` value per unit, hashed: `Move`, `Gather` (with its
  `ToNode` / `Mining` / `Returning` phase) or `Build`. The whole worker loop is
  therefore pinned by `RtsWorld::state_hash` rather than by tests poking at
  booleans. `Order::Move` stops on its own formation slot centre (phase 1.1:
  `FORMATION_ARRIVAL_CELLS = 0.25`, replacing the old shared arrival radius);
  `Gather` and `Build` complete on their own reach tests, measured against a
  *footprint rectangle* rather than a centre.
- **Flow-field pool.** The "no per-enemy pathfinding" rule extends to player
  units. `nav::FieldPool` holds `NAV_FIELD_SLOTS = 8` preallocated fields keyed
  by destination cell, evicted exact-LRU with ties to the lowest slot. Units
  sharing a destination share a field, which is group cohesion for free.
  Stamping a new obstacle invalidates **every** cached field, not the ones that
  look affected.
- **Economy and supply.** Two resources (Crystal, Gas) and one worker loop.
  Supply is **reserved at enqueue**, never at completion, or the cap would be
  decoration; `Supply::used` is **recomputed** every tick from live units plus
  queue reservations rather than incremented at five call sites. The cap is
  clamped at the 500-unit design pillar in the type, not by a caller
  remembering.
- **Grid placement.** Four ordered rules — in bounds, terrain (which carries
  every finished building, since a finished footprint is stamped into the
  navigation mask), overlap with an unfinished site, resource nodes. A unit
  standing on the plot is deliberately *not* a rule: placement validity must
  not flicker with a moving unit. A site is walkable so its builder can stand
  inside it; the footprint blocks only at completion.

Decisions: [ADR 013](ADR/013_ADR_phase1_scope_and_rts_entity_model.md),
[ADR 014](ADR/014_ADR_movable_camera_texture_table_and_ui_layer.md),
[ADR 015](ADR/015_ADR_economy_construction_and_production_determinism.md).
Shape of the slice: the
[RTS engine prototype architecture](rts-engine-prototype-architecture.html)
page. What it proves and does not:
[functional close](rts-engine-prototype-functional-close.md).

## RTS bodies, navigation and formations

Phase 1.1. Everything here is **RTS-only**; the horde section above still
describes `sim/`, and no document may merge the two claims.

- **Hard bodies.** `UnitKind::body_radius_cells` returns 3.0 for both Worker
  and Soldier, so contact distance is 6.0 and penetration is a strict `<` with
  no epsilon. Movement is a rotated sequential proposal/commit: units are taken
  in ascending slot order rotated by tick index, each candidate step is swept
  continuously against the static world and every other body, and it is
  accepted whole or not at all. A mover may displace bodies along their contact
  normals through a bounded chain (`MAX_PUSH_DEPTH = 3`,
  `MAX_PUSHED_BODIES = 8`, one displacement per body per tick, all-or-nothing);
  only after a chain is rejected does it try its descent rotated ±45° then
  ±90°. Nothing relaxes an existing overlap: every displaced position is proven
  legal before anything commits.
- **The invariant, stated narrowly.** A completed tick may leave two RTS unit
  bodies merged in exactly two cases: both units are active gather workers, or
  that exact pair is inside its bounded 12-attempt gather-exit transition
  (ADR 021, narrowing ADR 017). Every other merged pair is repaired, or counted
  by `body_overlaps` and reported through `TickError::UnrepairableOverlap`.
  Never write this rule without its exception, and never widen the exception:
  static terrain, map edges, resource nodes and finished buildings stay hard
  for gathering workers too, and a pair with a non-worker or non-gathering
  member is an ordinary hard pair.
- **Static navigation.** `rts::StaticNav` holds the raw solids (terrain,
  resource footprints, finished buildings) and a *centre-blocked* mask of the
  positions a 3-cell circle cannot occupy. Pooled fields rebuild over the mask;
  building **placement** keeps the raw rules, so body clearance never widens
  the exclusion a player sees.
- **Interaction reach is adaptive.** `orders::interaction_reach` (body radius +
  `NAV_CENTER_TOLERANCE_CELLS`) is a floor. The reach an order actually needs
  is `max(that, chosen approach-cell distance + tolerance)`, because dense
  inflated terrain can push every legal approach cell past the flat distance.
- **Formations.** One group order takes one pooled anchor field and assigns
  each member a distinct deterministic 6-cell lattice slot; insufficient slots
  reject the whole order rather than moving some of it. Terminal steering to a
  slot is bounded local placement, not per-unit pathfinding — `NAV_FIELD_SLOTS
  = 8` stands.
- **Body-safe transitions.** Production spawns on the nearest legal free
  position or waits with the unit paid and its supply reserved; a finishing
  site atomically preplans every evacuation before stamping itself solid, or
  stays walkable one more tick.

- **Gather-exit transition.** The pair table is a preallocated dense
  triangular byte per entity pair: `0` hard, `255` active gather provenance,
  `1..=12` completed exit attempts. One worker moves at most half a cell away
  per attempt; an attempt that reaches non-penetration clears the pair to hard
  the same tick, before normal movement. After twelve attempts a relocation is
  searched inside the same navigation component, and a failed relocation
  returns the pair to hard and reports it rather than granting it permanent
  grace. The table enters the state hash in a length-prefixed,
  identity-pinned block, so a replay cannot silently disagree about which
  slots a byte belonged to.

Decisions: [ADR 016](ADR/016_ADR_phase1_1_scope_and_input_geometry.md),
[ADR 017](ADR/017_ADR_rts_hard_collision_navigation_and_formations.md),
[ADR 018](ADR/018_ADR_settings_window_canvas_and_camera.md),
[ADR 019](ADR/019_ADR_hud_minimap_and_input_routing.md),
[ADR 020](ADR/020_ADR_audio_events_buses_and_generated_assets.md), and
[ADR 021](ADR/021_ADR_rts_feedback_polish_and_gather_collision.md) for the
gather policy. Shape of the slice: the
[phase 1.1 architecture](rts-interaction-ui-audio-hardening-architecture.html)
page. What it proves and does not:
[phase 1.1 functional close](rts-interaction-ui-audio-hardening-functional-close.md).

## Feedback polish

An extension of phase 1.1, driven by user feedback rather than by a missing
system. Everything here is app/render-side except the gather policy above.

- **Controls have identity.** Every discrete control carries a stable id and a
  framed visual state (`Disabled > Pressed > Hover > Selected > Idle`); an
  activation requires the same control on pointer-down and pointer-up, so a
  press that slides off a button does nothing. The top-right control is framed
  text `MENU`; the pause menu gains `CLOSE MENU` beneath the unmoved Settings
  button.
- **Settings are manipulated, not nudged.** Six numeric settings share one
  descriptor table: each has a live snapped slider and a three-digit typed
  field, and every accepted edit commits through the one runtime → gains →
  save → publish transaction. Each audio bus has a persisted mute flag that
  zeroes gain while keeping the stored level. The body scrolls by wheel and
  thumb over a single offset shared by render and hit test; Back and the
  warning stay fixed. A wheel notch is a mapped pointer event, so a letterbox
  bar drops it.
- **Two texture-free primitives, one instance layout.** The world grid and the
  drag box are diagonal line instances with their own sentinel, drawn in the
  depth-off procedural overlay without sampling the atlas — which is the only
  way to get an exact opaque pure-green border, since the panel-fill prop
  renders `texel * tint`. The grid is the full map lattice
  (`width + height + 2` lines), app-side config, default on, and never enters
  the world hash.
- **Placement helps the click.** An invalid cursor cell searches the
  surrounding footprint corners and ranks candidates from the corner the ghost
  is actually drawn at; preview and commit consume the same candidate, so the
  green footprint a player sees is the one that gets built.
- **Buildings answer clicks.** A player building is picked by its rendered
  sprite rect ∪ its ground footprint, and its card is exactly seven lines:
  kind, HP, ready/build %, supply grant, queue, head progress, rally (six
  until phase 2 inserted the HP line under the kind line).
- **Command keys are positional.** `QWE`/`ASD`/`ZXC` map row-major onto the
  3×3 card, so the key is wherever the button is; keyboard and pointer share
  one executor and a disabled slot is a silent no-op.

Decision:
[ADR 021](ADR/021_ADR_rts_feedback_polish_and_gather_collision.md). Shape:
[feedback-polish architecture](rts-feedback-polish-architecture.html). What it
proves, what it does not, and the one open regression:
[feedback-polish functional close](rts-feedback-polish-functional-close.md).

## App shell, HUD and audio

Also phase 1.1, and deliberately **app-side**: `RtsWorld` stays clock-free,
deterministic and audio-free.

- **Settings** are one per-user schema-1 JSON file under the SDL pref path.
  Missing, malformed, out-of-range or wrong-schema loads fall back to defaults
  with a warning on stdout. Offscreen runs never read or write it, so a test
  run can never depend on the developer's own configuration.
- **Logical canvas.** World and UI are always 1920 × 1080. The destination is
  the largest centred exact 16:9 integer rect (`render::DisplayViewport`);
  everything outside it is a cleared bar that rejects clicks and clamps motion
  to the content edge.
- **Camera frontier.** The camera clamps to the *projected* map AABB inset by
  half the logical view, not to the raw grid edge, so no pan can show dead
  space. Keyboard and edge pan carry independent speeds.
- **HUD first, world second.** A fixed pointer-owner order — lifecycle, Escape,
  modal, MENU, minimap, selection icons, command card, HUD background, world —
  is chosen at mouse-down and retained through the gesture. Every textured HUD
  element is a `ui` draw group; `overlay` stays procedural rings and
  texture-free lines only.
- **Audio derives from receipts.** Accepted-action receipts become semantic
  `AudioEvent`s (music, select/move/gather/build/reject voice, UI click),
  capped and sorted, weighted by Music/Voice/SFX buses under a master scalar,
  and handed to a sink. Offscreen runs use a fake sink and open no device.

## Combat

Phase 2. One combat contract for every armed thing, player or enemy. The
[Agent collision](#agent-collision) section above still describes the horde's
`sim/` soft separation, which phase 2 never touched; everything here is the
RTS side.

- **Instant hit, armor floor.** A weapon is damage / cooldown / range
  (`crates/mmd-engine/src/rts/combat.rs`). A hit lands the tick it fires — no
  projectile entities — for `max(1, damage - armor)`, so armor mitigates but
  never zeroes a hit. Placeholder stats, balance being a later phase: Soldier
  40 HP / 0 armor, damage 6, cooldown 15 ticks, range 24, speed 24 cells/s;
  Ghoul 30 / 0, damage 5, cooldown 30, range 8, speed 18 cells/s; Turret
  150 / 1, damage 10, cooldown 20, range 36; Worker 25 / 0, unarmed, speed
  30 cells/s; HQ 400 / 2, Depot 150 / 1, Barracks 200 / 1.
- **Targeting.** Nearest valid target inside weapon range, lowest-slot
  tie-break, range measured against the target's body circle or footprint
  rectangle rather than its centre. `Idle` and `AttackMove` auto-acquire and
  hold in place to fight; `Attack` fires only at its own target, chases it
  while out of range, and falls back to auto-acquire the tick that target
  dies; `Move`, `Gather`, `Build` and `Follow` never fire, so a retreat order
  is a real retreat. Target selection re-runs every tick while damage lands
  only on the cooldown edge — which is why consecutive shots into a moving
  clump spread instead of finishing one body. Persistent targeting is a later
  phase.
- **Enemy objective chain.** Every `OWNER_ENEMY` unit holds a permanent
  `Order::AttackMove` at **one** faction objective: the approach cell of the
  live player building nearest the faction origin (the HQ while it lives,
  lowest-slot tie-break, then Idle when no player building remains). The
  approach cell, not the building's own cell — a finished footprint is blocked
  in the inflated centre mask, so no field can be built to it. The objective
  recomputes on a building death, never by per-tick scan, and all enemies
  descend one shared pooled flow field per objective — hundreds of enemies,
  one field, so "no per-enemy pathfinding" and `NAV_FIELD_SLOTS = 8` both
  stand. One objective cell also means one approach: on the gate scene every
  wave converges on the HQ's north approach cell, whichever corner it spawned
  from.
- **Death is routed, not special-cased.** Units despawn; buildings — the HQ
  included — un-stamp their footprint, which invalidates every pooled field; a
  dying production building cancels its queue with no refund; HQ death sends
  gatherers Idle. The run is a sandbox: no win or lose, the outcome rides the
  exit tokens `kills`/`losses`/`enemies_spawned`/`first_combat_tick`/
  `hq_alive`.
- **Turret.** `BuildingKind::Turret`: worker-built for 75 crystal under the
  unchanged four placement rules, an 8 × 8-cell footprint (one build square),
  grants no supply, is no drop-off, and once finished auto-fires the nearest
  enemy with range measured from its footprint rectangle — a corner Ghoul must
  not cost it reach. An unfinished site never fires. Fire is silent this phase
  (named gap in the close doc).
- **Enemies are ordinary hard pairs.** The Ghoul takes the RTS hard-body
  contract as-is — ADR 021's gather-worker exception is not widened, so every
  enemy-touching pair is repaired, or counted by `body_overlaps` and reported.
  The horde keeps ADR 009's soft separation and may still overlap; never merge
  the two claims.
- **Hundreds on the gate, horde later.** The gate scene spawns 400 enemies
  across four scripted waves. The phase-3 claim — tens of thousands — is a
  different scale and a different slice; nothing here advances or spends it.

Decisions: [ADR 022](ADR/022_ADR_combat_model_and_enemy_faction.md) and
[ADR 023](ADR/023_ADR_combat_gate_scale_and_rebaseline.md). Shape of the
slice: the [combat architecture](combat-prototype-architecture.html) page.
What it proves and does not:
[functional close](combat-prototype-functional-close.md).

## Feedback round 2

Phase 2 as well, driven by user feedback rather than by a missing system. The
collision sentences below are about **RTS hard bodies**, not the horde.

- **A build grid for buildings only.** `BUILD_SQUARE_CELLS = 8`. Every
  footprint is a whole number of squares — Depot 8 (1 × 1), Turret 8 (1 × 1),
  Barracks 16 (2 × 2), HQ 24 (3 × 3) — the ghost floors its min corner to a
  square boundary, and a scenario-declared building that is not square-aligned
  is refused at load. The overlay draws squares instead of the per-cell lattice
  when the world grid is on, and is forced on while a ghost is pending.
  **Units keep moving in true float cells and ignore the square entirely**:
  the grid is a placement device, never a movement one.
- **A builder can always leave.** Completion already evacuated every body its
  footprint covered to a legal, distinct centre and cleared the builder's
  order; the reported trap did not reproduce through that path. What was
  genuinely unbounded — and is now fixed — is a site whose evacuation can never
  succeed: it used to hold at `build_ticks - 1` forever with the cost already
  spent. `STALLED_SITE_TICKS = 180` consecutive discarded plans now cancel it
  and refund in full. There is deliberately no "finish anyway" relaxation:
  within one connected region a relaxed search could only return a position
  that penetrates static geometry or one merged with another body, which the
  ADR 021 policy forbids outside the gather exception.
- **The card says what a unit is doing.** Status vocabulary: `IDLE`, `MOVING`,
  `BUILDING`, `FOLLOWING`, `MOVING TO MINERAL|GAS`, `COLLECTING MINERAL|GAS`,
  `RETURNING MINERAL|GAS`; node cards show `YIELD`. `Attack` and `AttackMove`
  both read `MOVING` — there is no `ATTACKING` string this phase — and an
  enemy card is kind plus HP only, with no status line at all.
- **Target rings are derived, not stored.** Whatever a selected unit's live
  order points at gets a thin ring each frame, which is what makes the ring
  survive deselect-and-reselect. One ring per target, never doubled with the
  selection ring.
- **Move markers and dashed bearings.** A ground order plants a bounded,
  hashed, self-expiring `Prop::MoveMarker` (`MAX_MOVE_MARKERS = 8`,
  `MOVE_MARKER_TICKS = 90`, oldest dropped first); a rallied unit plants none.
  A selected mover draws a dashed **straight bearing** to its goal — not the
  flow-field route it will actually walk — and a huge selection skips dashes
  entirely.
- **Follow, and rally points that name an entity.** `Order::Follow { target }`
  closes to interaction reach, holds there, re-paths once the target has moved
  `FOLLOW_REPATH_CELLS`, dies with its target, and never targets an enemy or
  itself. `RallyTarget` is either a `Cell` or an `Entity`, so a Barracks
  rallied onto a node produces workers that gather without a second order.
  Right-click resolves by ownership: enemy ⇒ attack, friendly ⇒ follow, node ⇒
  gather, ground ⇒ move.
- **A sandbox scene for hands.** `assets/scenarios/rts_sandbox_v1.ron`: a
  prebuilt base, eight Soldiers, thirty authored waves, run with `--scenario`
  and no `--frames`. It is **deliberately not on the merge gate** — an untimed
  scene proves nothing on a timer, and a test asserts its absence from every
  gate block.

Decisions: [ADR 024](ADR/024_ADR_build_grid_and_placement_snap.md) and
[ADR 025](ADR/025_ADR_order_feedback_follow_and_entity_rally.md). Shape of the
slice: the
[feedback round 2 architecture](rts-feedback-round2-architecture.html) page.

## Design Decisions

Gameplay:
- Clone StarCraft 1 feel before innovating.
- Unlimited unit selection. Implemented as `MAX_SELECTION == MAX_ENTITIES`:
  the selection can hold every entity the store can hold, so there is no
  selection cap to hit before the entity cap.
- Improved pathfinding.
- Grid placement.
- 500 unit population cap.
- Base-building in every mission.
- No base-less missions.

Themes:
- Civilization growth.
- Population growth as an important mechanic.
- Massive defensive battles.

Setting:
- Agartha-inspired underground world.

Visual references:
- StarCraft Brood War
- Stronghold

Character sprite inspirations:
- Asmongold
- Malcolm (BasedCamp)
- Simon (BasedCamp)
- Lifi
