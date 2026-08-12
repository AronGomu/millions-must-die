# Grill: RTS feedback

## Facts (scout)

- Manual resource bug: `selection::pick_at` recognizes resource only in 1 map cell; renderer draws 48×48 px quad. Most visible resource square falls through to move. — source: `crates/mmd-engine/src/rts/selection.rs`, `crates/mmd-engine/src/rts/pack.rs`, `src/rts_run.rs`
- Headless gather tests call `RtsWorld::order_gather` directly; they bypass click geometry. — source: `crates/mmd-engine/tests/rts_economy.rs`
- Unit pick radius equals scenario collision radius; no separate selection shape exists. — source: `crates/mmd-engine/src/rts/selection.rs`
- RTS units have no unit-unit collision pass. Independent flow-field steps can merge forever. — source: `crates/mmd-engine/src/rts/world.rs`
- Horde collision is soft separation; overlap remains allowed by accepted ADR. Hard RTS collision needs scoped superseding decision. — source: `docs/ADR/009_ADR_agent_separation_and_collision.md`, `AGENT.md`
- RTS speeds: worker 10 cells/s; soldier 8 cells/s. Horde speed: 8 cells/s, frozen by phase-0 contracts. — source: `crates/mmd-engine/src/rts/orders.rs`, `crates/mmd-engine/src/sim/tick.rs`
- Current HUD: fixed 1920×1080; text-only selection/production/build blocks; no clickable UI router, minimap, settings, or modal. — source: `crates/mmd-engine/src/rts/hud.rs`, `src/rts_run.rs`
- Current camera uses one 24-cell/s speed for keyboard and edge pan. Center clamps to scenario grid edges; viewport size is ignored. — source: `crates/mmd-engine/src/render/camera.rs`
- Current app has no settings model, persistence path, window-mode control, or pointer confinement. Pinned `sdl3` exposes required window/mouse APIs. — source: `src/rts_run.rs`, `Cargo.toml`, pinned `sdl3` source
- Current app has no audio system or audio assets. Pinned core SDL can load WAV and control stream gain; no mixer/codec dep is present. — source: `Cargo.toml`, pinned `sdl3` source
- “change the updates” has no matching product term in repo. Sim update rate is fixed and deterministic. — source: `feedback.md`, `src/rts_run.rs`
- “Terran One” file/license/provenance absent. Repo cannot distribute copyrighted soundtrack without legal rights. — source: `LICENSE`, `third_party/README.md`, `assets/sprites/source/README.md`
- Merge gate remains behavioral, deterministic, allocation-aware, offline; no perf number may gate. — source: `docs/05-testing.md`, `AGENT.md`

## Round 1 — Product behavior

| # | Question | Answer | Precision |
| --- | --- | --- | --- |
| 1 | History framing | Phase 1.1 interaction, UI, and audio hardening | — |
| 2 | Resource right-click by entity capability | Workers gather; non-workers move to resource approach cell | — |
| 3 | Overlapping resource right-click priority | Frontmost rendered entity wins | — |
| 4 | Player selection shape | Per-unit sprite body rectangle union collision circle | — |
| 5 | Hard non-overlap population | All RTS-world units, including future enemies | Horde `sim` remains soft-overlap |
| 6 | Collision radius | Worker and soldier: 3 cells | Half current 6-cell radius |
| 7 | Tripled speeds | RTS worker 30 cells/s; RTS soldier 24 cells/s | Horde speed unchanged |
| 8 | Persistence | Save per-user settings immediately | — |
| 9 | Camera settings | Separate keyboard-pan and edge-pan speed sliders | — |
| 10 | Menu/Escape | Gear pauses; Escape backs out; gameplay Escape opens menu | — |
| 11 | Pointer confinement | Confine during focused gameplay; release on focus loss only | — |
| 12 | Command grid | Context grid: worker builds; producer trains; empty slots disabled | — |
| 13 | Minimap v1 | Map silhouette and camera viewport rectangle only | Click-to-camera required |
| 14 | Bounds model | Engine cap validates scenarios; scenario extent is playable map; viewport-shrunk extent is camera area | — |
| 15 | “updates” meaning | Sound-effects volume | Four buses: master/music/voice/SFX |
| 16 | Unit cue fan-out | One beep per selected unit, capped at eight simultaneous cues | — |
| 17 | Music delivery | Implement loader and ship generated placeholder music; replace with licensed music later | No copyrighted track in repo now |

## Round 2 — System semantics

