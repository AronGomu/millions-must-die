# Manual test checklist

Steps a human must run and look at. One section per ticket; never edit another
ticket's section.

## T0 starcraft-scale-cap-and-body

Landed on `plan/horde-sim-headroom`. Everything automatable is green
(460 tests, clippy, `nix flake check`, all three xtask checks, all three
scene smokes). What is left needs eyes on a real window.

> **Restated by T9.** These steps described a flat top-down view. Since T9 the
> render layer projects 2:1 isometric and depth-orders the frame, so "two agents
> pressed together are edge-to-edge" is no longer what the screen shows: the
> sprites now stand on a projected ground point and deliberately overlap
> vertically, and the camera is fixed on the destination so most of the horde
> starts off screen. The *sim-side* claim these steps exist for — the body is
> exactly half a sprite — is unchanged and is now read off the floor ellipse.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves.
      Units read as chunky StarCraft-scale sprites, clearly bigger than the old
      30 px units (the drawn quad is still 48 px). The view is **isometric** and
      fixed on the destination, so the horde marches in from the upper left
      rather than filling the window at once. Press `Esc` to quit and confirm
      a clean exit.
- [ ] In that same window, watch two agents press together at a choke point or
      against an obstacle. Judge contact by the **floor ellipses** (`H`), not by
      the sprite art: the ellipses must meet edge to edge and not overlap
      deeply. The body is exactly half a sprite (6 cells = 24 px radius), but
      under the projection the sprites themselves overlap on screen — a nearer
      unit standing partly over the one behind it is correct isometric depth,
      not a body/sprite ratio bug.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` — the
      5 000-agent demo scene starts, spreads out of its spawn stacks, and quits
      cleanly on `Esc`.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      the 1 200-agent scene starts; at this lower density the edge-to-edge
      contact above is easiest to see. Quit with `Esc`.
- [ ] Judgement call for the plan owner, not a bug in this ticket: confirm
      5 000 agents at 48 px still reads as a horde rather than a sparse field.
      If it reads sparse, that is a design signal for the density knobs.

## T1 scenario-headroom-knobs

Contract-only slice: three new scenario fields (`separation_phases`,
`mass_class_count`, `separation_threads`), all pinned to their identity value
`1`. Nothing reads them yet, so a human should observe **zero visible change**
from T0. Everything automatable is green (`cargo fmt`, `cargo test --workspace
--locked`, clippy, `nix flake check`, `xtask bootstrap --check`, and all three
scene smokes with the gate hash reproduced byte for byte). What is left needs
eyes on a real window, same as T0.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did before this ticket: same 48 px sprites, same
      edge-to-edge contact at chokepoints. Press `Esc` to quit and confirm a
      clean exit.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      starts, spreads, quits cleanly on `Esc`; behaviour indistinguishable
      from T0.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      same check at the lower-density scale.
- [ ] Try authoring a scenario `.ron` that omits `separation_phases`,
      `mass_class_count` or `separation_threads` (or sets one to `0`) and
      confirm it fails to load with a named error instead of running
      silently untuned — the new fields are required, not defaulted.

## T2 stamped-bin-counts

Internal neighbour-index rewrite (`SpatialGrid::rebuild` in
`crates/mmd-engine/src/sim/spatial.rs` no longer clears `starts` before
counting; bin population is now an O(1) `bin_count` query via a rebuild
stamp). Behaviour is bit-identical to before this ticket — a human should
observe **zero visible change**. Everything automatable is green (`cargo
fmt`, `cargo test --workspace --locked`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`, `nix flake check`, the
testkit-gated `cargo build -p mmd-engine --no-default-features --features
gpu`, and the gate smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte). What is left needs eyes on a real window.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did before this ticket: same 48 px sprites, same
      edge-to-edge contact at chokepoints, same separation behaviour at
      chokepoints and in the corner-pocket case. Press `Esc` to quit and
      confirm a clean exit.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      starts, spreads, quits cleanly on `Esc`; behaviour indistinguishable
      from T1.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      same check at the lower-density scale.

## T3 amortised-separation

The separation scan and grid rebuild now spread over the scenario's
`separation_phases` ticks. Every scene except `collision_mid_v1` still pins
`separation_phases: 1` and must look and behave exactly as before this
ticket. `collision_mid_v1` alone now amortises over 4 phases: at most a
3-tick-stale neighbour position, one grid rebuild per 4 ticks instead of
every tick. Everything automatable is green (`cargo fmt`, `cargo test
--workspace --locked`, `cargo clippy --workspace --all-targets
--all-features -- -D warnings`, `nix flake check`, the testkit-gated `cargo
build -p mmd-engine --no-default-features --features gpu`, and the gate
smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte). What is left needs eyes on a real window.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did before this ticket (this scene stays at
      `separation_phases: 1`): same 48 px sprites, same edge-to-edge contact
      at chokepoints. Press `Esc` to quit and confirm a clean exit.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      starts, the stacked spawn column must still visibly spread apart over
      the run (now amortised over 4 phases, so the spread may look very
      slightly slower/chunkier tick to tick, but must not stall or freeze),
      no agent stuck inside the scene's obstacle, quits cleanly on `Esc`.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      unaffected by this ticket (stays at `separation_phases: 1`); same check
      as T2 at the lower-density scale.

## T4 row-contiguous-scan

