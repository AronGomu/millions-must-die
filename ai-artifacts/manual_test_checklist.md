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
`SCOPE_SYSTEM_COUNT = 14` and `no_perf_claim_in_docs`), `cargo clippy
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
      ADR 010, ADR 011 and ADR 012.

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
