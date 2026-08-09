# RESEARCH — "They Are Billions"-class horde performance, adapted for *Millions Must Die*

**Question asked:** *"I want the same performance as They Are Billions for similar or fewer enemies. Scour the internet for all information about the technical implementation. Interpret and adapt it for my 2D isometric StarCraft-like RTS."*

**Date:** 2026-08-09
**Repo state at time of research:** branch `plan/zombie-collision`, HEAD `108230a`
**Scope:** two parallel primary-source sweeps — (A) what Numantian Games actually said about *They Are Billions*, (B) the owner-source technical literature on simulating and rendering 20k–50k agents.

---

## Confidence tags used throughout

| Tag | Meaning |
|---|---|
| `[PRIMARY]` | Authored by the owner of the fact — the developer, the paper's author, the vendor's own docs, the shipped source. |
| `[SECONDARY]` | Third-party reporting or community reverse engineering. Evidence about the binary, not a developer statement. |
| `[INFERENCE]` | My reasoning on top of the sources. Explicitly flagged; never presented as fact. |
| `[UNVERIFIED]` | Widely repeated but no owner source was found. Treat as folklore. |

> **Merge-gate note.** This file lives in `.tmp/`. The `no_perf_claim_in_docs` test (`tests/validation_contract.rs:934`) scans only `README`, `ROADMAP`, `CONTRACT`, `CLOSE`, and `CONTRIBUTING.md` (`LIVE_DOCS`, line 770). Numbers here are safe. **If you move any figure from this file into `docs/` or the gated docs, the gate will fail** — and correctly so, because every number below is somebody else's measurement, not ours.

---

# PART 0 — The verdict, in six lines

1. **They Are Billions does not use flow fields.** The developers wrote this down explicitly. Every zombie computes its own path, invalidated by events. `[PRIMARY]`
2. **Its horde trick is flocking as a perception LOD** — most zombies follow neighbours instead of running their own perception. `[PRIMARY]`
3. **It is C#/.NET with a forked C++ physics library**, and the physics fork went from 200 bodies to 20,000 by hand-optimisation. `[PRIMARY]`
4. **It has no fixed timestep and no determinism** — the developers confirmed the game literally runs faster on faster CPUs. Your engine is already strictly better here.
5. **Your architecture (flow field + soft separation + SoA + uniform grid + zero-alloc + determinism) is closer to the modern shipped state of the art (AoE IV, Supreme Commander 2) than TAB's is.** You are not behind; you are on a different, better-documented line.
6. **The three cheapest unexploited wins, all primary-sourced:** amortise separation across N ticks (Reynolds got a free 10× at 15k agents), write `z = f(y)` + alpha-test to delete the 50k-sprite depth sort before you ever write one, and parallelise the separation pass (it is already a pure read-only pass).

---

# PART A — What *They Are Billions* actually is

## A.0 The entire primary corpus

Numantian published very little. This is all of it.

| Source | Type | URL | Date |
|---|---|---|---|
| Steam thread "Game Engine for 20k units?" — 4 developer replies | `[PRIMARY]` | https://steamcommunity.com/app/644930/discussions/0/1353742967825284553/ | Jun–Sep 2017 |
| Numantian blog "Development Update v.0.8.2" | `[PRIMARY]` | https://numantiangames.com/News/2018-06-07-they-are-billions-development-update-v-0-8-2/ | 2018-06-07 |
| Xataka interview with Jesús Arribas (Spanish) | `[PRIMARY]` | https://www.xataka.com/videojuegos/entre-bambalinas-de-they-are-billions-el-nuevo-bombazo-del-videojuego-espanol | 2018-05-29 |
| GamesBeat "The making of early access hit They Are Billions" | `[PRIMARY]` | https://gamesbeat.com/the-making-of-early-access-hit-they-are-billions/ | 2018-09-12 |
| Steam developer reply on game speed / CPU | `[PRIMARY]` | https://steamcommunity.com/app/644930/discussions/0/3022387599786311865/ | 2019-04-11 |
| Steam "Version Changes" changelog thread | `[PRIMARY]` | https://steamcommunity.com/app/644930/discussions/0/1499000547495151383/ | 2017–2021 |
| Store page / official site feature copy | `[PRIMARY]` | https://store.steampowered.com/app/644930/ | ongoing |

**Named developer:** **Jesús Arribas**, director and programmer. Per Xataka he wrote the game *including its graphics engine*, alongside two other programmers.

**GDC Vault: no entry exists.** No Numantian talk, slides, or postmortem. `[verified absence]`

The **Xataka interview** and the **0.8.2 blog post** carry almost all the real technical content. If only two sources get read, those are the two.

---

## A.1 The engine

### Language and runtime `[PRIMARY]`

> **"Our engine actually can move up to 30K units on a icore7 processor. The engine is programmed with c# over microsoft .Net. It is heavily optimized and scaless very well the performance over multiple cores. Take in mind that the trailer scenes are real gameplay scenes moved by our engine in real time while the same computer was recording the video and encoding it in x264 and in 4K resolution!"**
> — Numantian Games `[developer]`, 2017-06-14 · https://steamcommunity.com/app/644930/discussions/0/1353742967825284553/

### Why C#, not C++ `[PRIMARY]`

> **"It depends on many factors. For example, c++ assemblies are precompiled to a specific architecture while .NET assemblies are compiled dynamically to adapt the code to the current architecture so it can use specific instruction sets and optimizations. Also the flexibility of C# allow us to create very complex and optimized systems that in c++ would be much more difficult and more vulnerable to errors (memory leaks...). Nevertheless our physics library is compiled in c++ so we use the best of both worlds :)"**
> — Numantian Games `[developer]`, 2017-08-21 · same thread

**This is the single most important architectural statement in the corpus: a managed C# game/AI/pathfinding layer over a native C++ physics/collision library.**

### Why not Unity `[PRIMARY]`

> **"We tried Unity at first, but it was impossible to move smoothly [with] even as little as 400 units."**
> **"That's why we decided to create our own engine, [which is] much more optimized."**
> **"From a technical point of view, our biggest challenge was to optimize the game engine to be able to move hives of up to 30,000 zombies."**
> **"[We] iterated again and again, improving all aspects of the engine until we could move so many units. First it was 2,000, then 5,000, 10,000, 20,000, … and with the latest update up to 30,000."**
> — Jesús Arribas, GamesBeat, 2018-09-12

**The shape of that ladder matters more than the endpoint: 400 → 2,000 → 5,000 → 10,000 → 20,000 → 30,000.** There was no single architectural trick. It was ~75× won incrementally by removing per-entity overhead.

### The physics library is a *forked free library* `[PRIMARY]` — the most under-reported fact

> **"Encontramos una que funcionaba bien como base pero que necesitaba una enorme optimización. Por si sola, podía mover 200. Nosotros conseguimos que la nuestra modificada moviera 20.000"**
> *("We found one that worked well as a base but that needed enormous optimisation. By itself, it could move 200. We got our modified one to move 20,000.")*
> — Jesús Arribas, Xataka, 2018-05-29

Journalist's framing in the same piece: *"La búsqueda de la horda perfecta comenzó trasteando con librerías gratuitas de físicas"* — "the search for the perfect horde began by messing with **free physics libraries**."

**Which library: NO PRIMARY SOURCE.** Arribas never names it. Do not guess.

### Graphics API `[SECONDARY]`, well-evidenced

The game uses **SlimDX**, a managed .NET wrapper over DirectX:

- Install dir ships `InstallSlimDX.exe` and `_CommonRedist\DirectX\Jun2010\SlimDXRuntime_x64.msi`.
- Numantian's own pinned troubleshooting post instructs users to install "DirectX Runtime, SlimDX Runtime, and Visual C Libraries from the game's `_CommonRedist` folder" → **developer-acknowledged dependency** `[PRIMARY]`.
- Startup failures surface as `SlimDX.dll` load errors and `"Error in Init Renderer in Platform.Init"` / `DXSlim - InitRenderer`.

**Not MonoGame. Not XNA. Not SharpDX.** No trace of those assemblies in any file listing found.

**Direct3D version: never stated.** The Steam minimum spec says **DirectX 9.0c and shader model 3**, which strongly implies a D3D9 path `[INFERENCE]`. A user asked "**DX9 really?**" in the engine thread on 2017-06-18 and **the developers did not answer** `[PRIMARY — the non-answer is itself notable]`.

### Engine internals `[SECONDARY]`, reverse-engineered from a crash trace

A .NET stack trace posted on the *Lords of Xulima* Steam forum exposes the namespaces:

```
ICSharpCode.SharpZipLib.Zip.ZipFile.ReadEntries()
DXVision.Serialization.ZipResourceManager..ctor
DXVision.DXProject.LoadFromFile(String name)
JXApp.Windows.JXFMain.FMain_Load(Object sender, EventArgs e)
System.Windows.Forms.Form.OnLoad()
```
— user Hubmeister✚, 2020-04-11 · https://steamcommunity.com/app/296570/discussions/0/2259060348509511383/

- **`DXVision`** is the engine namespace. **No developer has ever called it that publicly** — this is a namespace read off a crash log, not a product name.
- `DXVision.dll` and `DXPlatform_Desktop.dll` ship in **both** *Lords of Xulima* (2014) and *They Are Billions* → confirmed shared engine lineage.
- Rendering is hosted in a `System.Windows.Forms.Form` — a classic SlimDX desktop-app pattern.
- Numantian's own troubleshooting post names `DXVision.dll`, `DXPlatform_Desktop.dll`, `ICSharpCode.SharpZipLib.dll`, `Steamworks.NET.dll` as engine components `[PRIMARY for the filenames]`.
- Bundled redist is **.NET Framework 4.6.2** (`NDP462-KB3151800`), and both `TheyAreBillions.exe` and `TheyAreBillions_x86.exe` ship → **32- and 64-bit builds both exist** `[INFERENCE — strong]`.

---

## A.2 Pathfinding — **per-unit, not flow fields** `[PRIMARY]`

This is the single most misreported fact about the game, and the developers settled it in writing.

> **"The game engine is ultra optimized in every aspect, especially the pathfinding, which is one of the most costly operations."**
> **"In many other games with big armies, the paths are computed for a full group of units and generally those paths don't change with time as the environment is static and fixed. However, on TAB every unit computes its own paths to reach its targets. This path is recomputed every few seconds to react to the changes in the environment (walls destroyed, structures built…)."**
> — Numantian Games, 2018-06-07 · https://numantiangames.com/News/2018-06-07-they-are-billions-development-update-v-0-8-2/

The optimisation shipped in 0.8.2 was **dirty-flag invalidation**: paths are recalculated *"only if something that can affect the current paths is changed"* — and for infected specifically, *"if a wall in their path is destroyed or built the path won't be recomputed"*, because infected do not route around walls unless directly blocked. Corresponding changelog line: **"Pathfinding Performance improved for zombie swarms."**

**The specific algorithm (A\*, Dijkstra, HPA\*, JPS) is never stated.** `NO PRIMARY SOURCE`