Pure restructuring of the per-agent 3x3 neighbour scan in
`crates/mmd-engine/src/sim/collision.rs`: it now fetches three row-contiguous
slices (`SpatialGrid::agents_in_bin_row`) instead of nine per-bin slices, and
an agent whose whole window holds only itself skips the scan and writes zero
directly. Behaviour is bit-identical to before this ticket — a human should
observe **zero visible change**. Everything automatable is green (`cargo fmt
--all -- --check`, `cargo test --workspace --locked`, `cargo test -p
mmd-engine --test separation` (32 tests, including
`a_bodied_scenario_is_pinned_to_a_golden_digest`,
`separation_is_capped_at_eight_neighbours` and
`separation_of_a_pair_is_equal_and_opposite` unedited), `cargo test -p
mmd-engine --test frame_allocations`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`, `nix flake check`, and the gate
smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte). What is left needs eyes on a real window.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did before this ticket: same 48 px sprites, same
      edge-to-edge contact at chokepoints, same separation behaviour at
      chokepoints and in the corner-pocket case. Press `Esc` to quit and
      confirm a clean exit.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      starts, the stacked spawn column still visibly spreads apart over the
      run (amortised over 4 phases, as in T3), no agent stuck inside the
      scene's obstacle, quits cleanly on `Esc`; indistinguishable from T3.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      unaffected by this ticket (stays at `separation_phases: 1`); same check
      as T3 at the lower-density scale.

## T5 push-priority-mass

Repulsion is now weighted by a per-agent push priority (`mass: Vec<u8>` /
`inv_mass: Vec<f32>` on `Simulation`, `mass[i] = (i % classes) + 1`): a
heavier neighbour pushes harder and is itself pushed less, so a dense goal
sink breaks its own symmetry instead of deadlocking. Every scene still at
`mass_class_count: 1` (`collision_mid_v1`, `technical_prototype_v1`, and the
`fixture_*` scenes) is bit-identical to before this ticket — a human should
observe **zero visible change** there. `collision_sprite_v1` alone now opts
into `mass_class_count: 2`. Everything automatable is green (`cargo fmt --all
-- --check`, `cargo test --workspace --locked`, `cargo test -p mmd-engine
--test separation` (36 tests, including `a_bodied_scenario_is_pinned_to_a_
golden_digest` and `a_bodyless_scenario_walks_the_flow_only_path` unedited,
plus the four new mass tests), `cargo test -p mmd-engine --test
frame_allocations`, `cargo clippy --workspace --all-targets --all-features --
-D warnings`, `cargo build -p mmd-engine --no-default-features --features
gpu`, `nix flake check`, all three xtask `--check` commands, and the gate
smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte). What is left needs eyes on a real window.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did before this ticket (this scene stays at
      `mass_class_count: 1`): same 48 px sprites, same edge-to-edge contact at
      chokepoints. Press `Esc` to quit and confirm a clean exit.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      unaffected by this ticket (stays at `mass_class_count: 1`); starts,
      spreads, no agent stuck inside the scene's obstacle, quits cleanly on
      `Esc`; indistinguishable from T4.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      this is the scene the ticket changed: it now runs two push-priority
      classes. Confirm the stacked spawn column still visibly opens up over
      the run, and that no agent ends up shoved inside a wall or obstacle.
      Quit cleanly on `Esc`.

## T6 parallel-separation

The separation pass can now run on a persistent worker pool
(`crates/mmd-engine/src/sim/pool.rs`), sized by the scenario's
`separation_threads`. **Every tracked scene stays at `separation_threads: 1`**,
so nothing below should look different from T5 — the threaded path is
deliberately shipped switched off and is proven by inline `GridSpec` tests
only, so the merge gate reproduces on a host with any core count.

Everything automatable is green: `cargo fmt --all -- --check`, `cargo test
--workspace --locked`, `cargo test -p mmd-engine --test separation` (42 tests
in both debug and release, with `a_bodied_scenario_is_pinned_to_a_golden_digest`
and `a_bodyless_scenario_walks_the_flow_only_path` unedited), `cargo test -p
mmd-engine --test frame_allocations` (9 tests, with
`a_collision_tick_allocates_nothing` and
`foreign_thread_allocations_do_not_leak_into_a_measure_scope` unedited),
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo build -p mmd-engine --no-default-features --features gpu`,
`cargo tree -e features | grep -c testkit` = 0, `nix flake check`, all three
xtask `--check` commands, the determinism test 10/10 in a row, and the gate
smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte. What is left needs eyes on a real window.

> Note added by T8: every window check below now also draws a cyan hitbox ring
> on each agent, because T8 turns the overlay on by default. That is expected
> and is **not** a T6 regression — press `H` to hide the rings if they get in
> the way of judging the movement these steps are actually about. T6's claims
> are about simulation behaviour, which T8 does not touch.

- [ ] `cargo run -- run --agents 5000` — a window opens and the horde moves
      exactly as it did in T5; this scene is at `separation_threads: 1`, so any
      visible difference at all is a defect. Press `Esc` and confirm a clean
      exit with no lingering process.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      unchanged by this ticket (`separation_threads: 1`, still 4 phases).
      Starts, spreads, no agent stuck inside the scene's obstacle, quits
      cleanly on `Esc`.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      unchanged by this ticket (`separation_threads: 1`, still 2 mass classes).
      The stacked spawn column still visibly opens up; quits cleanly on `Esc`.
- [ ] No thread outlives the process: after each `Esc` quit above, confirm the
      process is gone (`pgrep -f millions_must_die` returns nothing). A pool
      worker that failed to join would keep the process alive after the window
      closed.

## T8 hitbox-ring-overlay

Every entity now draws a procedural ring at its **real** body radius —
`collision_radius_cells * cell_size_px`, read from the live scenario. The ring
is atlas-free (a branch in `shaders/sprite.hlsl` selected by a negative
`uv_rect.x`), reuses the pinned 48-byte `SpriteInstance`, is on by default, and
is toggled with `H`.

Everything automatable is green: `cargo fmt --all -- --check`, `cargo test
--workspace --locked`, `MMD_REQUIRE_GPU=1 cargo test -p mmd-engine --test
render_correctness` (19/19, so `a_ring_is_hollow`, `golden_frame_matches` and
`world_to_clip_matches_gpu_raster` all really ran rather than skipping),
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`nix flake check`, all three xtask `--check` commands,
`cargo tree -e features | grep -c testkit` = 0, an empty
`git diff --stat crates/mmd-engine/src/sim/`, and the gate smoke still printing
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte.

The rings were also inspected off-screen: an offscreen readback of
`collision_sprite_v1` at 600 agents was written to PNG and viewed, showing one
thin hollow cyan ring per agent with the sprites visible through them, and an
A/B against the same frame with the overlay hidden showed identical sprites and
no rings. What is left needs eyes on a real window.

> **Restated by T9.** The ring is unchanged in code, but the projection under it
> is not: the quad is an ellipse **twice as wide as it is tall**, centred on the
> unit's feet instead of on its middle. A circle, or a ring hugging the sprite's
> outline, is now the failure. The ring also draws on its own depth-free
> pipeline, so it is never covered by a unit in front of it.
>
> **Corrected by T10 — the ring is now smaller than every entry below described.**
> T9's quad of `2r·tile_w` by `2r·tile_h` was `√2` too large on **both** axes:
> that is the circle's bounding box under the shearing projection, not the
> circle's image. The shipped quad is `√2·r·tile_w` by `√2·r·tile_h` —
> `67.9 × 33.9 px` on the tracked scenes, where it used to be `96 × 48`. The
> 2:1 aspect is unchanged, so "twice as wide as tall" still holds. What changed
> is the absolute size, and with it the one thing the overlay exists for: two
> units at exactly contact distance now render **tangent** rings instead of
> overlapping ones. Any entry below that describes the ring in sprite-widths has
> been rewritten rather than left to be read as still true.

- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      every unit carries a hollow ring. Press `H`: the rings disappear and the
      sprites do not move or flicker. Press `H` again: they come back
      identically. `Esc` exits cleanly.
- [ ] Same scene, ring **shape and placement**: each ring is an ellipse lying on
      the floor *under* the unit's feet, about twice as wide as tall — not a
      circle, and not centred on the unit's chest. A circle means the ring quad
      lost the projection's aspect.
- [ ] Same scene, ring **radius** read: where two units press together, their
      ellipses **touch without crossing**. Two rings that intersect mean the
      quad is oversized again — that was the T8/T9 defect T10 fixed, and it is
      the one thing this overlay is on screen to answer.
      Sizes to read against, not the old "one sprite tall": on the tracked
      scenes the ellipse is about **68 px wide and 34 px tall** against a 48 px
      sprite, so it is *wider* than the sprite and roughly **two thirds of the
      sprite's height** in its short (screen-y) axis. An ellipse a full sprite
      tall (48 px) is the pre-T10 size and a failure.
- [ ] Same scene, ring **legibility**: the ring is 1–2 px thick and
      semi-transparent, so a dense crowd still reads as separate bodies rather
      than a solid cyan mass. If it reads as a wash, `RING_TINT` /
      `RING_INNER` in `crates/mmd-engine/src/runtime.rs` are the two knobs.
- [ ] `cargo run -- run --agents 5000` — the gate scene at full population with
      rings on stays interactive (no obvious frame-rate collapse versus `H`
      off). This is the one cost the automated suite cannot measure: 5 000
      overlapping ring quads is a fill-rate question, not a CPU one.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` —
      rings appear here too (same body tuning), and `H` toggles them.
- [ ] The first presented frame already has rings — they must not pop in on
      frame 2. Watch the very first painted frame after the window appears.

## T9 isometric-projection-and-depth

The render layer now projects the world 2:1 isometric and draws depth-ordered.
The simulation is untouched and stays in Cartesian cell space — that is what
keeps every pinned digest alive. A cell is drawn as an `8 × 4` px tile, so the
480 × 270 grid becomes a 3 000 × 1 500 px diamond, deliberately larger than the
1920 × 1080 view; the camera is **fixed** on the destination cell and everything
outside the view is culled. Scrolling, edge-pan, zoom and selection are Phase 1
and are *not* in this build.

Everything automatable is green: `cargo fmt --all -- --check`,
`cargo test --workspace --locked` (516 passed) and the same suite again under
`MMD_REQUIRE_GPU=1` (so `an_agent_in_front_occludes_one_behind`,
`rings_are_never_occluded`, `world_to_clip_matches_gpu_raster` and
`golden_frame_matches` really ran rather than skipping),
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`nix flake check`, all three xtask `--check` commands,
`cargo test -p mmd-lab --test merge_gate`, `cargo tree -e features | grep -c
testkit` = 0, an empty `git diff --stat crates/mmd-engine/src/sim/`, and the
gate smoke still printing
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte.

The frame was also inspected off-screen before this list was written: a real
`Runtime` frame (`collision_mid_v1`, 5 000 agents, 900 ticks) was rendered to
PNG and viewed. The crowd's leading edge is a clean 2:1 diagonal, units lower on
screen cover the rank behind them with no slicing, and the ring pass paints cyan
ellipses over everything. What is left needs eyes on a real window.

- [ ] `cargo run -- run --agents 5000` — the horde reads as an **isometric**
      crowd on a 2:1 floor, not a flat top-down grid. Ranks recede up and to the
      right; the mass has a diamond edge, not a rectangular one. `Esc` exits
      cleanly.
- [ ] Same window, **depth order**: pick a spot where two units overlap. The one
      whose feet are lower on screen must be drawn in front, completely and
      cleanly. A unit sliced horizontally by its neighbour, or flickering
      between front and back as the pair moves, is the failure this whole ticket
      exists to prevent (that would mean the vertex stage is emitting a
      per-vertex depth instead of one value off the quad's bottom edge).
- [ ] Same window, **cutout edges**: sprites have hard pixel-art edges with no
      halo or dark fringe where they overlap. The alpha test replaced blending
      for the sprite pass, so a soft or grey outline means the cutoff is wrong.
- [ ] Same window, press `H`: a floor **ellipse** appears under each unit, about
      twice as wide as it is tall, lying flat under the feet. It must stay fully
      visible even where a unit in front overlaps it — a ring that disappears
      behind a neighbour means the ring pass picked up the depth state.
- [ ] Same window, **the fixed camera**: the view is centred on the destination
      cell and never moves. At `--agents 5000` the horde marches in from the
      upper left and only part of it is on screen at spawn. That is correct and
      is what the cull is for; there is no scrolling in this build, so do not
      report "cannot see the whole map" as a bug.
- [ ] Watch the boundary as units enter and leave the view. A unit must fade in
      and out of the frame by moving across the edge, never **pop** into
      existence a sprite-width inside the view — popping means the cull rejects
      quads that still straddle an edge.
- [ ] Same window, **the far edge of the map**: units at the top of the diamond
      (smallest `x + y`) must draw like any other. The depth key is the position
      down the diamond and the test is `GREATER` against a buffer cleared to 0,
      so a key of exactly 0 would be discarded rather than drawn behind
      everything — the shader floors sprites one depth quantum above it. A unit
      that vanishes only at the far corner means that floor was lost.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron` — same
      isometric read and same depth ordering at 5 000 agents on the demo scene.
      `Esc` exits cleanly.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      at 1 200 agents the depth ordering and the floor ellipses are easiest to
      judge one unit at a time. Quit with `Esc`.
- [ ] Judgement call for the plan owner, not a bug in this ticket: the eight
      atlas directions are now reinterpreted as the eight *isometric* facings.
      Confirm a unit walking toward the destination faces roughly the way it is
      moving. If the diagonals read wrong, that is an **art** fix in a later
      ticket — the direction index was deliberately not rotated in code.
- [x] ~~Judgement call: the floor ellipse is drawn a factor √2 larger than the
      geometrically exact projection of the body circle. Confirm whether it
      reads as "the unit's footprint" or as "noticeably too big".~~
      **Closed by T10, not by judgement.** It was not a matter of taste: at
      `2r·tile` the ellipse was the body circle's bounding box under the shear
      rather than its image, so two units at exactly contact distance drew
      *overlapping* rings and the overlay could not be used to judge contact.
      `runtime::ring_quad_size_px` now divides both axes by `√2` (shipped as
      `√2·r·tile`), the drawn ellipse is the exact projection, and
      `the_rings_of_two_touching_bodies_are_tangent` is the standing guard.
      Verified on an offscreen readback before/after: intersecting → tangent.

## T7 docs-adr-and-system-map

Docs-only ticket: no engine code changed. ADR 010 and ADR 011 were verified
against the shipped `sim/collision.rs`, `sim/spatial.rs` and `sim/pool.rs` and
flipped from `Proposed` to `Accepted`; ADR 011 also corrected a factually wrong
"debug assertion" bullet (the pool's re-entrancy check is a real `assert!`,
promoted from `debug_assert!` by T6) and gained the panic-safety and
release-acquire ordering claims T6 actually shipped. ADR 012 is new, recording
the StarCraft-scale resize and the render decisions T0/T8/T9 shipped, including
the `GREATER`/clear-`0` depth convention, the known gaps (`.spv` not bound to
`sprite.hlsl`, native DXIL/metallib blob debt, the √2 ring-ellipse margin
*(closed by T10 — see that section; ADR 012 now records the resolution instead
of the follow-up)*, the
frozen bench ladder's lost comparability, the pixel-gate tautology on the
Ubuntu fixture) named as risks rather than fixed. Everything automatable is
green: `cargo fmt --all -- --check`, `cargo test --workspace --locked` (35
`test result: ok` blocks, 0 failed), `cargo test -p millions_must_die --test
validation_contract` (7/7, including `every_system_has_a_test` at
`SCOPE_SYSTEM_COUNT = 14` and `no_perf_claim_in_docs`)
*(updated by T16: that file now holds ten tests and `SCOPE_SYSTEM_COUNT = 20` —
the six phase-1 systems were added, and each phase's systems resolve against
its own close doc)*, `cargo clippy
--workspace --all-targets --all-features -- -D warnings`, `nix flake check`,
all three xtask `--check` commands, and the gate smoke reproducing the
T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte via `cargo run -- run --agents 5000 --frames 300`.

What was checked without a GPU or a window: every numbered link in
`docs/ADR/README.md` (001–012) resolves to a file that exists in
`docs/ADR/`; `docs/horde-sim-headroom-architecture.html` and
`ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.html` contain no
`http://`/`https://` reference, no external `<script src>` or
`<link rel="stylesheet">`, and declare `color-scheme: dark` as their default
`:root`, with a `prefers-color-scheme: light` override present — the same
static checks used for every other architecture page in this repo, since
opening a real browser window is out of scope for a headless worker.

- [ ] Open `docs/horde-sim-headroom-architecture.html` in a real browser: it
      renders dark by default (no flash of light theme before the media query
      applies), nothing overflows the content column at common widths, and the
      browser's network panel shows no request while the page loads.
- [ ] Open `ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.html` the same way:
      the two ticket-flow SVGs (T1–T7, then T0/T8/T9) render without clipping,
      and the ticket-order table shows all ten tickets (T0 first, T7 last).
- [ ] Click every link in `docs/ADR/README.md` in a browser or editor preview
      and confirm each opens the right ADR body, including the two new
      "superseded in part" / "supplemented" notes at the top pointing at
      ADR 010, ADR 011 and ADR 012. *(T16: the index now also lists ADR 013,
      014 and 015, and two new tests —
      `adr_index_lists_every_adr_file` and `every_doc_link_resolves` — check
      that every ADR file is listed and every link under `docs/` resolves. The
      human check left is that each link's **text** matches the body it opens,
      which no test can decide.)*

## T10 review-fixes

Closes the two blockers and eight should-fixes from the four-dimension review of
`main..HEAD`. Adds no capability. One change is visible in a window: **item 3
made the hitbox ring smaller by a factor of √2 on both axes** — see the
correction note in the T8 section and the closed judgement call at the end of
T9, both of which were rewritten in place rather than left to read as still
true. On the tracked scenes the ellipse is now `67.9 × 33.9 px` where it was
`96 × 48`; the 2:1 aspect is unchanged.

Everything automatable is green and was run on this tree, not quoted:
`cargo fmt --all -- --check`; `cargo test --workspace --locked`;
`MMD_REQUIRE_GPU=1 cargo test -p mmd-engine --test render_correctness` (29/29,
`grep -c '^SKIP '` = 0, so `golden_frame_matches`, `rings_are_never_occluded`
and `a_ring_and_a_sprite_share_a_pass` really ran);
`cargo clippy --workspace --all-targets --all-features -- -D warnings`;
`nix flake check`; all three xtask `--check` commands;
`cargo test -p mmd-lab --test merge_gate`; and the gate smoke still printing
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte, **before and after** the ring change. `BODIED_STACK_HASH` and
`BODYLESS_GRID_PRE_SEPARATION_HASH` did not move. No golden image moved — the
golden scene is `static_demo_groups()` and carries no rings, and
`git status lab/` stayed clean throughout.

Four tests were mutation-checked rather than merely written: each was observed
**failing** under the exact mutation the reviewer used to prove its predecessor
vacuous, then observed passing after the mutation was reverted. The gate digest
is no longer prose — `the_gate_scene_walk_is_pinned_to_its_published_digest`
walks the real gate scene for its real 300 ticks, in the default
`cargo test --workspace`, with no GPU.

The ring was inspected off-screen, which is what makes the one window item below
a confirmation rather than the only evidence: the contact-pair scene was
rendered to PNG twice — once with the pre-T10 formula, once with the shipped one
— and looked at. Before, the two ellipses of a pair at exactly contact distance
visibly **intersect**. After, they **touch and do not cross**. What is left needs
eyes on a real window.

- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron` —
      **rings are tangent, not overlapping, for two agents at contact.** Find a
      pair pressed together and read the boundary: the two floor ellipses should
      meet at a point and neither should cut into the other. Rings that cross
      mean the √2 came back; rings with a visible gap mean it was applied twice.
      This is the item the whole overlay exists for, and it is the only box in
      this section that a headless worker cannot close.
- [ ] Same scene, sanity on the new size: the ellipse is noticeably **wider than
      the sprite and about two thirds of its height**. If it still reads as one
      full sprite tall, the pre-T10 quad is back.
- [ ] `cargo run -- run --agents 5000` — the gate scene at full population still
      starts, ticks and exits cleanly with the smaller rings, and the window
      title reads `millions_must_die — flow-field horde` with **no population
      figure** in it (item 1: the old title named a count `--agents 5001` is now
      refused for).
- [ ] `python3 tools/scenegen/gen_collision_scenes.py` from the workspace root,
      then `git status` — clean. The generator was rewritten to emit what the
      loader actually accepts; before T10 it emitted a `.ron` the loader
      refuses.
- [ ] Open `docs/horde-sim-headroom-architecture.html` in a real browser and
      read the new **render half**: the projection figure and the one-pass draw
      figure render without clipping, scroll horizontally inside their own
      containers rather than pushing the page, and the network panel shows no
      request.

## T1 placeholder-atlas-families

First ticket of the phase-1 RTS engine prototype plan. Frontloads all
phase-1 placeholder art generation (`rts/` unit+building+prop sheets, `ui/`
bitmap font) into `xtask atlases`. Nothing in the engine or renderer reads
these new families yet, so a human should observe **zero visible change**
from the current horde-sim scene. Everything automatable is green (`cargo
fmt`, `cargo test --workspace --locked`, clippy, `nix flake check`, all
three xtask checks including the new three-family `atlases --check`, and the
5000-agent/300-frame smoke).

- [ ] `cargo run -- run --agents 5000 --frames 300` — starts, ticks, and exits
      cleanly exactly as before this ticket; nothing on screen changed (this
      slice only adds generated PNGs on disk, wires nothing into the render
      path).
- [ ] Open `assets/sprites/generated/rts/worker.png` and
      `assets/sprites/generated/rts/soldier.png` in an image viewer: each is
      128x256px, a 4x8 grid of 32px placeholder humanoid sprites (flat blue
      body/light head for worker, flat red body for soldier), with a small
      yellow "facing pip" square that visibly moves around the body as you
      scan down the 8 direction rows.
- [ ] Open `assets/sprites/generated/rts/buildings.png`: rows 0-2 show three
      building silhouettes (HQ/Depot/Barracks) in columns 0-2, each with a
      lighter border; row 1 (under-construction) has a visible dotted/hatched
      look compared to row 0 (finished); row 2 columns 0-1 are resource nodes
      (blue crystal, purple gas) and columns 2-3 are visibly dimmer "depleted"
      variants of the same nodes. Rows 3-7 and row 0-1 column 3 are blank
      (transparent, checkerboard in most viewers).
- [ ] Open `assets/sprites/generated/rts/props.png`: row 0 has a green
      selection ring (col 0), a translucent green square (col 1), a
      translucent red square (col 2), and a white rally flag with a yellow
      pennant (col 3); row 1 has a blue diamond, a purple diamond, a yellow
      square, and a dark translucent panel fill. Rows 2-7 are blank.
- [ ] Open `assets/sprites/generated/ui/font.png`: 128x48px, a 16x6 grid of
      8x8 white-on-transparent glyphs. Spot-check: digits 0-9 and uppercase
      A-Z are legible letterforms; any lowercase letter cell (e.g. where `a`
      sits, column 1 row 2 counting from 0) renders as a plain hollow box, not
      a letter — that box is intentional, not a bug.
- [ ] `cargo run -p xtask -- atlases --check` from the workspace root prints
      an `atlases: ok (4 zombie png + manifest, 4 rts png + manifest, 1 ui
      png + manifest)` line and exits 0.
- [ ] `git status --porcelain` after the above — clean (no drift from running
      the check).

## T2 texture-table-and-scene-pass

Second ticket of the phase-1 RTS engine prototype plan. Generalises the
renderer's four hard-coded atlas groups into a flat 9-slot texture table and
introduces `ScenePass` (world / overlay / UI). The phase-1 sheets T1 generated
are now uploaded to the GPU at renderer construction, but nothing in `src/`
emits a UI layer yet, so a human should again observe **zero visible change**
in the running scene. Everything automatable is green (`cargo fmt`,
`MMD_REQUIRE_GPU=1 cargo test --workspace --locked`, the headless
`VK_DRIVER_FILES=/nonexistent` run, clippy, `nix flake check`, all three xtask
checks, and the three app smoke runs).

- [ ] `cargo run -- run --agents 5000 --frames 300` — starts, ticks, and exits
      cleanly, and the frame looks **pixel-for-pixel like it did before this
      ticket**: same sprites, same hitbox rings when `H` is on, no new panel,
      text or icon anywhere. The UI layer exists in the renderer but nothing
      fills it yet.
- [ ] With the window up, press `H` — hitbox rings still toggle and still draw
      *over* the sprites they annotate, including sprites standing in front of
      them. The overlay moved from being the last thing drawn to being the
      middle layer; it must not have gained an occluder.
- [ ] Press `F1` (overlay) and `Space` (pause), then `Esc` — all still behave
      exactly as before; the run prints `run: clean exit ...` and exits 0.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron
      --frames 300` — exits 0 and the sprite-collision scene renders as before.
- [ ] `MMD_REQUIRE_GPU=1 cargo test -p mmd-engine --test render_correctness
      golden_frame_matches` from the workspace root — passes. This is the
      byte-exact phase-0 frame gate; it must pass **without** anyone setting
      `MMD_UPDATE_GOLDEN=1`. If it fails, the scene pass changed the frame and
      the golden is right, not stale.
- [ ] `git diff --stat main -- lab/goldens/` — empty. No golden byte moved in
      this slice.
- [ ] `git status --porcelain` after all of the above — clean (no drift from
      running the checks; the golden-diff artifacts only appear under
      `target/` on a genuine failure).

## T3 bitmap-text-packer

Third ticket of the phase-1 RTS engine prototype plan. Adds the pure, headless,
GPU-free half of text rendering: `render::text::push_text` turns a `&str` into
UI sprite instances addressing the UI font slot (`SLOT_UI_FONT`). It draws
nothing on its own and nothing in `src/` calls it yet — the HUD that composes
it is a later ticket — so a human should again observe **zero visible change**
in the running scene. Everything automatable is green (`cargo fmt`,
`MMD_REQUIRE_GPU=1 cargo test --workspace --locked`, the headless
`VK_DRIVER_FILES=/nonexistent` run with the two new GPU text cases confirmed to
skip cleanly via `--nocapture`, clippy, `nix flake check`, all three xtask
checks, and the 5000-agent/300-frame smoke).

- [ ] `cargo run -- run --agents 5000 --frames 300` — starts, ticks, and exits
      cleanly, and the frame looks **pixel-for-pixel like it did before this
      ticket**: same sprites, same hitbox rings when `H` is on, no text or
      panel anywhere. The packer exists but nothing feeds it from `src/` yet.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron
      --frames 300` — exits 0 and the sprite-collision scene renders as before.
- [ ] `MMD_REQUIRE_GPU=1 cargo test -p mmd-engine --test render_correctness
      golden_frame_matches` from the workspace root — passes. This ticket adds
      no new draw call to any existing path, so the byte-exact phase-0 frame
      gate must still pass **without** anyone setting `MMD_UPDATE_GOLDEN=1`.
- [ ] `git diff --stat main -- lab/goldens/` — empty. No golden byte moved in
      this slice.
- [ ] `git status --porcelain` after all of the above — clean (no drift from
      running the checks; the golden-diff artifacts only appear under
      `target/` on a genuine failure).

## T4 camera-and-unprojection

Fourth ticket of the phase-1 RTS engine prototype plan. Adds `iso_unproject`
(the exact inverse of `iso_project`), `IsoView::{unproject, cell_at,
with_center_cell}`, and a new `render::Camera` with clamped cell-space
panning and edge-pan direction resolution. `IsoView::new` is unchanged and
still produces the fixed camera; `Runtime` builds that fixed `IsoView` the
same way it always has and nothing in `src/` calls `Camera` yet — a human
should again observe **zero visible change** in the running scene. Everything
automatable is green (`cargo fmt --all -- --check`, `cargo test -p mmd-engine
--test camera` (20/20), `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`,
clippy, `nix flake check`, all three xtask `--check` commands, and the
5000-agent/300-frame smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881` byte
for byte). The seven mandatory mutations (inverse-formula swap, stale
depth_bias, off-by-one clamp, edge-pan boundary `<` vs `<=`, dropped
outside-the-view guard, normalised diagonal, swapped `screen_dir_to_cells`
outputs) were each injected, confirmed red, and reverted to confirmed green.
The standing golden/atlas regression guard
(`git diff --stat main -- lab/goldens/ assets/sprites/generated/atlas_*.png
assets/sprites/generated/manifest.json`) is empty and `golden_frame_matches`
passed without regenerating anything.