| # | Question | Answer | Precision |
| --- | --- | --- | --- |
| 1 | Body vs static geometry | 3-cell body clears units, terrain, buildings, static obstacles, map edges | Radius-aware nav/approach required |
| 2 | Occupied spawn | Deterministic nearest-free outward search; production waits if none | No temporary overlap |
| 3 | Group arrival | Deterministic formation slots; choke queue uses rotating deterministic priority | — |
| 4 | Display scaling | Aspect-fit 16:9; letter/pillarbox; inverse mouse; bar clicks ignored; edge-pan at content edge | — |
| 5 | Window sizes | Exclusive closest 1920×1080; windowed 1280×720 resizable | Borderless desktop remains default |
| 6 | Focus loss | Clear held input/drag; release pointer; sim keeps running | Add toggleable pause-on-focus-loss setting if possible |
| 7 | Camera sliders | Keyboard + edge default 48 cells/s; range 6–96; step 6 | — |
| 8 | Selection panel | Single portrait/details; multi up to 24 sorted icons + overflow; click isolates; Shift-click toggles | — |
| 9 | Command grid | Fixed 3×3 stable slots; mouse + existing hotkeys share commands | — |
| 10 | Minimap projection | Isometric diamond + projected camera polygon | — |
| 11 | RTS map cap | 512×512 cells | Current scene stays 320×320 |
| 12 | Voice triggers | Newly selected player units + units accepting move/gather/build; first eight sorted; rest dropped | Distinct beep for rejected orders |
| 13 | SFX bus | Menu/settings/command-grid/minimap click beep | — |
| 14 | Volume controls | 0–100%, step 5; master 80, music 35, voice 70, SFX 60 | — |
| 15 | Music lifecycle | Autoplay + continuous loop, including unfocused; audio failure aborts startup | — |

## Shared understanding

- Goal: phase 1.1 vertical slice fixes manual RTS control mismatch, separates pick/body geometry, adds hard RTS collision, triples RTS unit speed, ships settings/window controls, rebuilds HUD, adds minimap/camera frontier, adds generated placeholder audio.
- Scope: RTS world/app only. Horde `sim/`, phase-0 hashes, benchmark policy remain unchanged.
- Resource orders: full rendered resource quad participates in frontmost rendered-entity picking. Workers gather; selected non-workers move to resource approach cell. Headless acceptance must exercise same click path as live SDL.
- Selection: per-kind rendered sprite-body rectangle ∪ 3-cell collision circle. Selection geometry independent from collision geometry.
- Collision: all current/future RTS-world units use hard non-overlap. Body also clears terrain, buildings, static obstacles, map edges. Radius = 3 cells for worker/soldier. Deterministic nearest-free spawn, formation slots, rotating choke priority. No per-unit pathfinding; pooled flow fields remain.
- Interaction reach: gather/build/drop-off approach distances derive from 3-cell body + target footprint so workers interact at legal contact without entering target geometry.
- Construction assumption: unfinished sites remain walkable/non-solid until completion, matching current attended-build model; completion performs deterministic radius-aware evacuation before footprint becomes solid.
- Speed: worker 30 cells/s; soldier 24 cells/s. Horde 8 cells/s unchanged.
- Settings: per-user persisted immediately. Default borderless desktop fullscreen, pointer confined while focused, exclusive fullscreen closest 1920×1080, 1280×720 resizable windowed. Fixed 1920×1080 logical canvas aspect-fits with bars + exact inverse input transform.
- Focus assumption: focus loss always clears held input/drag + releases pointer. `pause_on_focus_loss` setting exists, defaults off; when on, focus loss opens paused menu. This resolves “if possible” as in-scope.
- Camera: separate keyboard/edge speeds. Both default 48 cells/s; 6–96; step 6. Engine map cap 512×512; scenario extent = playable map; viewport-shrunk center area = camera frontier.
- HUD: bottom-left is isometric minimap + camera polygon; center is single details or sorted 24-icon multi-grid + overflow; right is fixed 3×3 context command card. HUD consumes pointer input before world input. Minimap click recenters camera. Settings gear at top-right opens paused one-button menu; Settings opens nested panel; Escape backs out; gameplay Escape opens menu.
- Audio: generated WAV placeholder music + generated beep assets; no copyrighted StarCraft file. Music auto-starts/loops continuously, including unfocused. Unit voice cues fire for newly selected units + accepted move/gather/build units, first 8 sorted. Distinct single reject cue fires once per rejected unit-order action on voice bus. UI click cue uses SFX bus. Effective bus gain = master × bus.
- Audio assumption: audio init failure aborts interactive window startup. Offscreen/headless validation does not initialize physical audio; deterministic fake sink validates events/gains.
- Volume: 0–100%, step 5; defaults master 80, music 35, voice 70, SFX 60.
- Out of scope: copyrighted soundtrack, combat, enemy AI implementation, zoom, fog/minimap entities, responsive dynamic render target, horde collision rewrite, performance gates.
- Docs: new superseding/supplementing ADRs; update RTS/collision architecture HTML, status/design/testing/glossary docs. Plan only now; no feature implementation.

## Confirmation

- User confirmed shared understanding on 2026-08-10.
