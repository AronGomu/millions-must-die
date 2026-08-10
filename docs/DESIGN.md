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

## Agent collision

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
  booleans. Only `Order::Move` stops on the arrival radius; `Gather` and
  `Build` complete on their own reach tests, measured against a *footprint
  rectangle* rather than a centre.
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