- [ ] `cargo run -- run --agents 5000 --frames 300` — starts, ticks, and exits
      cleanly, and the frame looks **pixel-for-pixel like it did before this
      ticket**: same fixed camera centred on the destination, same sprites,
      same hitbox rings when `H` is on. Nothing pans; the camera does not
      exist on this code path yet.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron
      --frames 300` — exits 0 and the sprite-collision scene renders as
      before, camera fixed as always.
- [ ] `MMD_REQUIRE_GPU=1 cargo test -p mmd-engine --test render_correctness
      golden_frame_matches` from the workspace root — passes. This ticket
      makes the `IsoView` origin movable in general, which is exactly the kind
      of change that could silently move the golden; it must still pass
      **without** anyone setting `MMD_UPDATE_GOLDEN=1`.
- [ ] `git diff --stat main -- lab/goldens/
      assets/sprites/generated/atlas_0.png
      assets/sprites/generated/atlas_1.png
      assets/sprites/generated/atlas_2.png
      assets/sprites/generated/atlas_3.png
      assets/sprites/generated/manifest.json` — empty. No golden or atlas
      byte moved in this slice.
- [ ] `git status --porcelain` after all of the above — clean (no drift from
      running the checks).

## T5 rts-scenario-family

Fifth ticket of the phase-1 RTS engine prototype plan. Extends the scenario
contract with a new optional `rts:` block (`RtsSpec`: starting Crystal/Gas,
starting supply cap, an HQ footprint site, and Crystal/Gas resource node
cells) and a fourth scenario family, `rts_prototype_v1` — a 320x320,
horde-free (`hard_agent_count`/`stretch_agent_count` locked to `0`)
base-building map. The tracked scene `assets/scenarios/rts_prototype_v1.ron`
is committed with its `.sha256` sidecar and a deterministic generator,
`tools/scenegen/gen_rts_scene.py`. Nothing consumes the new block yet — no
entity store, no render layer, no CLI — so a human should observe **zero
visible change** to any existing scene. Everything automatable is green:
`cargo fmt --all -- --check`; `cargo test -p mmd-engine --test
scenario_contract` (64/64, including the full RTS validator negative-test
suite and `phase0_scene_bytes_are_unchanged`, which hashes every tracked
phase-0 `.ron` against its committed `.sha256` sidecar to prove the new
`#[serde(default)]` field did not force a regeneration); `MMD_REQUIRE_GPU=1
cargo test --workspace --locked` (all green, including
`render::instance::tests::the_tracked_scene_list_is_complete` and
`the_destination_is_centred`, both updated to include the new tracked file);
`cargo clippy --workspace --all-targets --all-features -- -D warnings`;
`nix flake check`; all three xtask `--check` commands; both scene generators
rerun with an empty `git status --porcelain` delta (verified via stable
sha256 across reruns); `sha256sum -c` on the new sidecar — `OK`; and the gate
smoke reproducing the T0-pinned digest
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881` byte
for byte, confirmed via a before/after `git stash` comparison on this exact
tree. Six mandatory mutations (dropped `#[serde(default)]`, RTS family added
to `free_workload` without its own zero-population lock, inverted
presence/absence xor, `<=`→`<` on the supply cap, skipped node reachability,
shifted obstacle-formula modulus) were each injected, confirmed red, and
reverted to confirmed green — see the commit body for the individual kills.
The standing regression guard
(`git diff --stat main -- lab/goldens/ assets/scenarios/fixtures/
assets/sprites/generated/atlas_0.png assets/sprites/generated/atlas_1.png
assets/sprites/generated/atlas_2.png assets/sprites/generated/atlas_3.png
assets/sprites/generated/manifest.json`) is empty and `golden_frame_matches`
passed without regenerating anything. What is left needs eyes on a real
scene file and a real CLI run — this ticket ships no window-visible change.

