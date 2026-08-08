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

See [ADR 009](ADR/009_ADR_agent_separation_and_collision.md) and the
[agent collision architecture](agent-collision-architecture.html) page.

## Design Decisions

Gameplay:
- Clone StarCraft 1 feel before innovating.
- Unlimited unit selection.
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
