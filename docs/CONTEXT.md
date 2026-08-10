# Context

Consolidated from former `00-vision.md`, `02-prototype-roadmap.md`, `03-mvp.md`.

## Vision

- Offline-first PC RTS for Steam.
- Custom Rust engine.
- Pixel-art inspired by StarCraft: Brood War and Stronghold.
- Agartha-inspired underground setting.
- Focus on defending against enormous enemy hordes.

### Core Pillars

1. Mechanical RTS gameplay inspired by StarCraft.
2. Fortress defense in every mission.
3. Massive enemy scale.
4. High readability.
5. Accessible complexity.
6. Offline-first.
7. Large player armies (target population cap: 500).

### Artistic Direction

- Smaller units than StarCraft.
- Grid-based building placement.
- Slightly futuristic current-era technology.

## Roadmap

0. **Technical prototype**
   - Validate rendering and movement of massive hordes.
   - Status (2026-08-06): **closed on functional scope.** Every game system is
     covered by automated behavioural tests, and the 5 000-agent gate scene runs
     end to end on the development host (5 000 = `scenario::MAX_LIVE_AGENTS`,
     the engine's live simultaneous-agent ceiling). Performance is
     **unmeasured** and was never a phase-0 criterion: frame-time gating is
     retired, along with the cross-platform/architecture matrix and the
     multi-host validation lab, to a later optimization phase on the finished
     game; their code stays in-tree,
     frozen and non-gating. What phase 0 proves, what it does not, and every known
     gap: [functional close](technical-prototype-functional-close.md). What gates
     a merge: [testing strategy](05-testing.md). Earlier measurements, kept as
     history and claiming nothing:
     [technical prototype results](technical-prototype-results.md) (superseded).

1. **RTS Engine Prototype** — camera, selection, workers, economy, building, unit production.
   - Status (2026-08-10): **closed on functional scope.** All six systems ship
     as a thin vertical slice on a horde-free 320 × 320 scene, each covered by
     named automated tests, and one tracked script drives select → gather →
     build → produce end to end through both the engine and the shipped binary
     (`cargo run -- rts`, on the merge gate). Phase 0 is undisturbed: the
     5 000-agent scene's state hash, the render golden and the scenario
     contract are unchanged. It proves no combat, no enemy AI, no zoom, no
     minimap, no fog of war, no save/load, no second faction and no balance
     pass; performance stays **unmeasured**, and no verification exists for any
     host other than the development one. What phase 1 proves, what it does
     not, and every known gap:
     [RTS engine prototype functional close](rts-engine-prototype-functional-close.md).
     The decisions behind it: [ADR 013](ADR/013_ADR_phase1_scope_and_rts_entity_model.md),
     [ADR 014](ADR/014_ADR_movable_camera_texture_table_and_ui_layer.md),
     [ADR 015](ADR/015_ADR_economy_construction_and_production_determinism.md),
     with the shape of the slice on the
     [architecture page](rts-engine-prototype-architecture.html).
2. **Combat Prototype** — weapons, damage, turrets, enemy AI.
3. **Horde Prototype** — tens of thousands of enemies.
4. **Defense Prototype** — walls, waves, multiple entrances.
5. **Economy Prototype** — tune macro gameplay.
6. **Campaign Prototype** — menus, mission framework, one placeholder mission.
7. **MVP** — menu, settings, save, campaign mission 1, Steam build.

## MVP scope

Deliver:
- Main menu
- Options
- Campaign
- One mission
- Placeholder story
- Complete StarCraft-like RTS gameplay
- Large-scale enemy support validated by prototype

Iterate afterwards with new content and mechanics.