- [ ] `cargo run -- run --agents 5000 --frames 300` — starts, ticks, and
      exits cleanly, and the frame looks **pixel-for-pixel like it did
      before this ticket**: same gate scene, same sprites, same hitbox rings
      when `H` is on. This ticket only adds a new scenario file and a
      validator branch; nothing on the gate scene's code path changed.
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron
      --frames 300` — exits 0 and the collision demo scene renders exactly
      as before.
- [ ] `cargo run -- run --scenario assets/scenarios/rts_prototype_v1.ron
      --frames 3` — **fails**, printing `run failed: --agents 0 is not a
      runnable scene: the agent count must be > 0` and exiting nonzero. This
      is expected: the RTS scene is horde-free by design and nothing consumes
      it as a runnable population yet (that later ticket is T14: since then
      the scene runs under `cargo run -- rts`, not under `run`). A window
      opening or a crash with a different message would both be a defect.
- [ ] Open `assets/scenarios/rts_prototype_v1.ron` in a text editor: confirm
      it parses as valid RON at a glance (balanced parens, trailing commas)
      and that the `rts: Some((...))` block sits last, just before the
      closing `)`, with `start_crystal: 300`, `start_gas: 100`,
      `start_supply_cap: 10`, `hq_cell: (x: 160, y: 160)`, 8 `crystal_nodes`
      and 2 `gas_nodes`.
- [ ] `python3 tools/scenegen/gen_rts_scene.py` from the workspace root,
      then `git status` — no change to the working tree once the two
      generated files are already committed (rerunning is a no-op).
- [ ] `sha256sum -c <(sed 's|$|  assets/scenarios/rts_prototype_v1.ron|'
      assets/scenarios/rts_prototype_v1.sha256)` — prints `OK`.

## T6 entity-store-and-world

New module `mmd_engine::rts` (`entity.rs`, `economy.rs`, `world.rs`): a
preallocated SoA entity store with generational ids, and `RtsWorld`, which
seeds an HQ, six workers and ten resource nodes from `rts_prototype_v1.ron`
and advances a tick counter deterministically. Also new:
`testkit::RtsHarness`, the RTS sibling of `Harness`. Nothing renders, nothing
moves, no order or economy system exists yet — this is data-spine only, and
no existing command's behaviour changed (`cargo run -- run --agents 5000
--frames 300` reproduces the exact pre-change exit hash
`864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`, verified
via a before/after `git stash` comparison on this tree).

- [ ] `cargo test -p mmd-engine --test rts_world` — all tests pass (32 at T6;
      T7 appended the order/movement cases, so the count is 48 from T7 on).
      Skim the output for the seeding tests in particular
      (`world_seeds_the_scene`, `world_seeds_the_starting_stock`,
      `the_hq_sits_at_its_footprint_centre`,
      `workers_start_on_the_scenario_spawn_cells`) and confirm none are
      silently skipped or filtered out (0 ignored).
- [ ] `cargo run -- run --agents 5000 --frames 300` — starts, ticks, and
      exits cleanly, and the frame looks **pixel-for-pixel like it did
      before this ticket**: this slice adds a new module tree nothing else
      calls yet, so the gate scene's render path is untouched.
- [ ] `cargo doc -p mmd-engine --no-deps --open` (or browse
      `target/doc/mmd_engine/rts/index.html` directly) and skim the `rts`
      module's rustdoc: `EntityStore`, `EntityId`, `RtsWorld`, `Resources`,
      `Supply` should each read as a coherent, documented public API — no
      `TODO`s, no leftover private-looking names exposed by accident.
- [ ] Open `crates/mmd-engine/src/rts/world.rs` and confirm the seeding
      order comment (HQ, then crystal nodes, then gas nodes, then one worker
      per spawn cell) matches what `RtsWorld::from_scenario` actually does —
      a human sanity check that the "seeding order is part of the contract"
      claim in the source is not stale.

## T7 field-pool-and-movement

RTS units move. New `nav::field_pool::FieldPool` — eight preallocated flow
fields keyed by destination cell, exact-LRU evicted, rebuilt in place through
the new `FlowField::rebuild_in_place` + `FieldScratch` (so a rebuild reuses its
buffers instead of allocating). New `rts::orders` — `Order::{Idle, Move}`, a
per-slot `OrderTable`, per-kind walk speeds — and system 6 of `RtsWorld::tick`,
which walks every ordered unit one step down its field under the *same*
admissibility rule the horde walk uses. `RtsWorld` gains `order_move`,
`order_move_group`, `order_of` and `nav`. Nothing renders these units yet
(T12) and nothing issues orders from input yet (T8): reaching them by hand
means driving `RtsWorld` from a test or a scratch binary. The horde is
untouched

> **Restated by T15.** "A test or a scratch binary" is no longer the only way
> in. `cargo run -- rts --frames 1600 --inject-input-file
> assets/scenarios/rts_acceptance_v1.script` drives this movement path through
> the shipped binary, and `cargo run -- rts` drives it by hand. The claim these
> steps make about *this* slice's code is unchanged.
 — `sim/tick.rs` changed only two visibility keywords, and
`cargo run -- run --agents 5000 --frames 300` still exits on
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.

- [ ] `cargo test -p mmd-engine --test nav_pool` — 13 passed, 0 ignored. Skim
      for `rebuild_in_place_matches_build` (the pooled rebuild must stay
      bit-identical to `FlowField::build`) and
      `a_miss_may_grow_scratch_only_once`.
- [ ] `cargo test -p mmd-engine --test rts_world` — 48 passed, 0 ignored.
      Confirm `a_unit_reaches_its_destination`,
      `a_unit_walks_around_an_obstacle` and
      `an_unreachable_destination_clears_the_order` are all in the list and
      none was filtered out.
- [ ] `cargo test -p mmd-engine --test frame_allocations` — 12 passed.
      `movement_allocates_nothing` is the new one: 600 RTS ticks with six
      units under orders, zero heap allocations.
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0 and the exit
      line still reads
      `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
      A different hash means this ticket moved the horde walk, which it must
      not: read it, do not assume it.
