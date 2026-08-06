# Prototype Roadmap

0. Technical prototype
- Validate rendering and movement of massive hordes.
- Status (2026-08-06): **closed on functional scope.** Every game system is
  covered by automated behavioural tests, and the 50k scene runs end to end on
  the development host. Performance is **unmeasured** and was never a phase-0
  criterion: frame-time gating is retired, along with the
  cross-platform/architecture matrix and the multi-host validation lab, to a
  later optimization phase on the finished game; their code stays in-tree,
  frozen and non-gating. What phase 0 proves, what it does not, and every known
  gap: [functional close](technical-prototype-functional-close.md). What gates
  a merge: [testing strategy](05-testing.md). Earlier measurements, kept as
  history and claiming nothing:
  [technical prototype results](technical-prototype-results.md) (superseded).

1. RTS Engine Prototype
- Camera
- Selection
- Workers
- Economy
- Building
- Unit production

2. Combat Prototype
- Weapons
- Damage
- Turrets
- Enemy AI

3. Horde Prototype
- Tens of thousands of enemies.

4. Defense Prototype
- Walls
- Waves
- Multiple entrances

5. Economy Prototype
- Tune macro gameplay.

6. Campaign Prototype
- Menus
- Mission framework
- One placeholder mission

7. MVP
- Menu
- Settings
- Save
- Campaign mission 1
- Steam build
