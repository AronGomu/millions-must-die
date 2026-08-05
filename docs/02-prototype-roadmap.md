# Prototype Roadmap

0. Technical prototype
- Validate rendering and movement of massive hordes.
- Status (2026-08-05): **open — 50k gate evidence inconclusive; native
  cross-platform matrix deferred.** All locally runnable gates pass on the
  exact candidate commit and the real Linux/Vulkan production bench records
  50k medians ~10x inside the limits with 0 allocations in measured frames,
  but 50k trial noise exceeded the locked `nmad <= 0.03` bound, so no verdict
  may be frozen; one quiet-host rerun remains. Windows/D3D12 and macOS/Metal
  native lanes, real 3-host gate runs, and physical-pilot baselines remain
  `deferred-hw` pending reference hardware. Neither a phase-0 pass nor a
  failure is claimed. Evidence:
  [technical prototype results](technical-prototype-results.md).

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