- [ ] `cargo run -- run --frames 300` and watch the window: the horde scene
      must look exactly as it did before this ticket. RTS units are not drawn
      by any path yet, so a visible change here is a defect.
- [ ] `cargo doc -p mmd-engine --no-deps --open` (or browse
      `target/doc/mmd_engine/nav/field_pool/index.html` and
      `target/doc/mmd_engine/rts/index.html`) and read `FieldPool`'s docs:
      the LRU rule, the "invalidate every slot on `set_blocked`" rule, and
      the allocation note (a **miss** may grow the scratch heap once; a hit
      allocates nothing) should each read as a deliberate decision, not a
      leftover.
- [ ] Open `crates/mmd-engine/src/rts/orders.rs` and
      `crates/mmd-engine/src/sim/tick.rs` side by side and eyeball the two
      `step_admissible` bodies: they must be character-for-character the same
      rule, including the `diagonal_clear` arm. `rts::orders`'s unit test
      `rts_step_admissible_agrees_with_the_sim` is the automated version of
      this check — this one is the human confirming the duplication is
      deliberate and still honest.
- [ ] Sanity-check the speeds you can feel later: `WORKER_SPEED_CELLS_PER_SEC`
      is 10.0 and `SOLDIER_SPEED_CELLS_PER_SEC` is 8.0 (the horde's speed).
      If a phase-2 fight ever feels wrong, this is the constant to revisit.

## T8 selection

Turning a screen-space pointer gesture into a set of entity handles: pure
logic, no SDL, no rendering, no orders. New `rts::selection` —
`Selection` (a sorted, capped set of `EntityId`), `Pick` (unit → building →
node → nothing priority), `pick_at` (click), `box_select` (drag rectangle,
units only), `footprint_contains`/`footprint_min`, `normalise_rect`,
`is_drag`. `RtsWorld` gains `selection`, `selection_mut`, `click_select`,
`shift_click_select`, `box_select_into_selection`; `tick()` now drops a dead
entity from the selection as its last step; `state_hash()` now covers the
selection. Nothing draws the selection ring yet (T12), no HUD panel yet
(T13), no mouse plumbing yet (T14): reaching this by hand means driving
`RtsWorld` from a test or a scratch binary. The horde and the existing RTS
movement are untouched

> **Restated by T15.** Selection is reachable by hand since T14 (`cargo run --
> rts`, click and drag) and is exercised end to end by the tracked acceptance
> script's box select. The "scratch binary" note above describes T8's own
> moment, not today's tree.
 — `cargo run -- run --agents 5000 --frames 300` still
exits on `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.

- [ ] `cargo test -p mmd-engine --test rts_selection` — 31 passed, 0 ignored.
      Skim for `clicking_between_two_workers_picks_the_nearer`,
      `a_worker_standing_on_the_hq_wins_the_click` and
      `box_respects_the_camera`.
- [ ] `cargo test -p mmd-engine --test frame_allocations` — 13 passed.
      `selection_operations_allocate_nothing` is the new one: 100 box-selects
      of the full scene, zero heap allocations after warm-up.
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0 and the exit
      line still reads
      `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
      A different hash means this ticket moved the horde walk or the RTS
      movement it depends on, which it must not: read it, do not assume it.
- [ ] `cargo run -- run --frames 300` and watch the window: the horde scene
      must look exactly as it did before this ticket. There is no selection
      ring or box overlay drawn by any path yet, so a visible change here is
      a defect.
- [ ] Open `crates/mmd-engine/src/rts/selection.rs` and read `pick_at`'s
      three-step priority comment against its body: own units (nearest, not
      first-found) → own buildings (footprint contains the clicked cell) →
      resource nodes (occupy the clicked cell exactly) → nothing. Confirm the
      body really does check units before buildings, not the other way
      round.
- [ ] Sanity-check `UNIT_PICK_RADIUS_SCALE = 1.0` and
      `MAX_SELECTION = MAX_ENTITIES` (2048) against
      `docs/DESIGN.md`'s "Unlimited unit selection" decision — the selection
      cap is deliberately the entity store's own ceiling, not a smaller
      RTS-traditional 12. *(T16: `docs/DESIGN.md` now states that mapping in
      the decision itself, so this is a two-way check between the doc and the
      constant rather than an inference.)*

## T9 gather-loop

Two resources, one worker round trip: mine a load, haul it to the HQ, bank
it, repeat until the node is empty. New `EntityStore::{carry, set_carry}`
(two more preallocated columns, `carry_kind`/`carry_amount`); new
`rts::orders::{GatherPhase, Order::Gather}`; new `RtsWorld::{order_gather,
order_gather_group, nearest_drop_off}`; a new gather system runs as step 5 of
`tick()`, before movement, and the movement system now also drives both
`Gather` phases (arrival only clears an `Order::Move`, never a `Gather`).
Nothing renders a carried load or a mining animation yet (T12); no HUD (T13);
no input plumbing to issue a gather order from a click (T14): reaching this
by hand means driving `RtsWorld` from a test or a scratch binary. The horde,
existing RTS movement, and selection are untouched

> **Restated by T15.** The gather loop is reachable by hand since T14 and is
> driven end to end by the tracked acceptance script (box select → right-click
> the crystal node → the exit line's `crystal=` is positive after buying 350
> crystal of buildings and units).
 — `cargo run -- run
--agents 5000 --frames 300` still exits on
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.

- [ ] `cargo test -p mmd-engine --test rts_economy` — 30 passed, 0 ignored.
      Skim for `a_full_round_trip_banks_crystal`, `a_full_round_trip_banks_gas`,
      `the_worker_keeps_cycling` and `six_workers_on_one_node_all_deliver`.
- [ ] `cargo test -p mmd-engine --test rts_world` — 48 passed, 0 ignored (no
      new tests here; this is the "T9 didn't regress T7/T8's movement and
      order tests" check).
- [ ] `cargo test -p mmd-engine --test frame_allocations` — 14 passed.
      `the_gather_loop_allocates_nothing` is the new one: 2 000 RTS ticks
      with six workers split across two nodes and one HQ drop-off, zero heap
      allocations after warm-up.
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0 and the exit
      line still reads
      `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
      A different hash means this ticket moved the horde walk or the RTS
      movement/selection it depends on, which it must not: read it, do not
      assume it.
- [ ] `cargo run -- run --frames 300` and watch the window: the horde scene
      must look exactly as it did before this ticket. There is no worker
      cargo, mining animation, or resource-counter overlay drawn by any path
      yet, so a visible change here is a defect.
- [ ] Open `crates/mmd-engine/src/rts/world.rs` and read the `gather` method
      against `tick()`'s comment listing the reserved system order (1.
      commands, 2. camera, 3. construction, 4. production, 5. orders — this
      is where `gather` sits, 6. movement, 7. supply recount). Confirm
      `gather()` really is called before `movement()`, not after — the doc
      comment on `gather` explains why the order matters (a phase change
      this tick must also govern movement this same tick).
- [ ] Sanity-check the constants in `crates/mmd-engine/src/rts/economy.rs`
      against feel, not just correctness: `WORKER_CARRY_CAPACITY = 8`,
      `GATHER_TICKS = 60` (one second per full load at 60 Hz),
      `GATHER_REACH_CELLS = 2.0`, `DROP_OFF_REACH_CELLS = 1.0`. If a
      round trip ever feels too fast or too slow once it renders, these are
      the knobs.

## T10 building-placement

Buildings exist, cost, take time, block pathing and grant supply. New
`rts::build` (`Placement`, `PlacementError`, `building_cost`, `build_ticks`,
`supply_grant`, `footprint_cells`, `placement_valid`, the per-kind cost/time/
supply constants). `RtsWorld` gains `placement`, `begin_placement`,
`cancel_placement`, `confirm_placement`, `cancel_construction`, `is_site`,
`order_build`; `Order` gains `Build { site, field_slot }`; a new construction
system runs as step 3 of `tick()`, before orders and movement, so a site that
finishes this tick is finished for everything downstream that same tick. A
worker standing on a chosen footprint does **not** block placement — only
terrain, another building (finished or mid-construction), or a resource node
does. `drop_off_approach_cell` (T9's HQ-delivery routing) was rewritten on top
of the same new ring-scan the Build order uses: since a finished building's
footprint is now blocked, a worker can no longer be routed to its centre, only
to the nearest free cell in the 4-connected ring just outside it. Nothing
renders a build ghost, a construction site, or a progress bar yet (T12); no HUD
build menu (T13); no CLI to trigger a placement (T14): reaching this by hand
means driving `RtsWorld` from a test or a scratch binary. The horde, existing
RTS movement, selection and the gather loop are untouched in outcome

> **Restated by T15.** Placement is reachable by hand since T14 (`W`/`E` then a
> left click) and the tracked acceptance script places both a Depot and a
> Barracks through the shipped binary; the run's `buildings=3` is asserted.
 (though
the gather loop's HQ-approach routing was internally rewritten to share the new
ring-scan) — `cargo run -- run --agents 5000 --frames 300` still exits on
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.

- [ ] `cargo test -p mmd-engine --test rts_build` — 37 passed, 0 ignored. Skim
      for `a_depot_finishes_in_its_documented_time`,
      `a_finished_depot_blocks_navigation`, `cancel_refunds_the_full_cost` and
      `a_gatherer_still_delivers_after_the_hq_is_stamped`.
- [ ] `cargo test -p mmd-engine --test rts_economy` — 30 passed, 0 ignored (no
      new tests here; this is the "T10's approach-cell rewrite didn't regress
      T9's delivery" check — the riskiest part of this ticket).
- [ ] `cargo test -p mmd-engine --test frame_allocations` — 15 passed.
      `construction_allocates_nothing` is the new one: three simultaneous
      Depot sites, six attending workers, 600 RTS ticks including at least one
      site finishing (and therefore one footprint stamp, one flow-field
      cache invalidation, and the misses that follow), zero heap allocations.
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0 and the exit
      line still reads
      `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
      A different hash means this ticket moved the horde walk or something it
      depends on, which it must not: read it, do not assume it.
- [ ] `cargo run -- run --frames 300` and watch the window: the horde scene
      must look exactly as it did before this ticket. There is no build
      ghost, construction site, or supply-cap overlay drawn by any path yet,
      so a visible change here is a defect.
- [ ] Open `crates/mmd-engine/src/rts/world.rs` and read the `construction`
      method against `tick()`'s comment listing the reserved system order (1.
      commands, 2. camera, 3. construction — this is where it sits, 4.
      production, 5. orders, 6. movement, 7. supply recount). Confirm
      `construction()` really is called before `gather()` and `movement()`,
      not after.
- [ ] Open `crates/mmd-engine/src/rts/orders.rs` and read
      `building_approach_cell`'s doc comment against its body: the ring scan
      order (top edge left→right, right edge top→bottom, bottom edge
      right→left, left edge bottom→top) is 4-connected only — no diagonal
      corner cells — which is what keeps a routed worker within
      `DROP_OFF_REACH_CELLS`/`BUILD_REACH_CELLS` of the footprint it was sent
      to. A version that included the four corner cells would intermittently
      strand a worker just outside reach; that regression is exactly what
      `rts_economy`'s delivery tests above exist to catch.
- [ ] Sanity-check the constants in `crates/mmd-engine/src/rts/build.rs`
      against feel, not just correctness: Depot costs 100 crystal and takes
      180 ticks (3 s) attended; Barracks costs 150 crystal + 25 gas and takes
      300 ticks (5 s); HQ (not player-placeable in phase 1, but costed for
      completeness) costs 400 crystal and takes 600 ticks (10 s). Only HQ and
      Depot grant supply (10 each); Barracks grants none. A second worker
      attending the same site does **not** speed it up
      (`EXTRA_BUILDERS_SPEED_UP = false`) — that is a deliberate, documented
      deferral, not an oversight.

## T11 production-and-supply

Finished HQs queue Workers; finished Barracks queue Soldiers. Enqueue debits
resources and reserves supply immediately, cancellation refunds both, queues
hold at most five entries, completed units spawn beside their producer and walk
to an optional rally point. Supply usage is recounted every tick from live
units plus queued reservations. Nothing renders these RTS units or exposes
production input yet (T12–T14), so manual checks use tests/source; existing
horde window output must remain unchanged.

> **Restated by T15.** Production is rendered (T12), shown in the HUD (T13) and
> driven by `A`/`S` from the keyboard (T14). The tracked acceptance script
> queues a Worker at the HQ and a Soldier at the Barracks, and the run's
> `units=`/`supply=` fields are asserted against both.


- [ ] `cargo test -p mmd-engine --test rts_production` — 39 passed, 0 ignored.
      Skim for `queueing_cannot_exceed_the_cap`,
      `a_supply_blocked_enqueue_does_not_charge`,
      `a_barracks_can_be_queued_the_tick_it_finishes`,
      `production_stops_when_the_store_is_full`, and
      `production_is_reproducible`.
- [ ] `cargo test -p mmd-engine --test frame_allocations` — 16 passed.
      `production_allocates_nothing` measures 600 ticks with HQ and Barracks
      queues active, including Worker and Soldier completions, at zero heap
      allocations.
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0; final line reads
      `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
      Window remains phase-0 horde output; no RTS production UI renders yet.
- [ ] Open `crates/mmd-engine/src/rts/production.rs`: confirm queue cap `5`,
      Worker cost/time `50 crystal / 300 ticks`, Soldier cost/time
      `50 crystal + 25 gas / 360 ticks`, plus legal pairs HQ→Worker and
      Barracks→Soldier only.
- [ ] Open `crates/mmd-engine/src/rts/world.rs`: confirm `tick()` order remains
      construction → production → gather/orders → movement → supply recount,
      then selection self-heal. Production before construction or recount
      before movement violates reserved system order.
- [ ] Run production flow through `RtsHarness`: enqueue four Workers at starting
      supply 6/10, confirm fifth returns `SupplyBlocked`; cancel one, confirm
      crystal and one supply return; set rally `(200, 200)`, produce one Worker,
      confirm it ends within `ARRIVAL_RADIUS_CELLS` of rally.

## T12 world-render-packing

The RTS world now packs into instances — units, buildings, sites, nodes,
selection rings, the placement ghost, rally flags and the drag box — and owns
the camera the whole frame projects through. Nothing wires input or the HUD yet
(T13–T14), so there is still no interactive RTS window: manual checks read the
packed frame, the two device probes, and the unchanged horde output.

> **Restated by T15.** There *is* an interactive RTS window since T14
> (`cargo run -- rts`), and the packed frame is rendered every frame of the
> tracked acceptance run. The steps below still read the packer directly, which
> is what keeps them pointed at this slice.


- [ ] `cargo test -p mmd-engine --test rts_pack` — 36 passed, 0 ignored. Skim
      for `every_entity_packs_exactly_once`, `sprites_stand_on_their_ground_point`,
      `culling_uses_the_same_rect_as_the_horde`, `selection_rings_are_procedural`,
      `the_ghost_covers_the_whole_footprint` and `pan_dir_moves_the_camera_each_tick`.
- [ ] `MMD_REQUIRE_GPU=1 cargo test -p mmd-engine --test gpu_smoke` — 11 passed,
      3 ignored (the pre-existing `#[ignore]` device cases). The two new cases
      must **run**, not skip: `the_frame_renders` rasterises the packed base
      scene (more than 1 000 non-transparent pixels) and
      `the_ghost_draws_over_the_world` proves a Depot ghost dropped on the HQ
      tints the HQ's own pixels red.
- [ ] `VK_DRIVER_FILES=/nonexistent cargo test -p mmd-engine --test gpu_smoke --
      --nocapture` — both new cases print `SKIP …: no GPU device on this host`
      and the run still exits 0. A skip that reported success without printing
      is the failure mode this step exists to catch.
- [ ] `cargo test -p mmd-engine --test frame_allocations` — 17 passed.
      `pack_frame_allocates_nothing` measures 600 packs of the full scene with a
      selection, a rally flag, a pending ghost and a live drag, at zero heap
      allocations.
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0; final line reads
      `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
      The window is still phase-0 horde output: this slice adds no app path.
- [ ] Open `crates/mmd-engine/src/rts/pack.rs`: confirm every instance pushed to
      `frame.overlay` is a `SpriteInstance::ring` and every *textured* depth-off
      instance (rally flag, ghost tiles, silhouette, drag edges) goes to the
      slot-7 group in `frame.ui`. The overlay layer binds texture slot 0, so a
      textured instance there would sample the wrong sheet.
- [ ] Open `crates/mmd-engine/src/rts/world.rs`: confirm `tick()` runs the camera
      pan as system 2, before construction, and that `state_hash` feeds the
      camera centre in after the production table. A camera outside the hash
      means a replay that ends looking elsewhere would still claim to reproduce.
- [ ] Read a packed frame through `RtsHarness`: pack the scene at cursor
      `(960, 540)`, confirm 17 world instances split 6 / 0 / 11 across slots
      4 / 5 / 6; select the HQ and confirm exactly one overlay ring sized
      `ring_quad_size_px(8, 4, 6)`; `begin_placement(Depot)` and confirm 64 red
      tiles plus one silhouette, then move the cursor one cell `+x` and confirm
      every ghost quad shifted by `(+4, +2)` px.

## T13 hud

The HUD is pure layout: `rts::pack_hud` reads world state and appends a
top resource bar and a bottom command panel (selection, production, build
menu) to an already-packed frame's UI groups. No input, no CLI, no device
wiring yet (T14), so there is still no interactive RTS window — manual checks
read the packed frame, the two device probes, and the unchanged horde output.

> **Restated by T15.** The window exists since T14, and the tracked acceptance
> script turns the stdout HUD on (`F1`) for its final frames, so the HUD is now
> also exercised by the merge gate rather than only by these steps.

- [ ] `cargo test -p mmd-engine --test rts_hud` — 30 passed. Skim for
      `hud_appends_and_does_not_clear`, `the_top_bar_shows_the_stock`,
      `a_selected_worker_names_itself`, `the_build_menu_shows_costs` and
      `the_hud_never_panics_on_a_stale_primary`.
- [ ] `MMD_REQUIRE_GPU=1 cargo test -p mmd-engine --test gpu_smoke` — 13
      passed, 3 ignored (the pre-existing `#[ignore]` device cases). The two
      new cases must **run**, not skip: `the_hud_renders_legible_pixels`
      rasterises the top bar's icons and digits (more than 200 differing
      pixels against the bare panel), and `the_hud_draws_over_the_world`
      proves a world sprite placed under the bottom panel is overwritten.