> ⚠️ **The "TAB uses flow fields" claim traces to a Unity tutorial by Code Monkey** (https://unitycodemonkey.com/howtomakegame.php?i=theyarebillions) which teaches how to build *something like* TAB in Unity. It is not a description of TAB's engine. `[SECONDARY — misattributed]`

---

## A.3 Zombie AI — flocking *is* the perception LOD `[PRIMARY]`

From the Xataka interview, translated:

> **"Se trataba de conseguir recrear un comportamiento como el que puedes ver de [esa escena tan chula en 'Guerra Mundial Z'], la de la horda de Jerusalén. Y eso significa hacer que los zombis perciban."**
> *("It was about recreating a behaviour like that cool scene in World War Z, the Jerusalem horde. And that means making the zombies perceive.")*

Zombies **see** and **hear** at long distances, and their **excitation level** changes after tasting human flesh. Then the cost admission — **the key performance sentence in the whole corpus**:

> **"Pero claro, si esto lo tienes que hacer para cada zombi en cada instante, que compruebe todas estas cosas, no hay quien lo mueva."**
> *("But of course, if you have to do this for every zombie at every instant — have it check all these things — nobody can make it run.")*

And the trick:

> **"Si un zombi ve a un superviviente a lo lejos y va a por él, los otros lo siguen. Lo mismo pasa si una horda avanza en una dirección. Los que se encuentren por el camino se sumarán a ella."**
> *("If a zombie sees a survivor in the distance and goes for him, the others follow him. The same happens if a horde advances in a direction. Those it meets along the way will join it.")*

The journalist describes this as *"un movimiento de manada análogo al de una bandada de pájaros"* — **herd movement analogous to a flock of birds**, i.e. explicit flocking/boids. Also:

> **"Un zombi no avanza diciendo, mejor me voy a la colonia que me sale más rentable. No. Avanza hacia la presa que tiene más cerca."**
> *("A zombie doesn't advance saying, better I go to the colony that's more profitable. No. It advances toward the prey it has nearest.")*

**Architecture reading `[INFERENCE — strongly supported]`:** a two-tier system. A minority of zombies run real perception queries and produce goals; the majority propagate those goals by **following neighbours**. Combined with A.2, that gives: **shared/emergent goal selection, individual pathfinding.**

**AI tick rate vs render rate: never stated. AI LOD: never stated.** `NO PRIMARY SOURCE`

One real timing detail leaks from the v1.0.6 changelog `[PRIMARY]`: units were wasting shots because multiple units decided their actions at exactly the same time; the fix was **a small random offset (<1 ms) added to AI reaction time**. This reveals that **AI decisions are timestamped in real milliseconds and jittered** — AI scheduling is wall-clock driven, not lockstep-tick driven `[INFERENCE — strong]`.

---

## A.4 Threading `[PRIMARY]`

> **"Hi, it scales even in our best computer with 12 cores :) Nevertheless the scaling is not perfect as some part of the code has to be 'linear' (GPU drawing, critical sections...) but the parallel part scales perfectly (AI, Pathfinding, game logic...)."**
> — Numantian Games `[developer]`, 2017-09-19

> **"The problem with the OSX and Linux versions is not just the compatibility with Mono platform, but also getting the same performance as the .NET version. The game is heavily optimized and multithread and we know by our experience with LoX that Mono is much slower in that regard compared to .NET."**
> — Numantian Games `[developer]`, 2017-08-19

Console-era restatement: **"this game uses CPUs and multicore super intensive"** (PS4/Xbox announcement, 2019-07-05).

**Confirmed parallel:** AI, pathfinding, game logic. **Confirmed serial:** GPU drawing, critical sections. **Verified to 12 cores** on their own hardware.

**Never stated:** thread count, job system vs `Parallel.For` vs manual pool, partitioning scheme, sync primitives, whether sim state is double-buffered.

> ⚠️ **Correction to a widely-circulated misquote.** The line *"custom mem management code to avoid creating and cleaning up a lot of objects"* is **not a developer statement** — it was written by forum user "Awac" on 2017-09-13, speculating about GC avoidance. **Numantian has never said anything about GC strategy, object pooling, or memory layout.** `[UNVERIFIED]`

---

## A.5 No fixed timestep, no determinism `[PRIMARY]`

> **"Depends on the CPU speed to process the physics of the game and also depends on the amount of elements on the map. There is default speed to be playable on super high speed PCs but the speed gets balanced to be also playable in slower PCs, because every PC has its own speed. There is no way for players to control the game speed."**
> — Gomez (Officer, Numantian Games), 2019-04-11 · https://steamcommunity.com/app/644930/discussions/0/3022387599786311865/

**Simulation rate is coupled to hardware. A faster CPU literally makes the game run faster in wall-clock terms.** Community measurement in that same thread found roughly a **50% speed disparity between streamers' machines**, demonstrated by video timestamp analysis `[SECONDARY, but the dev reply validates the premise]`.

Corroborated by the shipped graphics option **"Fast Timers Optimization"**, which forces the game to preserve the in-game-time to real-time ratio rather than letting it sag under load `[PRIMARY — it is a shipped setting]`. Players report it *"may cause path finding issues, make units sometimes ignore orders"* on slower machines `[SECONDARY]`.

⇒ **Variable timestep, wall-clock driven, no lockstep determinism, no replay determinism.** Consistent with the <1 ms AI jitter in A.3 and with the game being single-player only. `[INFERENCE — strong]`

**Save format** `[SECONDARY, community reverse engineering]`: `.zxsav` + `.ZXCheck`, a ZIP with legacy PKZip password encryption containing JSON; rules in `ZXRules.dat` (also a password-protected ZIP); config in XML carrying .NET type names like `System.Drawing.Size, System.Drawing`. Sources: https://github.com/ash47/TheyAreBillionsModKit , https://github.com/DaneelTrevize/TABSAT

---

## A.6 The published performance numbers `[PRIMARY]`

| Claim | Source | Date |
|---|---|---|
| **"up to 20,000 units in real time"** | Store / official site copy | 2017– |
| **"up to 30K units on a icore7 processor"** | Steam dev reply | 2017-06-14 |
| Unity managed only **~400 units** | Arribas, GamesBeat | 2018 |
| Stock physics lib **200**; their fork **20,000** | Arribas, Xataka | 2018-05-29 |
| **"Few games can boast of handling 30K units in real time, each with fully independent AI."** | PS4/XB1 announcement | 2019-07-05 |
| **"swarms of 30,000 units, each one independent, moving with its own AI, listening, interacting with the environment"** / "the most optimized version of TAB across all platforms. It's pure engineering!" | Switch 2 announcement | 2026-01-22 |

The marketing number (20,000) and the engineering number (30,000) have always differed. The claim that 20,000 is *"a soft lock to make sure all computers can run the software"* is attributed to Arribas by Alienware Arena but **could not be verified at source** — `[UNVERIFIED]`.

**ms/frame, target framerate, profiler numbers: never stated, ever.** `NO PRIMARY SOURCE`

### System requirements `[PRIMARY — Steam store page]`

|  | Minimum | Recommended |
|---|---|---|
| OS | Windows 7/8/10 (32 & 64-bit) | Windows 7/8/10 (64-bit) |
| CPU | **2-core @ 2 GHz** | **4-core @ 3 GHz** |
| RAM | 4 GB | 8 GB |
| GPU | Intel HD3000 / shader model 3, 1 GB VRAM | Radeon 7950+ / GTX 670+, 4 GB VRAM |
| API | DirectX 9.0c | DirectX 9.0c |
| Display | 1360×768 | **4K (3840×2160)** |

The min spec is generous on GPU and stingy on CPU cores — the whole architectural story in one table. **A GTX 670 recommendation next to a 4-core CPU requirement, for a game whose devs say it scales to 12 cores, tells you exactly where the budget goes.**

### Why it is CPU-bound `[PRIMARY]`

Game speed **"depends on the CPU speed to process the physics"**; **"this game uses CPUs and multicore super intensive"**; pathfinding is **"one of the most costly operations."** Nothing in the entire corpus ever describes a GPU bottleneck.

### The late-game slowdown

Numantian never published a root-cause analysis. What exists is a changelog trail `[all PRIMARY]`:

- v0.3.13 — "Fixed: Building structures only finished when they were on Screen." *(an off-screen simulation bug — the one hint that on-screen/off-screen was ever a distinction in the sim, and it was fixed as a bug, i.e. the sim is **not** view-culled)*
- v0.5.2 — "Optimized overall performance."
- v0.6.1 — "General game performance improved specially when huge swarms are attacking the colony the game is less laggy."
- v0.8.2 — "Pathfinding Performance improved for zombie swarms."
- v0.9.2 — "Fixed bug that could cause frame drops after some time playing the game."
- v1.0.5 — **"FIXED: Memory leak in sound library which made the game use much more RAM than needed and leads to random crashes and freezing."**
- v1.0.19 — "Improved performance when building Tesla Towers in big cities."

**There is no developer statement admitting a systemic architectural limit on entity count.** The community claim "the TAB engine has too many entities for it to handle" is a player's diagnosis. `[SECONDARY]`

---

## A.7 What is *not* known about TAB — do not let anyone fill these in

1. Engine name (`DXVision` is a namespace off a crash trace, not a stated name)
2. Which physics library they forked
3. The pathfinding algorithm
4. Spatial partitioning structure — **never stated at all**
5. Thread count / job system design
6. AI tick rate vs render rate; AI LOD
7. Sprite pipeline: pre-rendered 3D?, direction count, atlas sizes, palette tricks — **a dev was asked directly and did not answer**
8. Draw-call batching / sprites per draw call — **zero sources of any kind**
9. Actual Direct3D version
10. ms/frame or target framerate
11. Fog of war implementation — **zero sources**
12. Tile pixel dimensions, chunking
13. GC strategy / object pooling / memory layout
14. Map size — `[SECONDARY]` community consensus is **256×256 tiles**, implied by a Nexus mod that adds 128/256/512 options; asked directly, Gomez replied only "This is an accepted suggestion for the next Map Editor" — **a non-answer**

**This is why Part B exists.** TAB alone cannot tell you how to build this. The engineers who *did* publish — Emerson (Supreme Commander 2), Cheng (Age of Empires IV), Reynolds (PS3 boids), Collin (Battlefield 3), Pritchett (AoE IV threading) — tell you far more.

---

# PART B — The primary-source technique literature

## B.0 The numbers table — read this first

| Number | Meaning | Source |
|---|---|---|
| **65,000 agents @ 45 fps sim / 31 fps sim+render** | GPU crowd, continuum global + local avoidance, Radeon HD 4870 (2008) | Froblins, SIGGRAPH 2008 |
| **100,048 agents @ 43.66 ms/frame** | Position-based crowd, 6 iterations, GeForce GT 750M (2013 laptop GPU) | Weiss et al. 2017 |
| **15,000 agents @ 60 fps** | PS3 boids, 6 SPUs, **5-nearest-neighbour cap**, steering every **10th** frame | Reynolds, PSCrowd 2006 |
| **5,000 agents / 8 ms / 8 Xeon cores** | ORCA's own best published figure | van den Berg et al. 2009 |
| **20,000–30,000 zombies** | shipped, C#/.NET + native C++ physics | Numantian |
| **~400 units** | what Unity capped Numantian at before they wrote their own engine | Arribas, GamesBeat |
| **1,600 units on a 1024×1024 grid** | AoE IV's actual budget (8 players × 200) | Cheng, GDC 2022 |
| **0.247 / 0.376 / 1.11 ms** | AoE IV **flow-field** time for 1 / 10 / 200 units → **200× units = 4.5× cost** | Cheng, GDC 2022 |
| **0.008 / 0.044 / 0.561 ms** | AoE IV **steering** time for 1 / 10 / 200 units → **200× units = 70× cost** | Cheng, GDC 2022 |
| **15,000 objects culled in 0.32 ms** (4 jobs, 2.66 GHz Core i7) | SoA + SIMD brute force, **3× faster and 1/5 the code** vs their old cull tree | Collin, DICE, GDC 2011 |
| **25,000 batches/sec @ 100% of a 1 GHz CPU** | the draw-call ceiling. Budget = `25k × GHz × pct / fps` | Wloka, NVIDIA, GDC 2003 |
| **~200 cycles** L2 miss vs **~3** cycles L1 multiply; **10:1** wait-to-work | why SoA matters | Acton, CppCon 2014 |
| **22 ms → 3.3 ms** on an 11,111-node update loop | measured AoS→SoA + prefetch on PS3 | Albrecht, Sony, GCAP 2009 |
| **maxNeighbors = 10** (RVO2 demos), **6** (Detour, hard-coded), **5** (PSCrowd) | nobody who shipped at scale used an uncapped neighbour query | §B.4 |
| **10 ms** | timestep above which explicit power-law separation forces go unstable — **your frame is 16.7 ms** | Karamouzas et al. 2017 |
| **57 µs/frame, 48 bytes/entity** | cost of a full budget-driven AI LOD manager | Sunshine-Hill, Game AI Pro |
| **~25 µs vs ~150 µs (≈100:1)** | AC Unity low-res vs puppet bulk cost per agent | Cournoyer, GDC 2015 |
| **>1000 tasks, up to 60,000 tracked accesses per sim tick** | AoE IV's parallel deterministic sim | Pritchett, GDC 2022 |
| **390% / 734% / 1147%** | false-sharing penalty at 2/3/4 threads on one cache line | Drepper 2007 |

---

## B.1 Flow-field pathfinding

### B.1.1 Emerson — "Crowd Pathfinding and Steering Using Flow Field Tiles" `[PRIMARY]` ★ canonical

Elijah Emerson (Gas Powered Games, *Supreme Commander 2*), *Game AI Pro* Vol. 1 Ch. 23, 2013, pp. 307–316.
http://www.gameaipro.com/GameAIPro/GameAIPro_Chapter23_Crowd_Pathfinding_and_Steering_Using_Flow_Field_Tiles.pdf

**World layout:**
> "the world is broken up into individual sectors containing grid squares, where each grid square is **1 × 1 meter** and each sector holds **10 × 10** grid squares."

**The three fields, with exact bit widths:**
- **Cost field** — "an **8-bit** field containing cost values in the range **0–255**, where **255** … represent[s] walls, and **1–254** represent the path cost." Empty sectors share one global "clear" field: *"In Supreme Commander 2, we had roughly **50–70%** of the pathable space marked as clear."*
- **Integration field** — "a **24-bit** field where the first **16 bits** is the total integrated cost … and the second **8 bits** are used for integration flags such as 'active wave front' and 'line of sight.'"
- **Flow field** — "**8-bit** fields with the first **four bits** used as an index into a direction lookup table and the second four bits as flags."

**Integrator — eikonal, not plain Dijkstra:**
> "The integrator takes the initial wave front and integrates it outward using an **Eikonal equation**."
> Wavefront rule: "make sure your wave front **stops when it hits previously integrated results** … If you don't do this, you risk having wave fronts bounce back and forth."

**The LOS pass — highest-value trick in the chapter:**
> "When an agent is within the LOS it can **ignore the Flow field results altogether** and just steer toward the exact goal position. Without the LOS pass, you can have **diamond-shaped flow directions** around your goal due to the integrator only looking at the four up, down, left, and right neighbors."
> Corners cast **Bresenham** shadow lines flagged "Wave Front Blocked": "we only integrate locations that are **not visible from the goal**." And: "the LOS first pass is **very cheap** because it does not look at neighboring cost values."

**★ Amortisation — the direct answer to "how do you spread field regeneration over frames":**
> "All of this is done by marking things dirty and rebuilding them based on a **priority queue, where each item in the queue is given a time slice of a fixed number of milliseconds**."
> "You can enforce low CPU usage by **capping the number of tiles or grid squares you commit to per tick**. You can also easily spread out integration work **across threads** because the Integration Field memory is separate from everything else."

**Cache keyed on portal, not goal — why it scales:**
> "The flow field cache contains all of our built flow fields, each with their own **unique ID based on the portal window they take you through**. In this way, work can be **shared across path requests despite having different goals**."

**Anti-popping when flow directions change:**
> "we recommend **storing off a path direction vector and blending in new flow directions as you cross grid squares**."

**Local resolution is physics, never replanning:**
> "agents could **push each other around** as well as slide along walls using physics." The failure mode avoided: "rebuilding a path every time there is a collision turns into a **compounding problem** … causing the game to **grind to a halt**."

**★ And his Future Work list contains, verbatim, your game:**
> "Support multiple goals. **Multiple goal flow fields are perfect for zombies chasing heroes.**"

### B.1.2 Cheng — "Pathing in Age of Empires IV" `[PRIMARY]` ★ the modern shipped version, with timings

Frank Cheng, Lead Navigation Engineer, World's Edge. GDC 2022. Cites Emerson directly.
https://media.gdcvault.com/GDC+2022/Speaker+Slides/Pathing+In+Age_Cheng_Frank+2022-03-29+00.16.38.pdf

**Requirements:** "Max 8 players with 200 units each. **1600 Units**" · "**1024 × 1024 Grid**" · dynamic environment · formation movement.

**Why not A\*:** "Treat all the units as A\* obstacles. Recompute every frame. **Too expensive.**" And with avoidance steering, "**No guarantee of a clear path back to the waypoint list.**"

**★ FMM over Dijkstra:**
> "Basic **8-neighbor Dijkstra** Distance integration only gives **16 directions**. Causing unnecessary turns." → "**FMM** provides a smoother gradient." → "An **8-bit array stores 256 possible directions**."

Independently corroborated by Froblins: *"Solving the continuous eikonal equation by using **Dijkstra's method on a discrete grid will not converge**; we will always get **stair-stepping artifacts** regardless of the number of times you refine the grid."*

**LOS integration, 4 steps verbatim:** "1) **Breadth-First-Search (BFS)** to iterate through the visible area. 2) Detect Impasse corners and draw '**shadow lines**' from the goal to the corner … 3) Line-of-Sight integration **terminates at the shadow lines**. 4) Starting at the shadow lines, use **FMM integration** for shaded area." Header: "**Faster and more accurate than FMM**."

**★ Three amortisation designs, with their stated trade-offs:**
- **Perfect Flow** (whole path from destination): "costly for paths that are **20, 30 tiles long**" · "Units don't always follow the path to the end" · "**Not Cache Friendly**."
- **Single Tile Flow**: "generate a single tile at a time **as the unit moves onto new tiles**" · "**Cache Friendly** … **reused as building blocks**" · but "**Poor Accuracy**."
- **★ Overlapping Segmented Flow** (shipped): "Generate Further Upstream … **Overlap the parent tile with new segments**" → "**Near Optimal Proximation**" · "Short segments ensure **minimal impact to the simulation cost**" · "**Compute As Needed**" · "**Cache Friendly**."

**★ MEASURED PER-FRAME COST — the most important shape in this entire dossier:**

| Unit count | 1 | 10 | 200 | scaling |
|---|---|---|---|---|
| **Flow-field time (ms)** | 0.247 | 0.376 | **1.11** | 200× units → **4.5×** cost |
| **Steering time (ms)** | 0.008 | 0.044 | **0.561** | 200× units → **70×** cost |

**Flow-field cost is sublinear in agent count (the field is shared). Steering cost is near-linear (per-agent). At 20k–50k agents the navigation field is effectively free and separation/steering is your entire budget.**

Their own stated cons: "Extra cost if the path is only used by a **single unit**. Extra recompute cost when **terrain changes**."

### B.1.3 Treuille, Cooper, Popović — "Continuum Crowds" `[PRIMARY]`

SIGGRAPH 2006, ACM TOG 25(3):1160–1168. https://grail.cs.washington.edu/projects/crowd-flows/continuum-crowds.pdf

**★ Why it scales — cost is per-CELL, not per-agent:**
> "The computational cost of our algorithm **depends on the number of grid cells** used to compute the dynamic potential."

Unit cost field (Eq. 4): **C ≡ (α·f + β + γ·g) / f**, with f = speed field, g = discomfort field. Then the **eikonal equation** (Eq. 5):
> "‖∇φ(x)‖ = C … A theorem from the calculus of variations … guarantees that **all optimal paths follow exactly the gradient of this function**."

Agents move by **ẋ = −f(x,θ)·∇φ/‖∇φ‖**. Solver:
> "we have chosen [the **fast marching method**], since [fast sweeping] **becomes inefficient when optimal paths must follow circuitous routes**."
> "A **heap** … gives the algorithm its **O(N log N)** running time, where N is the number of grid cells."

**★ Separation is a post-pass — and note how closely this matches your design:**
> "we enforce a pair-wise minimum distance between the people. We simply iterate over all pairs within a threshold distance, **symmetrically pushing them apart** … This procedure **does not strictly ensure that minimum distances are preserved** … Note that minimum distance enforcement has **linear time complexity if the crowd is first 'binned' into a high-resolution neighbor grid.**"

> ⚠️ **REALITY CHECK — this paper did not run at 60 fps.** "All simulations ran on a **3.4 GHz Pentium**… Simulation updates took between **2 and 5 frames per second**." Cite it for the maths and the per-cell scaling insight, **never as a real-time result**.

**Stated limitation to design around:** "we do not take into account visual occlusions … effectively assuming that people **really know the dynamic properties of the environment**."

### B.1.4 Shopf, Barczak, Oat, Tatarchuk — "March of the Froblins" `[PRIMARY]` ★ 65k agents

AMD, SIGGRAPH 2008 *Advances in Real-Time Rendering* Ch. 3.
https://advances.realtimerendering.com/s2008/SIGGRAPH2008%20-%20March%20of%20the%20Froblins.pdf

**★ The architecture statement — the LOD principle for your whole sim:**
> "By combining a **continuum-based global path planner** with a **fine-grained agent-based local avoidance model**, we can perform **expensive global planning at a coarse resolution and lower update rate** while the local model takes care of avoiding other agents and nearby obstacles **at a higher frequency**."

**Results:** "**65,000 agents** at real-time frame rates on a **single commodity GPU**" — "Simulation (global and local) alone for **65K agents** … is **45 fps**. Simulation … **along with rendering** … is **31 fps**." (Radeon HD 4870, 2008.)

**★ Eikonal cost:** "Our GPU based solver computes a **256²** solution in **20 ms** which is **faster than our CPU implementation by a factor of approximately 45**."
⇒ `[INFERENCE]` a full 256² eikonal on a 2008 CPU ≈ **900 ms**. Even granting 20× for modern hardware that is ~45 ms. **You cannot afford a full-map eikonal per frame on CPU** — which is exactly why Emerson tiles it 10×10 and AoE IV segments it.

**★ Their own limitations — read as warnings:**
> "scenarios arise where **agents can deadlock and will become stuck. This typically happens at sinks in agent navigations such as at a small goal.** Once agents become densely packed around a goal, agents that reach the goal will be **unable to navigate out of the goal area**. This could be solved by incorporating **varying levels of aggressive behavior** into agent movement that causes agents to **push each other out of the way**."
> "Using a small discrete set of local directions for navigation can also lead to **oscillation between two directions**, creating distracting behavior."

### B.1.5 Pentheny — flow-field complexity claim `[PRIMARY]`

*Fieldrunners 2*, Game AI Pro Ch. 24:
> "Compared to traditional pathfinding methods where the time complexity is **linear with respect to the number of units**, this approach is **constant with respect to the number of units simulated**."

Compression: store each vector as **a rotation from the north basis vector**, or **one byte per cell** for cardinal-only.

**Congestion maps** (Game AI Pro 2 Ch. 17) add crowd density as an **additive non-negative** A\* cost, preserving admissibility — with the key subtlety: *"Crowd density alone could be interpreted as a traversal cost; however, this would cause **agents moving together at a uniform velocity to unnecessarily avoid each other**."*

> **Gap:** no primary Uber Entertainment / Planetary Annihilation source was obtainable (forums return 403). Treat "PA uses flow fields" as `[UNVERIFIED]`.

---

## B.2 Data-oriented design

### B.2.1 Acton — "Data-Oriented Design and C++" `[PRIMARY]`

Mike Acton, Engine Director, Insomniac Games. CppCon 2014 keynote.

**The cycle-cost table, verbatim:**
> "2 × 32-bit read; same cache line = **~200**" · "Float mul, add = **~10**" · "Sqrt = **~30**" · "Mul back to same addr; **in L1** = **~3**"
> → "**Time spent waiting for L2 vs. actual work ~10:1**"

**The AoS-vs-SoA arithmetic, verbatim:**
> "**12 bytes** × count(32) = **384** = **64 × 6**" *(AoS: 6 cache lines)* · "**4 bytes** × count(32) = **128** = **64 × 2**" *(SoA: 2 cache lines)*
> "**Using cache line to capacity = 10× speedup**" · "Waste **60 bytes / 64 bytes** → **90% waste!**"

**Principles, verbatim:**
> "The purpose of all programs … is to **transform data from one form to another**."
> "**If you don't understand the hardware, you can't reason about the cost of solving the problem.**"
> "Rule of thumb: **Where there is one, there are many.**"
> "Rule of thumb: Store **each state type separately**. Store **same states together**."
> "**Solve for the most common case first, Not the most generic.**"

### B.2.2 Albrecht — "Pitfalls of Object Oriented Programming" `[PRIMARY]` ★ measured

Tony Albrecht, SCEE R&D, GCAP 2009. PS3, profiled with Sony Tuner.
https://harmful.cat-v.org/software/OO_programming/_pdf/Pitfalls_of_Object_Oriented_Programming_GCAP_09.pdf

The test is **an 11,111-object update loop** — structurally your agent tick.

| Stage | Time |
|---|---|
| Original (naive OO traversal) | **~22 ms** |
| Contiguous allocation ("**35% faster just by moving things around in memory!**") | **12.9 ms** |
| + data & code reorganisation (homogeneous sequential) | **4.8 ms** |
| + `dcbt` prefetch | **3.3 ms** |

**≈ 6.6× end-to-end on an 11k-entity update loop.**

Supporting counters: "L2 Cache misses: **36,345** @ **400 cycles** each ~= **4.54 ms**" → after: "**16,064**". "**1980: RAM latency ~ 1 cycle** / **2009: RAM latency ~ 400+ cycles**."

**★ The dirty-flag lesson, which contradicts intuition:** "If m_Dirty=false then we get branch misprediction which costs **23 or 24 cycles**" but "Calculation of the world bounding sphere takes only **12 cycles**" ⇒ "**using a dirty flag here is actually slower than not using one**."

Rules: "Optimise for data first, then code." · "Keep code and data homogenous … **Don't test for exceptions – sort by them.**"

### B.2.3 Unity DOTS `[PRIMARY, vendor]`

- Chunks: "Each chunk consists of **16 KiB**" · "A chunk contains **an array for each component type**" · "The arrays of a chunk are **tightly packed**."
- Burst: "uses **LLVM** to translate .NET IL to code that's optimized for … the target CPU architecture." **Unity publishes no numeric speedup figure** — do not cite one.
- Job safety: "The C# Job System solves this by **sending each job a copy of the data** … This copy **isolates the data, which eliminates the race condition**."
- `[ReadOnly]`: lets a job "execute … **at the same time as other jobs that also have read-only access**." Safety checks are "**only available in the Unity Editor and Play Mode**" — zero cost shipped.

### B.2.4 DICE — SoA + SIMD on a real 15k loop `[PRIMARY]`

Daniel Collin, GDC 2011 (*Battlefield 3* / Frostbite 2):
> "Rearrange the data from AoS to SoA … **Now we only need 3 instructions for 4 dots!**"
> "**Linear arrays scale great** / Predictable data / Few branches / Uses the computing power"

---

## B.3 Spatial partitioning

### B.3.1 Teschner et al. — "Optimized Spatial Hashing" `[PRIMARY]`

Teschner, Heidelberger, Müller, Pomeranets, Gross (ETH Zurich), VMV 2003.
https://matthias-research.github.io/pages/publications/tetraederCollision.pdf

> "hash(x,y,z) = ( x·p1 xor y·p2 xor z·p3 ) mod n where p1, p2, p3 are large prime numbers, in our case **73856093**, **19349663**, **83492791**"

> ⚠️ **Do not repeat the "three primes" claim.** **p2 = 19349663 is not prime: 19349663 = 41 × 471943.** Harmless in practice (the constants only need to mix bits), but the error has propagated into many codebases.

**★ Cell size — the measured optimum:**
> "The measurements … indicate that **a grid cell should have the size of the average edge length** of all tetrahedrons to achieve optimal performance."
> "**the grid cell size has a more significant impact on the performance than hash table size or hash function.**"

**★ The zero-clear trick — directly usable in a no-alloc sim:**
> "Our implementation of the hash table **does not require a re-initialization in each simulation step** … each simulation step is labeled with a unique **time stamp**. If the first pass stores vertices in a hash table cell with **outdated time stamp**, the time stamp is updated and the cell is reset before new vertices are inserted."

**Complexity:** "If the cell size is chosen to be proportional to the average tetrahedron size … the time complexity of the algorithm turns out to be **linearly dependent on the number of primitives**."

### B.3.2 Ericson, *Real-Time Collision Detection* Ch. 7 `[PRIMARY]`

**★ THE CELL-SIZE RULE (§7.1.1), verbatim:**
> "**cell size is generally adjusted to be large enough (but not much larger) to fit the largest object at any rotation.** This way, the number of cells an object overlaps is guaranteed to be **no more than four cells (for a 2D grid; eight for a 3D grid)**."
> "with cells and objects being almost the same size **the maximum number of objects in a cell is bounded by a small constant.** Consequently, resolving collisions within a cell using an **all-pairs** approach results in only a small number of pairwise object tests."

**★ He rebuts the "grids are hard to tune" folklore:**
> "It is sometimes claimed that it is difficult to set a near-optimal cell size … **Practical experience suggests that this is not true. In fact, for most applications although it is easy to pick bad parameter values it is equally easy to pick good ones.**"

**★ Free win — bin by AABB min-corner, not centroid (§7.1.6.1):** binning by centre "makes a total of **nine cells tested (27 cells in 3D)**"; binning by min corner means "At worst, all nine cells will still have to be checked, but **at best only four cells** have to be tested. **This makes the minimum corner a much better feature for object placement than the center point.**"

**Rebuild-from-scratch endorsement (§7.1.7):**
> "**if most or all objects move, the latter operation may turn out cheaper than trying to update the grid data structure.** Rebuilding the grid each frame also typically requires simpler data structures … Readding all objects each frame also means objects can be tested as they are added, **avoiding the potential problem of getting both A-collides-with-B and B-collides-with-A reported.**"

**★ The one named disqualifier for grids (§7.2):**
> "**The most significant problem with uniform grids is their inability to deal with objects of greatly varying sizes** in a graceful way."

`[INFERENCE]` Your agents are uniform, so the single condition Ericson names as disqualifying is absent. Note he never writes "grids beat trees for uniform objects" — that specific sentence is inference, not quotation.

### B.3.3 Measured evidence that grid beats tree

**BioDynaMo (PPoPP 2023)** `[PRIMARY]` — https://arxiv.org/pdf/2301.06984 — literally an agent-based simulation benchmarking grid vs kd-tree vs octree:
> "Simulations using BioDynaMo's uniform grid implementation are up to **191× faster than the kd-tree implementation** while consuming only **11% more memory** in the worst case."
> **★ Their stated reason:** "We exploit the fact that **the interaction radius is known at the beginning of the iteration.** For this **fixed-radius search problem, a grid-based solution is a good choice because the box of an agent can be determined in constant time**."
> **★ Their storage layout — exactly the flat cell-start design you already have:** "**All agents inside a box are stored in an array-based linked list. The box only needs to store the start index and the number of elements it contains.** To avoid zeroing all boxes …, we add a **timestamp** attribute … we can build the grid in **O(#agents)** time instead of O(#agents + #boxes)."
> **★ The scaling that matters:** "With one thousand agents, the execution time for one iteration is on average **1.21 ms** and increases only slightly until **10⁵ agents (2.80 ms)**."
> Morton vs Hilbert: "a **negligible performance improvement of 0.54%** … Therefore, we use the Morton order because it results in simpler code."

**Franklin, NEARPT3 (IEEE TVCG 2006)** `[PRIMARY]`:
> "NEARPT3, which uses a uniform grid, appears to be **the only method that enthusiastically rejects hierarchical data structures** … their **θ(lg N) query time makes them much slower** … where NEARPT3's query time is **θ(1)**."
> vs ANN (kd-tree) at 10M points: **4 µs vs 10 µs** query, **105 MB vs 916 MB** memory.

**PySPH** `[PRIMARY]` — the cleanest statement of the switchover condition:
> "**The uniform grid data structure is the most commonly used approach for NNPS** … **This method works efficiently when the support radius is constant for all particles**, but when the support radius is variable … The most common approach in these cases is to use an Octree."
> CPU structure comparison: **Spatial Hash** — "**poor cache performance**"; **Linked List** (flat head/next arrays) — "**better cache performance because of its compactness**"; **Z-Order** — "best cache performance … but requires a sort … which makes it **slower overall than the linked list**."

**Honest negatives:** Jolt justifies its 4-way quadtree purely by **SIMD width** and **does not compare to grids at all**. Box2D/Erin Catto establishes only the *cost* of trees ("A dynamic tree must be **actively balanced** … otherwise you can easily have the tree **degenerate into a very long linked list**") — **no Catto statement comparing grids to trees was found** `[UNVERIFIED]`.

### B.3.4 Cell size vs agent radius — convergent rules

| Source | Rule | Cells scanned (2D) |
|---|---|---|
| PySPH | cell = **support/query radius** | **3² = 9** |
| Green (NVIDIA) | "cell size is the same as the size of the particle (**double its radius**)" | 9 |
| BioDynaMo | cell = fixed **interaction radius** | 9 |
| Ericson | "largest object **at any rotation**" | overlaps ≤ 4; scan 9 (centroid) or **4–9** (min-corner) |
| Teschner | **average primitive edge length** (measured optimum, ratio 1.0) | — |

### B.3.5 Flat-array construction — sort + cell-start

Simon Green, NVIDIA "CUDA Particles" `[PRIMARY]`:
> "we use a grid where **the cell size is the same as the size of the particle (double its radius)**"
> "The grid data structure is **generated from scratch each time step** … **the performance is constant regardless of the movement of the particles**."
> **★ Counting-sort variant:** "**count the number of particles per grid cell** … then perform a **parallel prefix sum (scan)** to calculate the destination addresses … In the final pass we examine all the particles again, and **write them to contiguous locations**."
> **★ Reordering pays:** "we actually **re-order the position and velocity arrays into sorted order** to improve the coherence."

Hoetzlein (NVIDIA, GTC 2014): **counting sort beats radix** — "**Better to perform 1-radix on exact bins, rather than on digits**", kernel calls/frame **15 → 4**, "**5-10× faster**"; app-level "16,384 at 32 fps" (2009) → "**193,487 at 32 fps**" (2013), "**same hardware**", "**11× faster**".

---

## B.4 Local avoidance / separation at horde density

### B.4.1 ORCA / RVO2 `[PRIMARY]` — and why it is out of budget

van den Berg, Guy, Lin, Manocha, ISRR 2009. https://gamma.cs.unc.edu/ORCA/publications/ORCA.pdf

**Their headline performance:**
> "For **5,000** agents on **eight** cores, it takes **8 ms** (**125 frames per second**) to solve the collision-avoidance linear program for every agent … and **15.6 ms** (**64 fps**) … [for] the office evacuation simulation." (8× Intel Xeon 2.66 GHz, OpenMP.)

**★★ Their own three admissions of failure at density:**
> "In densely packed conditions, this may also lead to a **global deadlock**, as the chosen velocities for the robots converge to zero when the robots are very close to one another."
> "as the magnitude of the optimization velocity increases, it is increasingly more likely that the linear program is **infeasible** … this would lead to **unsafe navigation in even medium density conditions**."
> §5.3: "the set ORCA_A^τ is **empty** … choosing a collision-free velocity **cannot be guaranteed**." And: "the new velocity selected for the robot **does not depend on the robot's preferred velocity**. This means that the robot **'goes with the flow'**."

**Their shipped example values** (github.com/snape/RVO2): `Blocks.cc` → `maxNeighbors = 10`, 100 agents. `Circle.cc` → `maxNeighbors = 10`, 250 agents, timestep 0.25 s (**4 Hz**). API docs warn on both `maxNeighbors` and `neighborDist`: "**The larger this number, the longer the running time … If the number is too low, the simulation will not be safe.**"

`[INFERENCE]` Linear-scaling their own figure ⇒ **~32 ms for 20k agents on 8 cores, for avoidance alone.** ORCA is out of budget at your scale, and its guarantee evaporates exactly where a horde game lives.

### B.4.2 Reynolds — Steering Behaviors `[PRIMARY]`

GDC 1999, https://www.red3d.com/cwr/steer/gdc99/
- **Separation**: "a repulsive force is computed by subtracting the positions …, normalizing, and then applying a **1/r** weighting." — **with his own disclaimer: "1/r is just a setting that has worked well, not a fundamental value."**
- **Neighborhood**: a **distance** plus an **angle** defining a field of view (most implementations drop the angle).
- Boids page: "the straightforward implementation … has an asymptotic complexity of **O(n²)** … it is possible to reduce this cost down to **nearly O(n)** by the use of a suitable **spatial data structure**."

### B.4.3 ★★ Reynolds — "Big Fast Crowds on PS3" `[PRIMARY]` — the most actionable source in the dossier

SCEA R&D, Sandbox Symposium, July 2006. https://www.red3d.com/cwr/papers/2006/PSCrowdSandbox2006.pdf

> "supports simulation and display of simple crowds of up to **15,000 individuals at 60 frames per second**."

**★ THE NEIGHBOUR CAP:**
> "**PSCrowd considers only the 5 nearest neighbors while steering each boid.** In the 1987 version, all boids within a given neighborhood were considered."

**★ TEMPORAL AMORTISATION — the least-used trick on this list:**
> "The first two demos use a **skipThink count of 8** and the **2D crowd uses skipThink of 10**."

**Steering is recomputed every 10th frame for the 15,000-agent crowd. That is a free 10× on the dominant per-agent cost.**

**Fixed-capacity bucket lattice, not a hash map:** "**15,000** Individuals … using a Lattice divided into **2500 (50×1×50)** Buckets." · 3D: "**Each Bucket can contain up to 160 Fish**" · Rebucket "applied **once per frame** … **constant time O(1)**" (swap-with-last) · known failure mode: "**'Bucket overflow.' Simulation cannot proceed if a Bucket's storage capacity is exceeded.**"

**★ Why 2D is cheaper — directly relevant to an isometric game:**
> "in the 2D case, the Bucket neighborhood is **3×3** while in 3D it is **3×3×3**. **Three times as many** CondensedBuckets must be processed … Also, when agents with separation behavior are restricted to a 2D surface they tend to be **less densely packed per unit volume**."

Historical anchors from the same paper: 1987 — "**one hour** to simulate **one second** of **80** boids"; 1999 PS2 — "**280** boids at **60** fps"; Continuum Crowds — "**10,000** agents at **5 fps** without graphics."

### B.4.4 ★★ The stability warning — explicit forces break at 60 FPS

**Karamouzas, Sohre, Narain, Guy — "Implicit Crowds", SIGGRAPH 2017** `[PRIMARY]`
https://motion.cs.umn.edu/pub/ImplicitTTC/implicit_crowds.pdf

> **"PowerLaw model leads to collisions and other discontinuities in motion with time steps much larger than 10 ms."**
> "they suffer from **numerical stability issues**, since forces can assume large values and vary quickly."

**A 60 FPS frame is 16.7 ms. The measured-from-humans force model is already past its stability limit at your frame rate under explicit integration.** Their fix (implicit optimisation, 100 iterations/step, 400–2000 agents) is not viable at 20k–50k — **take the diagnosis, not the cure.**

**Karamouzas, Skinner, Guy — "Universal Power Law", PRL 113, 238701 (2014)** `[PRIMARY]`:
> exponent measured at "**2.05 ± 0.123**" and "**2.017 ± 0.192**"; law **E(τ) = k/τ² · e^(−τ/τ₀)**; "For smaller values of τ (less than **~200 ms**), the energy … **saturates to a maximum value**"; "**τ₀ ≈ 3 s**".
> **★ The conceptual claim:** interaction is "based **not on the physical separation** … but on their **projected time to a potential future collision**."
> **★ Justification for a small neighbour cap:** "interactions between distant, non-neighboring pedestrians are **screened by the presence of nearest-neighbors**."

### B.4.5 Position-based crowds — the only 100k data point `[PRIMARY]`

Weiss, Litteneker, Jiang, Terzopoulos, MiG 2017. https://arxiv.org/pdf/1802.02673

| Scenario | agents | ms/frame |
|---|---|---|
| Dense, high count | 10,032 | 14.06 / 13.63 |
| Bottleneck | 3,600 | 17.76 |
| **Bottleneck** | **100,048** | **43.66** |

Δt = 1/48, **6 iterations**/step, timings exclude rendering, CUDA on a **GeForce GT 750M** (2013 laptop GPU). "we use **two hash-grids**, for short and long range collisions. **This is more efficient than using one grid for both.**" Constraint: `C(x_i,x_j) = ‖x_i − x_j‖ − (r_i + r_j) ≥ 0`.

**★ Their head-to-head against the explicit power-law force model (Fig. 11):**
> "Left: In a **sparse** setting, the agents successfully avoid collisions. Right: In a **dense** setting, the agents **collide, overlap, and are not able to pass smoothly**."

They reproduce "**jamming and arching** near the corridor's entrance, as well as the **formation of pockets**" — useful as a correctness check for your own crowd.

### B.4.6 Neighbour caps in shipped code, and the RTS answer

**Recast/Detour** `[PRIMARY]`, `DetourCrowd.h`:
```c
/// The maximum number of neighbors that a crowd agent can take into account
/// for steering decisions.
static const int DT_CROWDAGENT_MAX_NEIGHBOURS = 6;
```
A **compile-time constant**, not a tunable. `dtCrowdNeighbour` is 8 bytes, so an agent's whole neighbour set is **48 bytes** — under one cache line.

**Guy & Karamouzas, Game AI Pro 2 Ch. 19** `[PRIMARY]`:
> "By selecting a **fixed maximum number of neighbors** for each agent, the runtime will be **nearly linear** in the number of agents."
> Recommended params: "a **Δt of 20 ms**"; "a moderate time horizon of **4 s**"; goal force "a **k of 2**"; and "**cap the maximum avoidance force** … (we use **20**)."
> Failure modes named: agents "can even **overlap**"; force methods produce "**distracting oscillations in an agent's velocity**."

**★ The shipped-RTS answer is asymmetric, not reciprocal:**
- **Blizzard, StarCraft II 5.0.15 patch notes** `[PRIMARY]`: "Increased **allied push priority** for **Thors** and **Siege Tanks**. Intended to assist bulky units in pathfinding when **surrounded by many small friendly units**."
- **Emerson (SupCom2)**: "agents could **push each other around** … using physics", "super large robots that could push back **a hundred** tanks."
- **Patrick Wyatt** on original StarCraft: **harvesters ignore unit collision entirely** to prevent congestion.

`[INFERENCE]` A one-way **priority/mass field** is far cheaper than ORCA and is what horde RTSs actually ship.
**Negative result:** no official Blizzard engineering writeup on SC2 local avoidance exists; forum claims about CDT navmesh + funnel are `[UNVERIFIED]`.

---

## B.5 Simulation LOD and time-slicing

### B.5.1 Cournoyer — "Massive Crowd on Assassin's Creed Unity" `[PRIMARY]`

François Cournoyer, Ubisoft Montreal, GDC 2015.

**Before:** "A CPU limit of **100 NPCs** / Limit of **20 civilians** / Around **4 around the player**"

| Tier | Band | Entity? | CPU/agent |
|---|---|---|---|
| **Autonomous** | < 12 m | yes | "Costly", **max 40** concurrent, full Havok |
| **Puppet** | 12–40 m | yes | **~150 µs**, "Most components are OFF" |
| **Low-res** | > 40 m | **no entity** | **~25 µs**, no Havok |

> "CPU Costs per bulks: Low Res **~25 us** / Puppet **~150us** … Factor **~ 100:1**"

**Scale:** 10,000 crowd NPCs on screen (typical profile 2,000). `UpdateBulkMulti` at **1.00–1.07 ms** across **5** worker threads.

**★ Their spatial index:** "Dynamic Map 2d — Spatially repeating / Keep two maps, **double buffered** / **Lockless insertion** / **No remove** / **1.5 us for a query of 1.5m radius**"

**★ Anti-popping is a swap *protocol*, not a flag flip:** "**Swapping**: Find the best matching entity / Reapply color / **Teleport to low res position** / Match the hats and props" · "**Less models means better matching**." They shipped **without** a cross-fade and listed "Dithering" under future work.

### B.5.2 Sunshine-Hill — "AI Level-of-Detail Control with the LOD Trader" `[PRIMARY]`

Ben Sunshine-Hill (Havok), Game AI Pro Vol. 1 Ch. 14.

**The thesis — distance bands are the wrong control variable:**
> "**There's no 'LOD threshold distance' we could pick which would respect our budget and give most visible characters the detail we want.**"

LOD selection becomes a **per-frame knapsack**: criticality × audacity, worked in x-space (`x = −ln(1−p)`) so independent break-in-realism probabilities **add**.

**★ MEASURED COST:**
> "its average execution time was **57 microseconds per frame**, or **0.17% of the target frame time**. Its memory usage was **500 kB** for the transition data and **48 bytes per entity**."

**★ Two features that solve problems you will have:**
- **Existence as an LOD feature** — the principled replacement for a despawn bubble.
- **Popping is first-class** — costs attach to the *transition*, and **one-way** transitions express hysteresis cleanly.

### B.5.3 ★★ Graham — the honest critique of time-slicing `[PRIMARY]`

David "Rez" Graham, Game AI Pro Online 2021 Ch. 2.

**The canonical resumable-cursor pattern (Listing 2):**
```cpp
void TimeSlicedUpdate(float deltaMs) {
    constexpr float kTimeSlice = 2.f;  // 2 ms
    static size_t index = 0;           // persist across calls
    float startTime = GetCurrentTime();
    while (GetCurrentTime() + startTime < kTimeSlice && index < simObjects.size())
    { simObjects[index].Update(deltaMs); }
}
```

**★ READ THIS BEFORE BUILDING IT:**
> "**Time-slicing reduces spikes; it doesn't improve overall framerate.**"
> "you are incurring a cost for **every object in the list**. Time-slicing doesn't magically solve performance issues; all it does is **spread out those issues across multiple frames**. If your frames are consistently too long … then **time-slicing will only hurt performance**."
> "updates will be **inconsistent** … In the extreme case, you can suffer from **starvation**."

**His preferred alternative:** a **priority queue keyed on absolute wake time**. Anything analytically predictable (cooldowns, decay, lifespans, spawn timers) should be an **event**, not a tick.

> ⚠️ Note the tension with B.4.3: Reynolds' `skipThink` is *not* time-slicing in Graham's sense. Reynolds **does less total work** (steering computed 1× per 10 frames), which is a real throughput win. Graham is warning against **the same total work spread thinner**. Make sure you build Reynolds', not the one Graham warns about.

### B.5.4 Narain, Golas, Curtis, Lin — "Aggregate Dynamics for Dense Crowd Simulation" `[PRIMARY]`

SIGGRAPH Asia 2009. Intel Core i7-965 @ 3.2 GHz. *(All live GAMMA URLs 404; recovered via Wayback.)*

| Scenario | Agents | Grid | ms/frame |
|---|---|---|---|
| Circle | 10 k | 40×40 | 34.2 |
| Mosque | 25 k | 80×80 | 88.1 |
| Campsite (Hajj) | 100 k | 120×90 | 447 |

> "our method makes it possible to simulate a crowd of **1 million agents** at **3 seconds per frame** … For comparison, we used the most recent publicly-available implementation of the **RVO** method … It **failed to run on scenes containing more than 70,000 agents**."

**★ The cost-structure insight that should drive your architecture:**
> "The computational cost of the UIC solve is approximately linear with the number of **actively constrained cells** …, but is **independent of the actual number of agents in the scene**. The other expensive step is the **pairwise collision resolution, which is an unavoidable per-agent cost**."

Note the grids are tiny — 40×40 for 10,000 agents ≈ 6 agents/cell. They deliberately over-separate ("mean of **1.25 d_min**") for "**smoother, jitter-free** motion."

**★ They propose exactly the hybrid worth building:**
> "a more expensive agent-based scheme could be used [near the viewpoint], while **distant agents would be efficiently simulated using our model on a coarser grid**."

### B.5.5 Fauerby — "Crowds in Hitman: Absolution" `[PRIMARY]`

Kasper Fauerby, IO Interactive, GDC 2012.

**★ Full PS3 frame budget:**
> "**1200 agents simulated, 500 on-screen** / **PPU: 5ms** — Animation ~2ms, Crowd AI/steering ~2ms, Framework ~1ms / **SPU: ~20ms** / **GPU: 8ms**" · "**Scales nearly linearly with number of agents**."

**★ The data layout — the most transferable slide:**
> "Size of a full agent **~256 bytes**. Separate out '**agent core**'. Stores the most basic properties: position, speed etc. **36 bytes**. … Allocate all cores as a single, **128 byte aligned**, block of memory (**1200 agents: 42kb**)."
> Cell map is **SoA**: "Map is **4 arrays**, each storing a different attribute … Usually an algorithm is only interested in one of the attributes." · Job sizing: "**30 agents per job**", working set "~93kb".

**Cheap variety:** "Unique scaling factor for each agent — small amount: **~5%**. … Does wonders for perceived diversity."
**Honest negative:** high-fidelity animation made agents "react **much slower** to steering input, which makes it **harder to avoid collisions**."

### B.5.6 Others worth knowing

- **Zubek, "1000 NPCs at 60 FPS"** — **caveat: no time-slicing and no LOD bands.** He hits the target by *eliminating* per-frame work: "we run action performance **almost entirely open-loop** … the system **only evaluates the world when it has nothing to do**." Search-space reduction beat algorithm choice: "we reduced the space from **15,000 grid cells to only 100 graph nodes with 400 edges**" — "our domain knowledge … **so drastically that the choice of algorithm no longer mattered**."
- **Mars & Chanut, "Hierarchical Architecture for Group Navigation"** — **★ the latency law**: "Group members only take into account orders computed by the group during the **previous simulation update** … the **delay grows linearly with [hierarchy] depth**."
- **Karlsson, "Squad Coordination in Days Gone"** — **★ asymmetric hysteresis**: "the Frontline can gain only a **maximum percentage of its new value per second** … **limiting short-term oscillations**" but "the larger the difference, the quicker the move." Plus **"Ghosts"** — an agent leaving the aggregate "**leaves a placeholder at its last position**" so the centroid doesn't jerk.

---

## B.6 Rendering 20k+ sprites

### B.6.1 ★★ Wloka — "Batch, Batch, Batch" `[PRIMARY]` — the draw-call ceiling

Matthias Wloka, NVIDIA, GDC 2003. https://www.nvidia.com/docs/io/8228/batchbatchbatch.pdf

**★ The number, on a slide titled "Please Hang Over Your Bed":**
> "**25k batches/s @ 100% 1GHz CPU**"

**★ The budget formula:** "**25k × GHz × Percentage / Framerate**" — worked example: "Target: 30fps; 2GHz CPU; 20% Draw/SetState: X = **333** batches/frame".

**The CPU-bound threshold:**
> "at **< 130 tris/batch** (avg) you are – completely, – utterly, – totally, – **100%** – CPU limited! • CPU is busy doing nothing, **but submitting batches**!"
> VTune at 2 tris/batch: "**78%** driver; **14%** D3D; **6%** Other"
> "Submitting X batches: **O(X)** work for CPU … Can **reduce constants but not order O()**"
> Slide title: "**300 Batches Per Frame Sucks.**"

`[INFERENCE]` **Even granting a generous 10× for 20 years of drivers and CPUs, per-sprite draw calls at 20k sprites are one to two orders of magnitude over budget. Instancing is not an optimisation here; it is the only viable architecture. Target < 50 draw calls/frame.**

### B.6.2 Instancing APIs `[PRIMARY, vendor docs]`

- **D3D11 `DrawIndexedInstanced`**: "Instancing may extend performance by reusing the same geometry to draw multiple objects … requires multiple vertex buffers: at least one for per-vertex data and a second buffer for per-instance data." Per-instance data is `D3D11_INPUT_PER_INSTANCE_DATA`. **No documented `InstanceCount` cap** beyond UINT.
- **OpenGL** `glDrawArraysInstanced` / `glVertexAttribDivisor`: "If `divisor` is zero, the attribute … advances **once per vertex**. If … non-zero, the attribute advances **once per `divisor` instances**." (GL 3.3+)
- **WebGPU**: `drawIndexed(indexCount, instanceCount, …)`; default limits `maxTextureDimension2D` = 8192, `maxTextureArrayLayers` = 256.

⇒ **One instanced call = 50,000 sprites, 1 draw call.**
`[INFERENCE]` A 2D sprite instance of pos(2) + size(2) + UV rect(4) + colour ≈ 32–48 bytes ⇒ **50,000 sprites ≈ 1.6–2.4 MB/frame** upload. Trivial.

### B.6.3 Atlases vs texture arrays `[PRIMARY]`

**NVIDIA, "Improve Batching Using Texture Atlases" (2004)**:
> "An internal survey of four DirectX9 titles reveals … **SetTexture() is one of the most common batch-breakers.**"
> **Mip pollution rule**: "A sub-texture of dimension w × h **cannot cross 'w' power-of-2 lines horizontally nor 'h' power-of-2 lines vertically**."
> **★ Bleeding**: "**While bilinear filtering of the highest resolution mip-level is thus safe, anisotropic filtering … does potentially access unrelated neighboring texels. Worse, bilinear and anisotropic filtering of all lower mip-maps also access unrelated neighboring texels.**" → "these artifacts are **not easily overcome**."
> **Half-texel rule**: u ∈ [.5/width, 1−.5/width].
> **Address modes break**: "remapping texture coordinates outside of [0,1] … **results in atlas coordinates that access neighboring textures in the atlas.**"

**Texture arrays, the constraint** (Microsoft): "**All texture arrays in Direct3D are a homogeneous array of textures; this means that every texture in a texture array must have the same data format and size (including texture width and number of mipmap levels).**"

`[INFERENCE]` **For an RTS with a fixed unit cell, a 2D texture array beats an atlas**: no bleeding, no padding, no UV remapping, working clamp/wrap per layer — at the cost of uniform frame size, which unit sprites have anyway. Use an atlas only for genuinely mixed-size art (terrain, buildings, UI), padded ≥ 4 px.

### B.6.4 ★★ Isometric depth sorting — the crux

**Khronos, OpenGL 4.6 Core Profile Spec, §7.13 p.160** `[PRIMARY]`:
> "**while fragment shader outputs are always written to the framebuffer in primitive order**, stores executed by fragment shader invocations are not."

**This is the crux. Blending is a non-commutative operator applied in API primitive order. The depth buffer only *discards* fragments; it does not *reorder* the surviving ones.**

| Approach | CPU sort? | alpha **BLEND** | alpha **TEST** |
|---|---|---|---|
| Painter's, sort by y, depth off | **YES — full 20k–50k sort every frame** | ✅ correct | ✅ correct |
| `z = f(y)` into depth buffer, no sort | **NO** | ❌ order-dependent | ✅ **order-independent** |

**Microsoft, blend state doc** `[PRIMARY]` names your exact case:
> "**Alpha-to-coverage is also traditionally used for screen-door transparency or defining detailed silhouettes for otherwise opaque sprites.**"

**★ `[INFERENCE]` RECOMMENDED ARCHITECTURE:** render units as **alpha-tested (cutout) opaque** geometry with **`z = f(y_screen)` computed in the vertex shader** from the per-instance y. This buys **zero CPU depth sort**, **arbitrary instance order** (so you sort by *atlas/material* instead), **early-Z overdraw savings**, and correct occlusion for hard-edged pixel art. Reserve real alpha-blend + back-to-front sorting for the *hundreds* of genuinely translucent things (shadows, selection rings, smoke, health bars) — not the tens of thousands.

**Ericson, "Order your graphics draw calls around!" (2008)** `[PRIMARY]` — Christer Ericson, then Director of Tools & Technology, Sony Santa Monica (*God of War III*).

The 64-bit sort key, bit 63 → 0: `Fullscreen layer 2 | Viewport 3 | Viewport layer 3 | Translucency type 2 | Depth 24 ⟷ Material ID 30` (the last two swap).
> "**making sure that categories we want to sort on first start at the MSB end** of the key."
> "**sometimes we want to sort on depth before material** … (for example when sorting translucent objects back-to-front). Other times we might want to sort on **material before depth** (typically for opaque geometry)."
> "**you probably don't need 24 bits for depth.**"
> **★ Scale**: "For current platforms like the **PS3**, there really is **no problem sorting 5,000 or even 10,000 draw calls each frame** this way."

**★ `[INFERENCE]` Isometric adaptation:** replace `Depth` with **quantised screen-space y**. Your map is bounded, so a **16-bit y is lossless**, not approximate:
```
[ layer:4 | translucency:2 | y_screen:16 | atlas_id:6 | material:4 ] = 32 bits
```
A 32-bit key halves sort bandwidth vs Ericson's 64-bit, and a lossless 16-bit key makes a **single-pass counting sort over 65,536 buckets** viable — two linear passes, zero comparisons. **You already own a counting sort** (`crates/mmd-engine/src/sim/spatial.rs`).

**Michael Herf, "Radix Tricks" (2001)** `[PRIMARY]`:
> "the **11-bit** optimization improves performance by about **40%**" · "**65536** floating-point numbers … on my **P3/600** … My mergesort achieves about **12** sorts/sec … The radix achieves **97** sorts/sec"

`[INFERENCE]` 65,536 × 97 = 6.36 M el/s ⇒ **~157 ns/element at 600 MHz** (radix ≈ 8.1× mergesort). Scaling clock alone puts a modern core well under **10 ns/element** ⇒ **50,000 sprites < 0.5 ms**. With a 16-bit integer y key you need neither the float flip nor 3 passes.

**Shipped-engine confirmation** — Unity: "Use a diagonal axis **(1, 1, 0)** so the more top-right the GameObject is in the scene, the further away it is. **This is useful for isometric games.**" Godot: `CanvasItem.y_sort_enabled`. `[INFERENCE]` Two independent engines both solve iso depth as "project to a scalar, sort by it" — **but neither targets 50,000 sprites**, which is why the depth-buffer route above deserves serious consideration at your scale.

### B.6.5 ★★ Culling 20k+ sprites — Collin, DICE `[PRIMARY]`

Daniel Collin, GDC 2011, shipped in *Battlefield 3* / Frostbite 2.

**Scene scale:** "Our worlds usually has max **~15000** objects" · "First try was to just use **parallel brute force**" · "**3× times faster** than the old culling" · "**1/5 code size**".

**★ THE TABLE — "15000 Spheres":**

| Platform | 1 Job | 4 Jobs |
|---|---|---|
| Xbox 360 | 1.55 ms | 0.52 ms |
| **x86 (Core i7 2.66 GHz)** | **1.0 ms** | **0.32 ms** |
| PlayStation 3 | 0.85 ms | 0.23 ms |
| PS3 (SPU asm) | 0.63 ms | 0.18 ms |

**15,000 frustum-vs-sphere tests in 0.32 ms — ~21 ns per object, on 2011 hardware.**

> Why they deleted the hierarchy: the old system had "DynamicCullTree scaling / Sub-levels / Pipeline dependencies / **Hard to scale**" → "**Linear arrays scale great** / Predictable data / Few branches."
> "Rearrange the data from **AoS to SoA** … Now we only need **3 instructions for 4 dots**!"
> Add/remove: "Use the '**swap trick**' … Just swap with the last entry and decrease the count."
> Cheap extra culls: "**Project AABB to screen space**"; "**If area is smaller than setting just skip it**."
> Conclusion: "**It's all about data** / Simple data often means simple code."

`[INFERENCE]` Your 20k–50k sprites are the same order, on CPUs 4–8× faster with **AVX2**, and a 2D screen-rect test is *cheaper* than a 6-plane sphere test. **Budget ~0.2–0.5 ms/frame — 1–3% of 16.67 ms.** The lesson is **not** "build a quadtree": DICE **deleted** theirs and got 3× faster with 1/5 the code.

### B.6.6 Engine caps worth knowing

- **Unity `Graphics.DrawMeshInstanced`**: "**You can only draw a maximum of 1023 instances at once**", and "Unity **does not further cull individual instances** … [and] **does not sort individual instances**."
- **Godot MultiMesh**: "can draw up to **millions** of objects in one go" · **★ the tension in one sentence**: "**there is no screen or frustum culling possible for individual instances** … millions of objects will be always or never drawn."

⇒ **The architecture: cull on CPU (SoA + SIMD) → compact survivors into the instance buffer → one instanced draw.**

---

## B.7 Fixed timestep, determinism, threading

### B.7.1 Fiedler — "Fix Your Timestep!" `[PRIMARY]`

Glenn Fiedler, 2004-06-10. https://gafferongames.com/post/fix_your_timestep/

```c
double t = 0.0, dt = 0.01;
double currentTime = hires_time_in_seconds();
double accumulator = 0.0;
while ( !quit ) {
    double newTime = time();
    double frameTime = newTime - currentTime;
    if ( frameTime > 0.25 ) frameTime = 0.25;
    currentTime = newTime;
    accumulator += frameTime;
    while ( accumulator >= dt ) {
        previousState = currentState;
        integrate( currentState, t, dt );
        t += dt; accumulator -= dt;
    }
    const double alpha = accumulator / dt;
    State state = currentState * alpha + previousState * ( 1.0 - alpha );
    render( state );
}
```

**Spiral of death:**
> "It's what happens when your physics simulation can't keep up with the steps it's asked to take."
> "being behind causes your update to simulate **more steps to catch up**, which causes you to fall further behind."
> Framing: "the renderer **produces time** and the simulation **consumes it** in discrete dt sized steps."

> ⚠️ `[INFERENCE]` The published article declares `State previous; State current;` but the body references `previousState`/`currentState` — **it does not compile as printed.** Fix the names, not the semantics.

**Deterministic Lockstep (2014):**
> "bandwidth is proportional to the size of the input, **not the number of objects** … you can network a physics simulation of **one million objects with the same bandwidth as just one**."
> "that does *not* necessarily mean it would also be deterministic across different compilers, a different OS or different machine architectures … **it's probably not even deterministic between debug and release builds**."

### B.7.2 ★ Terrano & Bettner — "1500 Archers on a 28.8" `[PRIMARY]` — the RTS source

Paul Bettner, Mark Terrano, Ensemble Studios. GDC 2001. https://zoo.cs.yale.edu/classes/cs538/readings/papers/terrano_1500arch.pdf

**Targets:** "Support for **8** players" · "**16Mb** Pentium **90** with a **28.8** modem" · "Target consistent frame rate of **15 fps**" · "**30%** graphic rendering, **30%** AI and Pathing, and **30%** running the simulation."

**★ Why commands, not state:**
> "Just passing X & Y coordinates, status, action, facing and damage would have **limited us to 250 moving units** in the game at the most."
> "run the exact same simulation on each machine, passing each an **identical set of commands**."

**★ The 2-turn pipeline:** "commands issued during turn **1000** would be scheduled for execution during turn **1002**." · "Turns were typically **200** msec in length … At any point during the game, commands were being **processed for one turn, received and stored for the next turn, and sent out for execution two turns in the future**."

**★ Latency tolerance:** "For RTS games, **250 milliseconds** of command latency **was not even noticed** — between **250 and 500** msec was very playable, and beyond **500** it started to be noticeable." · "a consistent **500** msec command latency was playable, but one that **varied** was considered 'jerky'."

**★ OOS debugging — read this before you write your checksum:**
> "very subtle differences would **multiply over time**. A deer slightly out of alignment when the random map was created would forage slightly differently — and minutes later a villager would path a tiny bit off."
> "As much as we check-summed the world, the objects, the Pathfinding, targeting and every other system — it seemed that there was **always one more thing that slipped just under the radar**. Giant (**50Mb**) message traces…"
> "programmers were not used to having to write code that used the **same number of calls to random** within the simulation."
> "the code must **not depend on any local factor** … The code path taken on all machines must match."

### B.7.3 ★★ Pritchett — "The MAW: Safely Multithreading the Deterministic Gameplay of AoE IV" `[PRIMARY]`

Joel Pritchett, Franchise Technical Director, Age of Empires, Microsoft. GDC 2022 (slides include speaker notes).
https://media.gdcvault.com/GDC+2022/Speaker+Slides/GDC22_MAW.pdf

**The parallelisation unit — "simulation islands":**
> "we approached the problem by finding '**islands**', or groups [of] entities who are **only looking amongst themselves** that we can update in a single task." · "**Plenty enough islands to spread across the cores.**"

**★ Double-buffering the whole sim state:**
> "when the presentation and simulation sync up, **all of the relevant simulation state is copied in one massive go to a second buffer** so the presentation has a **fixed view of the world** to work from while the simulation gets on generating the next simulation frame."

**★ The determinism insight — the whole reason the talk exists:**
> "if we could build a system that could catch the **nondeterministic use of an entity** …, it would **also guarantee that there won't be contention for that object** across our multiple worker threads. **We could get rid of mutexes or locking and solve our determinism at the same time.**"
> The rule enforced: "The system only ever allows **one task in a task group to have write access** to an object. If an object is write modified, then **no other task in that group is allowed to read or write** from that object." · "Any number of tasks in a group are allowed to **read** … so long as no other task ever writes."
> "**Time is irrelevant**, as it is assumed a task could overlap any other task."

**★ Results:** "**20% performance penalty in our dev builds**" · "**Up to 60,000 tracked accesses per sim tick across >1000 tasks**" · "we went from **10s per tick** of overhead to **single digit milliseconds**" · "**Callstacks alone are 80% of the current cost**" · "**The system is core count agnostic**" — same errors caught single-threaded, which they used "**a lot** to verify code could run in parallel **before** actually making it run in parallel."

Design target, worth noting: "our game was designed to run **4 player 800 unit games on a 10 year old ultrabook**."

### B.7.4 Job systems with measured scaling `[PRIMARY]`

**Gyrling, "Parallelizing the Naughty Dog Engine Using Fibers", GDC 2015:**
Spec: "**6** worker threads / Each one is **locked to a CPU core** / **160** fibers / **3** job queues / **No job stealing**" · "**~800-1000 jobs per frame**".
> ⚠️ **Correction to a common claim — it is spin locks, not lock-free queues:** "Mutex, semaphore, condition variables… **Locked to a particular thread. Fibers migrate between threads** … **Atomic spin locks are used almost everywhere**."

**★ Measured ladder:** initial port **132 ms** → jobify everything **55 ms** → "More jobs and fewer locks" **36 ms** → diminishing returns **25 ms** → new pipelined design "**15.5 ms!!! Ship it!**"
**★ FrameParams = the N-buffer pattern:** "Uncontended resource / **No locks needed as each stage works on a unique instance** … We have **16** FrameParams that we rotate between."
**Per-thread allocator:** "**2 MiB** block … one per worker thread … **No contention / No locks required for 99.9% of memory allocations**."

**Tatarchuk, "Destiny's Multi-threaded Renderer Architecture", GDC 2015:**
> "We store all dynamic data … in a **double-buffered** frame packet ring buffer. This … is **fully stateless** — it is generated each frame and thrown away."
> "**~1 MB** per frame … which is about **9%** of the total game state."
> **★ Locks measured:** "**The wall of red that you see is locks. This is awful.**" → replaced with "an **interlocked bitvector**" → "The wall of red is now gone." Caveat: "**beware of schrodinger's bugs**."

**Intel oneTBB grain size:** "A rule of thumb is that `grainsize` iterations of `operator()` should take at least **100,000 clock cycles**."
`[INFERENCE]` At 20k–50k agents with hundreds of cycles each, a grain of **~256–2048 agents/job** lands in the window. **One agent per job is pure overhead.**

> ⚠️ The widely-repeated "use a Unity batch count of 32–128" does **not** appear in any Unity page fetched. `[UNVERIFIED]`

### B.7.5 ★ False sharing — the numbers `[PRIMARY]`

**Drepper, "What Every Programmer Should Know About Memory" (2007), §6.4.1:**
> "The measured overhead, computed by dividing the time needed when using **one single cache line** versus a **separate cache line for each thread**, is **390%**, **734%**, and **1,147%** respectively." (2/3/4 threads.)
> **★ Rule 3:** "**Move read-write variables which are often written to by different threads onto their own cache line. This might mean adding padding at the end.**"

**Herb Sutter, "Eliminate False Sharing" (2009):** on **24 cores**, a naive shared result array meant "the parallel code ran actually ran **slower than the sequential code**, and in **no case did we get any better than a 42% speedup**"; accumulating into a **local variable** and writing once gave "**perfect scaling, linear in the number of processors**."

**★ Takeaway: the best fix is not padding — it is a thread-local accumulator written once at the end.**

### B.7.6 ★ The cautionary tale — Factorio `[PRIMARY]`

"Friday Facts #215 — Multithreading issues", kovarex & Klonan, 2017-11-03. https://factorio.com/blog/post/fff-215
> "Whenever something is changed, the other copies of the same page need to be invalidated and updated. This means that the threads are **invalidating each others cache all the time**, which slows the whole process so much that **it is slower than the non-parallel solution**."
> Parallelising trains / electric network / belt updates "didn't speed things up, it was **actually even slower**." What *did* thread: "The prepare logic gathers all the data (sprite draw orders) for rendering … in parallel up to **8** threads."

**A deterministic sim that mutates a shared world graph in-place does not parallelise by adding threads.** Every source that *succeeded* — Naughty Dog FrameParams, Destiny frame packet, AoE IV MAW islands, Unity `[ReadOnly]` — used the same fix: **last-frame state immutable, write to a separate output buffer, swap at the tick boundary.**

### B.7.7 Float determinism `[PRIMARY]`

**Bruce Dawson, "Floating-Point Determinism":**
> "Is IEEE floating-point math deterministic? … The answer is an unequivocal '**yes**'. Unfortunately the answer is also an unequivocal '**no**'."
> "floating-point determinism is **not about getting the 'right' answer** … it's about getting the **same** answer on some range of machines and builds."
> Breakers: x87 **80-bit** intermediates vs SSE; `/fp:fast`; **32- vs 64-bit** builds; debug vs release; compiler version; **FMA**; `rsqrt`; transcendentals (`sin`/`cos`/`tan` — no IEEE standard); `a + b + c` ordering.
> "If you can control these factors … then floating-point math **can** be deterministic, and indeed **many games have been shipped** based on this."

> ⚠️ **He does not recommend fixed-point. Across every primary source fetched, no author recommends fixed-point** — that is community folklore, `[UNVERIFIED]` as a primary-sourced recommendation.

**MSVC `/fp` docs**, a real cross-version desync trap:
> "Floating-point contractions aren't generated by default under `/fp:precise`. **This behavior is new in Visual Studio 2022. Previous compiler versions could generate contractions by default under `/fp:precise`.**"

Fiedler's expert round-up quotes **Gas Powered Games (Supreme Commander)**: "We have the compiler floating point model set to Fast **/fp:fast** … We have **never had a problem** with the IEEE standard across any PC cpu AMD and Intel with this approach." (Same-binary only.)

---

# PART C — Adaptation to *Millions Must Die*

## C.0 Where you actually stand

Read against the primary record, the engine on `plan/zombie-collision` is **not behind TAB — it is on a different and better-documented line.** Concretely:

| Dimension | They Are Billions `[PRIMARY]` | *Millions Must Die* today | Verdict |
|---|---|---|---|
| Navigation | **per-unit paths**, event-invalidated | **shared flow field**, built once (`nav/flow_field.rs`) | You match SupCom2/AoE IV, not TAB. **Better for your design.** |
| Memory layout | never stated | **SoA** `Vec<f32>` (`sim/agents.rs:55`) | Matches Acton/Albrecht/DICE |
| Allocation | never stated | **zero per-frame**, enforced (`alloc_guard.rs`) | Ahead of TAB's public record |
| Neighbour query | never stated | **uniform grid, counting sort** (`sim/spatial.rs`) | Matches BioDynaMo/Green/Ericson |
| Neighbour cap | never stated | **8** (`sim/collision.rs:36`) | In the 5–10 band everything shipped uses |
| Separation model | forked C++ physics solver | **soft steering**, honest about overlap | Correct call — see C.1 |
| Timestep | **none — hardware-coupled** | **fixed 1/60** (`sim/tick.rs:8`) | **You are strictly ahead** |
| Determinism | **none** | cross-process proven, seed 0 canonical | **You are strictly ahead** |
| Threading | AI/path/logic parallel, 12 cores | **single-threaded** (`sim/tick.rs:49`) | **Biggest untapped headroom** |
| Rendering | never stated (SlimDX/D3D9) | instanced, 4 atlas groups, 48 B/instance | Modern; sorting unsolved (C.3) |

**One honesty caveat that matters.** Your 50k-agent scene and TAB's 30k are **not comparable numbers**. TAB's zombies have sight, hearing, excitation state, target selection, combat, and per-unit paths. Yours walk a precomputed field. **Agent count is the easy axis; per-agent behaviour is the expensive one, and that is the bill Phase 1 will hand you.** Every headroom decision below should be judged against "what will this cost when each agent also fights, dies, and picks targets."

---

## C.1 What TAB teaches that you should copy — and what you should not

### Copy: flocking as the perception LOD

Arribas' admission — *"if you have to do this for every zombie at every instant … nobody can make it run"* — is the horde design constraint stated in one line. His answer is that **most zombies never run perception at all; they follow neighbours.**

You already have the substrate. `SpatialGrid` gives you the neighbour set; `accumulate_separation` already walks it. Adding an **alignment/cohesion term** to the same scan is nearly free — it reuses the neighbour loop you already pay for. When Phase 1 adds targeting, the rule should be:

- A small fraction of agents (the "scouts", or agents near a stimulus) run a real perception query and set a goal.
- Everyone else **inherits the goal from the neighbours already in their bin** — one extra accumulate in the existing loop.

This is a two-tier system, and it is exactly what Froblins describes generically: *"expensive global planning at a coarse resolution and lower update rate while the local model takes care of … a higher frequency."*

### Copy: dirty-flag invalidation for the field

TAB's one published optimisation was recomputing paths **only when something that can affect them changes** — and specifically *not* recomputing when a wall changes for units that don't route around walls.

Your `FlowField::build` is a full-map reverse Dijkstra done once. That is correct for a static prototype scenario and **wrong the moment Phase 1 adds buildable walls**, which is the entire game. Do not fix this with a full rebuild per change. Do it Emerson's way (C.4).

### Do NOT copy: per-unit pathfinding

TAB does it and calls it "one of the most costly operations." AoE IV measured the alternative: **200× more units cost only 4.5× more flow-field time.** Your `AGENT.md` constraint — *"No per-enemy pathfinding — navigation via flow fields"* — is the correct call and is backed by two shipped RTS engines. **Keep it. Do not let TAB's example erode it.**

### Do NOT copy: variable timestep

TAB shipped without one and the developers confirmed the game runs faster on faster CPUs. You have a fixed 1/60 tick, cross-process determinism, and a quantised state hash. **That is a shipped-quality property TAB never had**, it is what makes your test harness possible, and it is a prerequisite for replays and any future lockstep multiplayer (Terrano & Bettner, B.7.2). Guard it.

---

## C.2 The single highest-leverage change: amortise separation

**Where the cost is.** AoE IV's measurement is the whole argument: field time is sublinear in agent count, **steering time is near-linear**. At 50k agents the field is free and `accumulate_separation` is the bill. It is the only per-agent, per-tick, neighbour-scanning work in `tick.rs`.

**The primary-sourced fix.** Reynolds, PSCrowd 2006: *"The first two demos use a **skipThink count of 8** and the **2D crowd uses skipThink of 10**"* — at **15,000 agents, 60 fps**. Steering is recomputed once every 10 frames and the result is reused in between.

**Why this is legitimate and not the thing Graham warns about.** Graham's critique is that time-slicing "doesn't improve overall framerate … all it does is spread out those issues." That applies to **the same total work spread thinner**. Reynolds' `skipThink` does **less total work** — 1/10th of the separation scans. It is a throughput win, not a spike-smoothing trick.

**How it fits your determinism contract.** The bucket must be a pure function of index and tick — no clock, no RNG:

```rust
// sim/tick.rs — sketch, not a patch
const SEP_PHASES: u64 = 4;                       // start conservative
let phase = sim.tick_index % SEP_PHASES;
// only agents whose bucket matches recompute this tick; sep_x/sep_y persist otherwise
if (i as u64) % SEP_PHASES == phase { /* recompute */ }
```

`sep_x`/`sep_y` are already persistent `Vec<f32>` fields on `Simulation` (`sim/agents.rs:64`), so the stale value is *already* what a skipped agent would read. The grid rebuild can drop to the same cadence.

**Cost/benefit:** at `SEP_PHASES = 4` you do a quarter of the neighbour scans and a quarter of the grid rebuilds. Reynolds shipped at 10.

**The risk to test for, honestly:** stale repulsion means an agent can walk further into an overlap before being pushed out. Your docs already forbid claiming agents cannot overlap, so this does not break a stated contract — but it *will* change the state hash, and every behavioural test that pins agent positions will need regenerating. Treat `SEP_PHASES` as a scenario field so a test scenario can pin it to 1 and keep the old bit-exact behaviour.

---

## C.3 The rendering decision you have not yet made: isometric depth

**Current state.** `shaders/sprite.hlsl` writes `output.position = float4(ndc, 0.0, 1.0)` — **z is always 0** — and there is no depth buffer. Draw order is therefore instance order, and instance order is atlas-group order (`render/renderer.rs`, 4 groups, one `draw_indexed_primitives` each). **For a top-down test scene this is fine. For a StarCraft-like isometric view it is wrong**: a unit behind a wall will draw over it depending only on which atlas it happens to live in.

**The two options, and why one is much better at your scale.**

| | Painter's algorithm | `z = f(y)` + depth buffer |
|---|---|---|
| CPU work | **sort 50k instances every frame** | **none** |
| Instance order | must be depth order | **free** — sort by atlas instead |
| Alpha blend | correct | **incorrect** (order-dependent) |
| Alpha test (cutout) | correct | **correct, order-independent** |
| Overdraw | full | **early-Z rejects hidden pixels** |

The Khronos spec is the authority on why: *"fragment shader outputs are always written to the framebuffer in **primitive order**."* Blending is non-commutative; depth testing only *discards*.

**Recommendation `[INFERENCE, but well-supported]`: go with `z = f(y_screen)` + alpha test.** Pixel-art RTS sprites are hard-edged cutouts — exactly the case Microsoft's own blend-state doc calls out (*"defining detailed silhouettes for otherwise opaque sprites"*). Concretely:

1. Add a depth attachment to the offscreen target and enable depth write + `LESS` test for the unit pass.
2. In `VSMain`, replace the constant `0.0` with a normalised depth derived from the instance's world y (and layer). `SpriteInstance` is 48 B with `pos: [f32;2]` already present — **you can derive z from `instance_pos.y` with no change to the 48-byte layout or the `instance_layout_is_stable` test.**
3. Discard on alpha in `PSMain` (or use alpha-to-coverage) instead of blending, for the unit pass only.
4. Keep a **second, blended pass** afterwards for the genuinely translucent hundreds — shadows, selection rings, health bars, smoke — sorted back-to-front. Hundreds, not tens of thousands.

**If you ever do need a real sort** (e.g. large translucent sprites), do it Ericson's way with a packed key and your existing counting sort:
```
[ layer:4 | translucency:2 | y_screen:16 | atlas_id:6 | material:4 ] = 32 bits
```
Your map is bounded, so 16-bit y is **lossless**. Herf's radix numbers scale to well under 10 ns/element on a modern core ⇒ **50k sprites < 0.5 ms**. But the depth-buffer route costs zero, so this is the fallback, not the plan.

**Second rendering gap: no culling.** `pack_instance_groups` packs every agent every frame. DICE culled **15,000 objects in 0.32 ms on 4 jobs (2011 hardware)** with SoA + SIMD brute force, after *deleting* their cull tree — "3× faster and 1/5 the code." A screen-rect test on your SoA `x`/`y` is cheaper than their 6-plane sphere test. **Do not build a quadtree.** Budget ~0.2–0.5 ms and cut both the upload and the vertex work by however much of the map is off-screen.

---

## C.4 Flow field, when the map becomes destructible

Phase 1 is fortress defense. Walls get built and destroyed. Your current field is a **single full-map reverse Dijkstra built once** (`FlowField::build`). Three upgrades, in order of value:

**1. Tile it and dirty-flag it (Emerson).** Break the field into sectors — Emerson used **10×10 grid squares per sector** — and rebuild only dirty sectors, driven by *"a **priority queue, where each item in the queue is given a time slice of a fixed number of milliseconds**."* He also notes the integration field is separate memory, so *"you can easily spread out integration work across threads."* Cap tiles-per-tick to bound the cost.

**2. Add the LOS pass (Emerson / Cheng).** Agents with line of sight to the goal *"can ignore the Flow field results altogether and just steer toward the exact goal position."* Without it you get *"diamond-shaped flow directions around your goal."* Emerson: *"the LOS first pass is very cheap because it does not look at neighboring cost values."* This is the highest quality-per-cycle item on the list.

**3. Consider FMM instead of 8-neighbour Dijkstra.** Cheng, verbatim: *"Basic 8-neighbor Dijkstra Distance integration only gives 16 directions. Causing unnecessary turns."* Your `flow_field.rs` uses exactly that — `CARDINAL_COST = 1000`, `DIAGONAL_COST = 1414`, 8 neighbours — so descent vectors come from a discrete set. Froblins independently: Dijkstra on a discrete grid *"will not converge; we will always get stair-stepping artifacts."*

**Cost/benefit, honestly:** FMM is a real rewrite of the integrator and it changes every hash and golden in the repo. **Do items 1 and 2 first**; they are additive. Treat 3 as a visual-quality decision to make once you can see units walking, not now.

**And the free one:** Emerson's own Future Work list says *"**Multiple goal flow fields are perfect for zombies chasing heroes**."* Your design is literally that. When you need hordes converging on several targets, it is N fields blended, not N× the pathfinding.

---

## C.5 Threading: what to parallelise, and the trap

**The trap first.** Factorio parallelised belts, trains, and the electric network and found it *"slower than the non-parallel solution"* because threads were *"invalidating each others cache all the time."* Drepper measured the mechanism: **390% / 734% / 1147%** overhead at 2/3/4 threads sharing a cache line. Sutter got **worse than sequential on 24 cores** from one shared result array.

**Every source that succeeded used the same shape:** read last frame's state immutably, write to a separate buffer, swap at the tick boundary. Naughty Dog's 16 rotating FrameParams — *"No locks needed as each stage works on a unique instance."* Destiny's double-buffered frame packet — *"fully stateless."* AoE IV's MAW — *"one task in a task group to have write access."* Unity's `[ReadOnly]`.

**Where your code is already safe.** `accumulate_separation` (`sim/collision.rs:122`) reads `x`, `y`, `grid` — all immutable — and writes only `sep_x[i]` / `sep_y[i]`. **It is already a pure, index-disjoint, read-only-input pass.** That is the textbook parallel-for and it is the single most expensive loop in the tick. Splitting `sep_x`/`sep_y` into chunks and processing them in parallel is safe by construction, needs no locks, and is **bit-identical** — no floating-point reassociation happens, because each output element is computed by exactly one thread in exactly the same order.

**Where it is not yet safe.** The movement loop in `tick::step` mutates `sim.x[i]`/`sim.y[i]` in place and, via `recycle_one`, advances the shared `sim.recycle_cursor` — a sequential dependency that would produce different spawn assignments under any nondeterministic ordering. Fix it the AoE IV way if you want it parallel: a **read-only pass that marks arrivals into a bitset**, then a **short serial pass that assigns spawn slots in ascending index order**. Deterministic, and the serial part is proportional to arrivals, not to population.

**Grain size.** Intel oneTBB: *"grainsize iterations … should take at least 100,000 clock cycles"*, and *"typically a loop needs to take at least a million clock cycles to make it worth using parallel_for."* `[INFERENCE]` at hundreds of cycles per agent that puts you at **~256–2048 agents per job**. **One agent per job is pure overhead.**

**Sequencing note.** Do C.2 (amortisation) *before* threading. Amortising by 4 is a guaranteed 4× on that pass with no new failure modes; threading by 4 is at best 4× and brings false sharing, a new dependency (`rayon` is not currently in `Cargo.toml`), and a determinism surface to defend. **Cheap and safe first.**

---

## C.6 Smaller wins, each primary-sourced

**Skip the per-tick bin clear.** `SpatialGrid::rebuild` calls `self.starts.fill(0)` every tick — O(bins), independent of agent count. Teschner: *"Our implementation of the hash table **does not require a re-initialization in each simulation step** … each simulation step is labeled with a unique **time stamp**."* BioDynaMo converged on the same trick independently: *"we can build the grid in **O(#agents)** time instead of O(#agents + #boxes)."* On a large map with `bin_size = 1.0` (which is what `bin_size_cells()` returns for any radius ≤ 0.5) this is a real cost you are paying for nothing.

**Your neighbour cap of 8 is right — leave it.** Detour hard-codes **6** as a compile-time constant; RVO2's own demos ship **10**; Reynolds used **5** at 15k agents. You are inside the band that everything which shipped at scale uses. Guy & Karamouzas state the reason: *"By selecting a **fixed maximum number of neighbors** … the runtime will be **nearly linear** in the number of agents."* And Karamouzas' PRL paper gives the physical justification: *"interactions between distant, non-neighboring pedestrians are **screened by the presence of nearest-neighbors**."*

**Bin by min-corner, not centre.** Ericson's free win: centroid binning *"makes a total of nine cells tested"*; min-corner binning means *"at best only four cells have to be tested."* Your `bin_of` uses the position directly (a centre). With `bin_size = 2r` you could scan 2×2 instead of 3×3 in the common case — **up to a 2.25× reduction in bins visited**, for a change confined to `spatial.rs` and `collision.rs`.

**Your separation falloff is already the safe choice.** You use **linear falloff** — full push at coincidence, none at contact. Reynolds' original `1/r` comes with his own disclaimer: *"1/r is just a setting that has worked well, **not a fundamental value**."* And Karamouzas 2017 is explicit that the physically-measured power-law model *"leads to collisions and other discontinuities in motion with time steps much larger than **10 ms**"* — **your tick is 16.7 ms.** A `1/r` or `1/r²` force would be numerically worse at your timestep, not better. **Do not "upgrade" to a physical force model.**

**Do not adopt ORCA/RVO2.** Its authors' own best number is **5,000 agents / 8 ms / 8 cores**, and they document that in dense conditions the linear program becomes **infeasible** and can produce **global deadlock**. Narain et al. independently found RVO *"failed to run on scenes containing more than 70,000 agents."* At 50k agents, in a game whose entire premise is a dense crowd, ORCA fails exactly where you need it.

**When agents jam at the destination, use priority, not a better solver.** Froblins names your future bug precisely: *"agents can deadlock and will become stuck. This typically happens at sinks in agent navigations such as at a small goal … agents that reach the goal will be **unable to navigate out of the goal area**."* Their proposed fix and the shipped-RTS answer agree — **asymmetric push priority**. Blizzard's SC2 5.0.15 notes: *"Increased **allied push priority** for Thors and Siege Tanks."* Emerson: *"super large robots that could push back a hundred tanks."* A per-agent mass/priority byte multiplying the repulsion is far cheaper than any reciprocal solver.

**Cheap visual variety.** Fauerby: *"Unique scaling factor for each agent — small amount: **~5%**. … Does wonders for perceived diversity."* Your `SpriteInstance` already carries a per-instance `size: [f32; 2]` and an RGBA `tint`. Both are free variety channels you are currently not using.

---

## C.7 A frame budget to design against

**This is a target derived from other people's published measurements, not a measurement of your engine.** Phase 0 retired benchmarking and no number here gates anything.

At 60 fps you have **16.67 ms**. A defensible split for 50k agents plus a 500-unit player army:

| Stage | Budget | Anchor |
|---|---|---|
| Flow field (amortised, dirty tiles only) | **< 1 ms** | AoE IV: 1.11 ms for 200 units, sublinear |
| Grid rebuild (counting sort, timestamped) | **~0.5 ms** | BioDynaMo: 2.80 ms at 10⁵ agents incl. search |
| Separation (amortised 1/4, threaded) | **3–4 ms** | AoE IV steering is the near-linear term |
| Movement integrate + recycle | **1–2 ms** | Albrecht: 11k-node loop at 3.3 ms post-DOD |
| Cull + pack instances | **0.3–0.5 ms** | DICE: 15k objects, 0.32 ms, 4 jobs, 2011 |
| Upload (≈2.4 MB) + 1–4 draw calls | **< 1 ms** | Wloka: budget is thousands of batches, you use 4 |
| **Player army AI, combat, targeting (Phase 1)** | **remainder** | the real unknown |

Two things that table is meant to make obvious: **navigation is not your problem, separation is**; and **the budget you should be protecting is the one Phase 1 will spend on actual gameplay**, not on moving dots.

---

## C.8 Ranked backlog

| # | Change | Where | Evidence | Effort | Payoff |
|---|---|---|---|---|---|
| 1 | Amortise separation + grid rebuild across N ticks | `sim/tick.rs`, `sim/collision.rs` | Reynolds `skipThink` 8–10 @ 15k/60fps | S | **N× on the dominant pass** |
| 2 | `z = f(y)` + depth buffer + alpha test | `shaders/sprite.hlsl`, `render/renderer.rs` | Khronos §7.13; MS blend-state doc | M | **deletes a 50k sort you'd otherwise write** |
| 3 | Parallelise `accumulate_separation` | `sim/collision.rs` | AoE IV MAW; ND FrameParams; already index-disjoint | M | ~cores×, bit-identical |
| 4 | Screen-rect cull before packing | `runtime.rs::pack_instance_groups` | DICE 15k @ 0.32 ms, no tree | S | cuts upload + vertex work |
| 5 | Timestamped bins (skip `starts.fill(0)`) | `sim/spatial.rs` | Teschner; BioDynaMo | S | O(agents) not O(bins) |
| 6 | Tile + dirty-flag the flow field | `nav/flow_field.rs` | Emerson priority queue + ms time slice | L | **required for destructible walls** |
| 7 | LOS pass in the integrator | `nav/flow_field.rs` | Emerson; Cheng 4-step | M | quality + speed |
| 8 | Per-agent push priority (mass byte) | `sim/collision.rs` | Blizzard SC2 5.0.15; Emerson; Froblins | S | fixes goal-sink deadlock |
| 9 | Min-corner binning (2×2 scan) | `sim/spatial.rs` | Ericson §7.1.6.1 | S | up to 2.25× fewer bins |
| 10 | Flocking goal propagation | `sim/collision.rs` (same scan) | Arribas/Xataka; Froblins two-tier | M | **the TAB horde feel**, Phase 1 |

---

# PART D — Corrections to widely-repeated folklore

1. **"They Are Billions uses flow fields."** — **False.** The developers wrote the opposite: *"on TAB every unit computes its own paths."* The claim traces to a Unity tutorial about building something *like* TAB.
2. **"Numantian wrote custom memory management to avoid GC."** — **Not a developer statement.** Forum user "Awac", 2017-09-13. Numantian has never commented on GC, pooling, or memory layout.
3. **Teschner's three hash constants are "large prime numbers".** — **p2 = 19349663 = 41 × 471943, not prime.** Harmless, but the error is in many codebases.
4. **"Continuum Crowds does 10,000 agents in real time."** — It ran at **2–5 fps on a 3.4 GHz Pentium in 2006**. Cite it for the maths, never as a real-time result.
5. **"ORCA scales to tens of thousands."** — Its authors' best published figure is **5,000 agents / 8 ms / 8 cores**, and the ORCA project page's "25,000 agents in a virtual Hajj" carries **no timing**. Narain et al. found RVO *"failed to run on scenes containing more than 70,000 agents."*
6. **"Use a physically-measured power-law separation force."** — Unstable above a **10 ms** timestep. Your frame is 16.7 ms.
7. **"Time-slicing improves framerate."** — Graham: *"Time-slicing reduces spikes; it doesn't improve overall framerate."* Reynolds' `skipThink` is a different thing — it does **less work**, not the same work spread thinner.
8. **"Naughty Dog uses lock-free queues."** — *"**Atomic spin locks are used almost everywhere**."*
9. **"Fixed-point is required for determinism."** — **No primary source recommends it.** Dawson's position is that IEEE float is deterministic if you control the build; Gas Powered Games shipped Supreme Commander on `/fp:fast` and *"never had a problem"* same-binary.
10. **"Jolt/Box2D prove trees beat grids."** — Jolt justifies its quadtree by **SIMD width** and does not compare to grids. No Catto statement comparing them was found.
11. **The GPU spatial-binning patent (US8810590B2) is AMD's** (Oat/Shopf/Barczak), **not Uber Entertainment's**.
12. **Fiedler's published accumulator loop does not compile as printed** (`previous`/`current` vs `previousState`/`currentState`).

---

# PART E — Gaps, stated honestly

**About They Are Billions:** engine name, the forked physics library's identity, the pathfinding algorithm, spatial partitioning, thread count, AI tick rate, AI LOD, the entire sprite pipeline, draw-call batching, the Direct3D version, ms/frame, fog of war, tile pixel size, chunking, and GC strategy — **all have no primary source.** A developer was asked directly about the sprite pipeline and about DX9 and answered neither.

**About the technique literature:** no primary Planetary Annihilation source was obtainable (Uber forums 403); no Blizzard engineering writeup on SC2 local avoidance exists; Brockington's LOD-AI chapter is print-only; no primary Creative Assembly technical talk on Total War aggregate simulation was found. GDC Vault video-only talks (Genova on Destiny threading; the AC Unity and Hitman vault entries) are login-gated — free equivalents were used where they existed.

---

## Master source list

**They Are Billions `[PRIMARY]`**
- https://steamcommunity.com/app/644930/discussions/0/1353742967825284553/ — engine thread, 4 dev replies (2017)
- https://numantiangames.com/News/2018-06-07-they-are-billions-development-update-v-0-8-2/ — pathfinding update
- https://www.xataka.com/videojuegos/entre-bambalinas-de-they-are-billions-el-nuevo-bombazo-del-videojuego-espanol — Arribas interview
- https://gamesbeat.com/the-making-of-early-access-hit-they-are-billions/ — Arribas, GamesBeat
- https://steamcommunity.com/app/644930/discussions/0/3022387599786311865/ — game speed / CPU coupling
- https://steamcommunity.com/app/644930/discussions/0/1499000547495151383/ — changelog thread
- https://store.steampowered.com/app/644930/ — specs, feature copy

**Pathfinding**
- http://www.gameaipro.com/GameAIPro/GameAIPro_Chapter23_Crowd_Pathfinding_and_Steering_Using_Flow_Field_Tiles.pdf
- https://media.gdcvault.com/GDC+2022/Speaker+Slides/Pathing+In+Age_Cheng_Frank+2022-03-29+00.16.38.pdf
- https://grail.cs.washington.edu/projects/crowd-flows/continuum-crowds.pdf
- https://advances.realtimerendering.com/s2008/SIGGRAPH2008%20-%20March%20of%20the%20Froblins.pdf
- http://www.gameaipro.com/GameAIPro/GameAIPro_Chapter24_Efficient_Crowd_Simulation_for_Mobile_Games.pdf

**Data-oriented design**
- https://github.com/CppCon/CppCon2014/blob/master/Presentations/Data-Oriented%20Design%20and%20C%2B%2B/
- https://harmful.cat-v.org/software/OO_programming/_pdf/Pitfalls_of_Object_Oriented_Programming_GCAP_09.pdf
- https://docs.unity3d.com/Packages/com.unity.entities@1.3/manual/concepts-archetypes.html
- https://www.gamedevs.org/uploads/culling-the-battlefield-battlefield3.pdf

**Spatial partitioning**
- https://matthias-research.github.io/pages/publications/tetraederCollision.pdf
- https://realtimecollisiondetection.net/books/rtcd/toc/
- https://arxiv.org/pdf/2301.06984 (BioDynaMo, PPoPP 2023)
- https://wrfranklin.org/p/105-nearpt3.pdf
- https://arxiv.org/pdf/1909.04504 (PySPH)
- https://developer.download.nvidia.com/compute/cuda/2_2/sdk/website/projects/particles/doc/particles.pdf
- https://ramakarl.com/pdfs/2014_Hoetzlein_Fast_Neighbors.pdf

**Local avoidance**
- https://gamma.cs.unc.edu/ORCA/publications/ORCA.pdf
- https://www.red3d.com/cwr/steer/gdc99/
- https://www.red3d.com/cwr/papers/2006/PSCrowdSandbox2006.pdf
- https://motion.cs.umn.edu/pub/ImplicitTTC/implicit_crowds.pdf
- https://arxiv.org/pdf/1412.1082 (Universal Power Law, PRL 2014)
- https://arxiv.org/pdf/1802.02673 (position-based crowds)
- https://raw.githubusercontent.com/recastnavigation/recastnavigation/main/DetourCrowd/Include/DetourCrowd.h
- http://www.gameaipro.com/GameAIPro2/GameAIPro2_Chapter19_Guide_to_Anticipatory_Collision_Avoidance.pdf
- https://news.blizzard.com/en-us/article/24225313/starcraft-ii-5-0-15-patch-notes

**Simulation LOD**
- https://archive.org/stream/GDC2015Cournoyer/GDC2015-Cournoyer_djvu.txt
- http://www.gameaipro.com/GameAIPro/GameAIPro_Chapter14_Phenomenal_AI_Level-of-Detail_Control_with_the_LOD_Trader.pdf
- http://www.gameaipro.com/GameAIProOnlineEdition2021/GameAIProOnlineEdition2021_Chapter02_Efficient_Event_Based_Simulations.pdf
- https://web.archive.org/web/2018/http://gamma.cs.unc.edu/DenseCrowds/narain-siga09.pdf
- https://media.gdcvault.com/gdc2012/slides/Programming%20Track/Fauerby_Kasper_CrowdsInHitman.pdf

**Rendering**
- https://www.nvidia.com/docs/io/8228/batchbatchbatch.pdf
- https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-drawindexedinstanced
- https://download.nvidia.com/developer/NVTextureSuite/Atlas_Tools/Texture_Atlas_Whitepaper.pdf
- https://registry.khronos.org/OpenGL/specs/gl/glspec46.core.pdf (§7.13)
- https://learn.microsoft.com/en-us/windows/win32/direct3d11/d3d10-graphics-programming-guide-blend-state
- https://realtimecollisiondetection.net/blog/?p=86
- http://stereopsis.com/radix.html
- https://docs.unity3d.com/Manual/2d-renderer-sorting.html

**Timestep, determinism, threading**
- https://gafferongames.com/post/fix_your_timestep/
- https://zoo.cs.yale.edu/classes/cs538/readings/papers/terrano_1500arch.pdf
- https://media.gdcvault.com/GDC+2022/Speaker+Slides/GDC22_MAW.pdf
- https://media.gdcvault.com/gdc2015/presentations/Gyrling_Christian_Parallelizing_The_Naughty.pdf
- https://advances.realtimerendering.com/destiny/gdc_2015/Tatarchuk_GDC_2015__Destiny_Renderer_web.pdf
- https://www.akkadia.org/drepper/cpumemory.pdf (§6.4.1)
- https://jacobfilipp.com/DrDobbs/articles/DDJ/2009/0905/0905ec01/0905ec01.html
- https://factorio.com/blog/post/fff-215
- https://randomascii.wordpress.com/2013/07/16/floating-point-determinism/
- https://uxlfoundation.github.io/oneTBB/main/tbb_userguide/Controlling_Chunking_os.html
