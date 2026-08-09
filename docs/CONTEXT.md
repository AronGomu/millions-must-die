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