- [ ] `VK_DRIVER_FILES=/nonexistent cargo test -p mmd-engine --test gpu_smoke
      -- --nocapture` — the run still exits 0 (this host's GPU discovery is
      not gated by that variable; see the T13 report for detail).
- [ ] `cargo test -p mmd-engine --test frame_allocations` — 18 passed.
      `pack_hud_allocates_nothing` measures 600 `pack_frame` + `pack_hud`
      pairs with a selection, a rally point and a live production queue, at
      zero heap allocations.
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0; final line
      reads `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
      The window is still phase-0 horde output: this slice adds no app path.
- [ ] Open `crates/mmd-engine/src/rts/hud.rs`: confirm every push into
      `frame.ui[0]` is a `Prop::PanelFill` quad or an icon, and every glyph
      goes to `frame.ui[1]` via `push_text` — a textured panel pushed into the
      font group would draw with the wrong texture bound.
- [ ] Read a packed HUD through `RtsHarness`: pack the tracked scene fresh
      (no selection), confirm the top bar reads `300` crystal, `100` gas and
      `6/10` supply; select the HQ, confirm the selection block reads
      `SELECTED 1` / `HQ` / `READY`; confirm the build menu always lists three
      rows (`[Q] HQ 400C`, `[W] DEPOT 100C`, `[E] BARRACKS 150C 25G`) with the
      HQ row red (300 crystal on hand, cost 400).

## T14 rts-cli-subcommand

The first ticket where a human can actually drive the RTS slice
interactively: `cargo run -- rts` opens a real window over
`assets/scenarios/rts_prototype_v1.ron` (a 320x320 map, HQ near the
map centre, six workers spawned just south of it, a crystal node and a gas
node on each side). Mouse and keyboard now drive `mmd_engine::rts::RtsWorld`
directly; everything below the app layer (T6-T13) is unchanged.

- [ ] `cargo run -- rts` — a window opens on the HQ. The bottom panel shows a
      resource bar (`300` crystal, `100` gas, supply `6/10`), a build menu
      (`[Q] HQ`, `[W] DEPOT`, `[E] BARRACKS`), and an empty selection block.
      Press `Esc`: the window closes and the process exits cleanly (check the
      exit code is `0`, e.g. `echo $?`).
- [ ] `cargo run -- rts` again. Left-click directly on one of the six workers
      just south of the HQ: the selection block now reads `SELECTED 1` /
      `WORKER`, and a green ring appears under the clicked worker. Left-click
      empty ground: the ring disappears and the selection block goes back to
      empty.
- [ ] Left-click-drag a rectangle around all six workers: the selection block
      reads `SELECTED 6` and six rings appear. Right-click a patch of open
      ground away from the base: all six workers turn and walk there.
- [ ] With the six workers still selected, right-click the nearby crystal
      node: the workers walk to it, and over the next few seconds the top
      resource bar's crystal count climbs above `300` and keeps climbing as
      workers cycle between the node and the HQ.
- [ ] Press `W`: a Depot-shaped ghost (a footprint of tiles plus a building
      silhouette) follows the mouse, tinted green where it could be placed and
      red over the HQ, a node, or off the buildable terrain. Press `X`: the
      ghost disappears. Press `W` again and left-click on clear ground away
      from the HQ and the nodes: the ghost is replaced by an actual Depot
      under construction, a worker walks to it and starts building, and the
      build menu row's cost readout goes red if you can no longer afford the
      next one.
- [ ] Press `W`, then right-click anywhere: the ghost cancels (same as `X`)
      and **no** unit moves — a right click while a ghost is pending must
      never also issue a move/gather order.
- [ ] Left-click the HQ itself (a ring appears around its footprint,
      selection block reads `SELECTED 1` / `HQ`), then press `A`: the
      production block shows a Worker queued; the crystal bar drops by 50;
      a few seconds later a new worker walks out of the HQ and `units`-style
      counts (visible via `--inject-input ...; --frames N` runs, see below)
      rise by one.
- [ ] Hold an arrow key (e.g. Right): the camera pans smoothly toward that
      edge. Release the key: panning stops immediately — the view must not
      keep drifting after key-up. Move the mouse to within ~12px of a window
      edge: the camera also edge-pans in that direction while the pointer
      stays there.
- [ ] Press `F1`: the terminal now prints one `rts: hud tick=... crystal=...
      gas=... supply=.../ ... sel=... ghost=...` line per frame. Press `F1`
      again: the lines stop. Press `Space`: the world visibly freezes (units
      stop moving, resource counts stop climbing even while gathering) until
      `Space` is pressed again.
- [ ] `cargo run -- rts --frames 600 --inject-input
      "1:drag:880,540,940,590;5:rclick:920,458" ` (no window needed —
      add `SDL_VIDEODRIVER=offscreen` first if you want to confirm the
      headless path) — exits `0`; the final `rts: clean exit ...` line's
      `crystal=` is above `300` (the six workers were sent to gather).
- [ ] `cargo run -- run --agents 5000 --frames 300` — still exits 0 with
      `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
      This ticket adds a parallel command; `run` must read as before.

## T15 end-to-end-acceptance

The ticket that makes "the phase-1 slice works" falsifiable. One tracked
script, `assets/scenarios/rts_acceptance_v1.script`, drives select → gather →
build → produce, and is asserted twice: `crates/mmd-engine/tests/rts_acceptance.rs`
walks the same sequence through `RtsWorld` and checks fourteen milestones by
world state, and `tests/rts_acceptance.rs` runs the shipped binary on the
script and reads the milestones off the exit line. New CLI surface:
`--inject-input-file <PATH>` (same grammar as `--inject-input`, plus newline
separators and `#` comments; the two flags are mutually exclusive at the clap
layer). `docs/05-testing.md`'s required merge gate gains the acceptance
command as its last line.

Everything automatable is green and was run on this tree, not quoted:
`cargo fmt --all -- --check`; `cargo test -p mmd-engine --test rts_acceptance`
(3/3); `cargo test --test rts_acceptance` (10/10);
`MMD_REQUIRE_GPU=1 cargo test --workspace --locked` (every block ok, 0 failed —
so the GPU cases really ran rather than skipping, including `gpu_smoke`'s 13);
`VK_DRIVER_FILES=/nonexistent cargo test --workspace --locked` (also green;
on this run that variable *did* force the no-GPU path and the GPU cases
skipped — the T13 note that it does not is a real observation of a different
run, so treat this variable as unreliable rather than as a contract);
`cargo clippy --workspace --all-targets --all-features -- -D warnings`;
`nix flake check`; all three xtask `--check` commands; `cargo run -- run
--agents 5000 --frames 300` still exiting on
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
byte for byte, plus both collision scenes at exit 0; the acceptance run itself
exiting 0 on `tick=1449 frames=1449 quit=true crystal=86 gas=50 supply=9/20
units=8 buildings=3 nodes=10`; an empty
`git diff --stat main -- lab/goldens/ assets/scenarios/fixtures/
assets/sprites/generated/atlas_*.png assets/sprites/generated/manifest.json`;
`host_offscreen_matches_tracked_golden` passing **without**
`MMD_UPDATE_GOLDEN`; and `cargo tree -e features | grep -c testkit` = 0.

One host-level hazard was hit and mitigated while writing this: eight
independent 1 600-frame GPU subprocess runs in `tests/rts_acceptance.rs`,
running in parallel with the other GPU test binaries, reproduced
`VK_ERROR_DEVICE_LOST` twice in five full-suite runs (the run then exits 1 and
the case fails on the exit code). The five cases that only read the exit line
now share **one** run behind a `OnceLock`, so the file spends two heavy
processes instead of eight — `the_acceptance_run_is_deterministic` still spends
a second, independent process, or it would be comparing a string with itself.
Six consecutive `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` runs were
clean afterwards. If it ever returns, it is this host's device under
parallelism, not the acceptance run's behaviour.

Six mandatory mutations were each injected, observed red, reverted and
observed green — see the commit body for the individual kills. The script
needed **no** retiming; the engine test's own step counts did (the ticket's
400 ticks for the Depot is short of the builder's walk from the crystal node,
so the milestone steps are 900/900/300/900/400 ticks). What is left needs eyes
on a real window.

- [ ] `cargo run -- rts --frames 1600 --inject-input-file
      assets/scenarios/rts_acceptance_v1.script` — watch the whole run in a
      real window without touching the mouse or keyboard: six workers get
      box-selected, walk to the crystal node north-west of the base and start
      cycling loads back to the HQ; a Depot goes up south-east of the base; a
      new Worker walks out of the HQ; a Barracks goes up west of the base; a
      Soldier walks out of the Barracks. The run quits itself and exits `0`
      (`echo $?`).
- [ ] Same run, the last ~50 frames: one `rts: hud tick=… crystal=… gas=…
      supply=…/… sel=… ghost=none` line per frame appears in the terminal (the
      script presses `F1` at frame 1400). The final `rts: clean exit …` line
      reads `units=8 buildings=3 supply=9/20` and a positive `crystal=`.
- [ ] Open `assets/scenarios/rts_acceptance_v1.script` in an editor: every
      entry is `FRAME:KIND[:ARGS]`, the comments explain what each block is
      for, and every coordinate in it is one of the eight documented in
      `crates/mmd-engine/tests/rts_acceptance.rs`'s `SCRIPT_COORDS` table.
      Change one coordinate by hand and re-run `cargo test -p mmd-engine
      --test rts_acceptance` — `the_script_coordinates_hit_what_they_name`
      must fail. Put it back.
- [ ] `cargo run -- rts --frames 30 --inject-input-file
      assets/scenarios/rts_acceptance_v1.script` — **fails**, exits `1`, and
      names the entries that never fired. A budget too short to reach the end
      of a script must never look like a pass.
- [ ] `cargo run -- rts --inject-input 1:key:esc --inject-input-file
      assets/scenarios/rts_acceptance_v1.script` — **fails** with a clap usage
      error and exit `2`. Two sources of scripted input must be refused rather
      than silently ranked.
- [ ] `cargo run -- rts --inject-input-file /nope.script` — **fails**, exits
      `1`, and the message names `/nope.script`. No stack trace.
- [ ] Read `docs/05-testing.md`'s **Required merge gate** block: the last
      command is the acceptance run, and the sentence under the block says it
      asserts behaviour and an exit code and consumes no measurement number.
- [ ] `cargo run -- rts` with no flags — still opens the interactive window
      from T14 and still responds to the mouse and keyboard. The script path
      is an addition; it must not have taken over the default.

## T16 docs-and-phase-close

The ticket that closes phase 1 on the record rather than on the code. Three
ADRs (013, 014, 015) and the architecture page were re-read against the shipped
source and amended where they diverged; a new
`docs/rts-engine-prototype-functional-close.md` publishes what phase 1 proves,
what it does not, the five deviations from plan, and fourteen known gaps; every
index (`docs/README.md`, `docs/ADR/README.md`, `docs/CONTEXT.md`,
`docs/DESIGN.md`, `docs/05-testing.md`, `docs/GLOSSARY.md`, `AGENT.md`,
`README.md`) now points at it. No gameplay or render behaviour changed.

Three contract tests are new in `tests/validation_contract.rs` —
`phase1_close_doc_names_only_real_tests` (parses the close doc's
`System | Test binary | Named tests` table and resolves every name against a
`#[test]` scan of the binary it claims, plus a reverse scan of the prose),
`adr_index_lists_every_adr_file`, and `every_doc_link_resolves` — and
`every_system_has_a_test` grew six phase-1 rows (`SCOPE_SYSTEM_COUNT` 14 → 20).
Because a phase-1 system must not be published by the *phase-0* close doc, each
`SystemCoverage` entry now names its own close document.

Two documents disagreed with the code, and the **code comment** was wrong both
times, so the code was corrected and the ADRs record it: `ScenePass::overlay`'s
doc comment still listed placement tiles as overlay content (they are textured
and live in a `ui` group), and `render/instance.rs` still called zoom phase-1
work (ADR 014 defers it). Both are comment-only edits; no behaviour moved.
Two dead links in the superseded `docs/technical-prototype-results.md` (to the
consolidated `02-prototype-roadmap.md`) were repointed at `CONTEXT.md#roadmap`
so `every_doc_link_resolves` has nothing to excuse.

Everything automatable is green and was run on this tree, not quoted:
`cargo fmt --all -- --check`; `cargo test --test validation_contract` (10/10);
`MMD_REQUIRE_GPU=1 cargo test --workspace --locked` (48 `test result: ok`
blocks, 0 failed); `VK_DRIVER_FILES=/nonexistent cargo test --workspace
--locked` (48 blocks, 0 failed); `cargo clippy --workspace --all-targets
--all-features -- -D warnings`; `nix flake check`; all three xtask `--check`
commands (`4 zombie + 4 rts + 1 ui`); `cargo run -- run --agents 5000 --frames
300` still exiting on
`hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`;
both collision scenes at exit 0; the acceptance run exiting 0 on `tick=1449
frames=1449 crystal=86 gas=50 supply=9/20 units=8 buildings=3 nodes=10`;
`golden_frame_matches` passing **without** `MMD_UPDATE_GOLDEN`; an empty
`git diff --stat main -- lab/goldens/ assets/scenarios/fixtures/
assets/sprites/generated/atlas_*.png assets/sprites/generated/manifest.json`;
and `cargo tree -e features | grep -c testkit` = 0.

The five mandatory mutations were each injected, observed red, reverted and
observed green: a renamed mapped test, a fictional row in the close doc's
table, a `144 fps` sentence in the close doc, ADR 014 deleted from the index,
and a `docs/README.md` link pointed at a missing file. Five injected, five
killed, no survivors.

What is left needs eyes, a browser, or a judgement no test can make.

- [ ] Open `docs/rts-engine-prototype-architecture.html` in a real browser: it
      renders dark by default with no flash of light theme, the four SVGs (two
      worlds, the tick order, the gather state machine) render without
      clipping at common widths, and the browser's network panel shows no
      request while the page loads.
- [ ] On that page, read the box titled **The rule a contributor can get
      wrong** and then open `crates/mmd-engine/src/rts/pack.rs`. The page says
      `overlay` carries procedural rings only and everything textured goes in
      `ui`; confirm `pack_frame`'s section 2 pushes only
      `SpriteInstance::ring(...)` and that the rally flag, ghost and drag box
      in section 3 all go through `frame.prop_group()`.
- [ ] Read `docs/rts-engine-prototype-functional-close.md` end to end with the
      code open beside it. Every claim must be one you can trace: the six
      systems, the five deviations, the fourteen gaps. Anything you cannot
      trace is a defect in this document, not a nuance.
- [ ] In that close doc, check the tone of gap 13 (known flakiness). It must
      read as *unexplained and unfixed* — `gpu_smoke::the_hud_draws_over_the_world`
      failing 3/3 on one machine and passing 2/2 on another at the same commit
      is recorded, not resolved. If a future reader could mistake it for
      "handled", it is worded wrong.
- [ ] Read the new **Corrected during implementation.** bullets at the end of
      ADR 013, 014 and 015 against the code they name: the two `sim/`
      visibility keywords (`git diff main -- crates/mmd-engine/src/sim/` must
      show exactly two `pub`/`pub(crate)` changes and one `#[cfg(test)]`
      re-export), the placement ghost inside `RtsWorld::state_hash`, the four
      test hooks, and `Order::Move`-only arrival in `RtsWorld`'s mover.
- [ ] `git diff main -- crates/mmd-engine/src/sim/` — confirm with your own
      eyes that no signature and no body changed. This is the one promise that
      keeps the phase-0 hash meaningful, and it is worth checking by hand
      rather than trusting the ADR that claims it.
- [ ] Read `README.md`'s new phase-1 paragraph, then run the command in it in
      a real window. A new reader must be able to see the prototype from that
      paragraph alone, and the hotkeys it lists (`Q`/`W`/`E`, `X`, `A`/`S`,
      `R`, arrows) must be the ones that actually respond.
- [ ] Click every link in `docs/README.md`'s new **Phase 1 architecture**
      section and in the close doc's **Related** section. `every_doc_link_resolves`
      proves the targets exist; only a human can say the link text describes
      what opens.
- [ ] Read `docs/GLOSSARY.md`'s new **RTS (phase 1)** table out loud against
      the code refs. Each word must be one you would actually say in a chat
      message ("fix the ghost", "the fieldpool thrashes"); any word you would
      not say is dead weight and should be replaced, not kept.
- [ ] Skim `docs/CONTEXT.md`'s roadmap item 1 and `AGENT.md`'s Status section
      side by side. They must agree with each other and with the close doc on
      what phase 1 claims — especially that performance is unmeasured and that
      nothing cross-platform was verified.
