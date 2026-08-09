# T9: Isometric projection and depth

**Plan:** `./ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** T8
**Commit outcome:** The render layer projects the world 2:1 isometric and draws
depth-ordered; the simulation is untouched and every state hash holds.

## Context (self-contained)

- `docs/CONTEXT.md` and `docs/DESIGN.md` both commit the project to a
  StarCraft-like read, and `.tmp/RESEARCH_they_are_billions_performance.md`
  § B.6.4 names isometric depth sorting as *the crux*. The renderer today is
  flat top-down: `pack_instance_groups` maps a cell straight to an axis-aligned
  pixel quad, and `shaders/sprite.hlsl` writes `z = 0.0` with no depth
  attachment and no ordering beyond atlas-group order.
- This slice: the projection and the ordering. **The simulation stays in
  Cartesian cell space** — that is precisely what keeps `BODIED_STACK_HASH`,
  `BODYLESS_GRID_PRE_SEPARATION_HASH` and T0's pinned gate digest alive. Moving
  the sim to isometric space would break the flow field's grid assumptions and
  every pinned digest in the repo, for no gameplay gain: an isometric game is a
  Cartesian game with a different camera.
- Out of scope here: camera control. This ticket ships a **fixed** view centred
  on the destination cell. Scrolling, edge-pan, zoom, selection and terrain
  tiles are Phase 1.
- Assumptions in force:
  - Tile is `2 × cell_size_px` wide by `cell_size_px` tall — 8 × 4 px at the
    scenario's `cell_size_px: 4`. The 480 × 270 grid therefore projects to a
    3 000 × 1 500 px diamond, which is **larger than the 1920 × 1080 view**.
    That is correct for an RTS and is why this ticket also adds a cull.
  - Depth correctness with alpha comes from an alpha-test `clip()`, not from
    sorting. Pixel art is effectively 1-bit alpha, so cutout is honest, and it
    is what lets the four atlas groups stay batched.
  - `SpriteInstance` stays 48 bytes. The two depth scalars land in
    `FrameUniforms::_pad`, which is already 8 unused bytes.
  - `MMD_UPDATE_GOLDEN=1` is the documented, reviewed golden-regeneration path.
    Every host golden moves in this ticket.
  - `graphify` is installed. Orient with `graphify query`; end with
    `graphify update .`.

## Requirements

- `crates/mmd-engine/src/render/instance.rs` gains three pure functions, each
  independently tested:
  - `iso_project(cx: f32, cy: f32, tile_w: f32, tile_h: f32, origin: [f32; 2]) -> [f32; 2]`
    — `sx = origin[0] + (cx - cy) * tile_w * 0.5`,
    `sy = origin[1] + (cx + cy) * tile_h * 0.5`.
  - `iso_origin(width: u32, height: u32, dest: Cell, tile_w: f32, tile_h: f32, view: [f32; 2]) -> [f32; 2]`
    — the fixed offset that puts the destination cell at the view centre.
  - `iso_depth(ground_y: f32, scale: f32, bias: f32) -> f32` — the normalised
    sort key.
- `FrameUniforms::_pad` is renamed to `depth_scale: f32, depth_bias: f32`.
  Struct size stays 16 bytes; `cbuffer FrameUniforms` in the shader follows.
  `depth_scale = 1.0 / iso_map_height_px`, `depth_bias = -origin[1] / iso_map_height_px`,
  so `ground_y * scale + bias` is the agent's position down the *map* diamond in
  `[0, 1]` and is independent of where the camera sits.
- `shaders/sprite.hlsl`:
  - `VSMain` computes `ground_y = instance_pos.y + instance_size.y` — the quad
    **bottom**, which is the agent's feet, constant across the quad. Emitting a
    per-vertex `world.y` instead would give one sprite a depth gradient and let
    it slice into its neighbours; this is the one detail that must not be got
    wrong.
  - `output.position = float4(ndc, saturate(ground_y * depth_scale + depth_bias), 1.0)`
    for sprite instances, and `0.0` for ring instances (see the ring rule below).
  - `PSMain` alpha-tests: `clip(texel.a - ALPHA_CUTOFF)` with the cutoff a named
    constant, before the existing premultiplied return.
- `crates/mmd-engine/src/render/renderer.rs`:
  - Creates a depth texture at the offscreen resolution, cleared to `1.0` each
    pass.
  - Sprite pipeline: depth test `LESS`, depth write **on**.
  - **Second pipeline**, built from the *same* shader modules, for the ring
    pass: depth test **off**, depth write **off**. This is the second pipeline
    T8 deliberately deferred; rings are a debug overlay and must never be
    occluded by a unit standing in front of them.
  - Draw order per frame: four atlas groups (depth pipeline) → ring instances
    (no-depth pipeline).
- `crates/mmd-engine/src/runtime.rs`:
  - `pack_instance_groups` projects through `iso_project` and anchors the quad
    so its **bottom edge** sits on the projected ground point:
    `px = sx - sprite_size_px * 0.5`, `py = sy - sprite_size_px`.
  - `pack_ring_instances` (from T8) centres its quad **on** the ground point and
    its size becomes `[2r * tile_w_per_cell, 2r * tile_h_per_cell]` — a 2:1
    quad, so T8's shader branch renders an **ellipse**. That is the
    StarCraft-correct read of a circular body lying on an isometric floor, and
    it needs no shader change.
  - Both packers reject a quad whose AABB lies entirely outside the view before
    pushing it. The cull must be a pure function of the instance rect and the
    view rect, and must be tested at all four edges.
- `world_to_clip` gains a `depth: f32` parameter and returns it in `[2]`, so the
  CPU mirror still states what the shader emits and
  `world_to_clip_matches_gpu_raster` keeps binding the two together.
- The doc comment on `validate_collision_scene_dims` in
  `crates/mmd-engine/src/scenario.rs` ("Screen geometry is locked…") is corrected:
  `width`/`height`/`cell_size_px` describe the **cell grid**, not a screen
  mapping, once the projection is isometric.
- The eight sprite directions already in the atlas are reinterpreted as the
  eight isometric facings; `direction_count: 8` and the atlas contract are
  unchanged. If the diagonal facings read wrong on screen, that is an **art**
  fix in a later ticket — do not rotate the direction index in code without
  recording why.
- No change under `crates/mmd-engine/src/sim/`. `git diff --stat` on that
  directory must be empty.

## Inputs

- **From Depends (T8, `d5c0ba9`) — CONCRETE, what actually landed and the traps
  it found (this supersedes the one-line summary below):**
  - The ring ships as a **second pipeline over the same quad**, procedural and
    atlas-free. `SpriteInstance` is unchanged at 48 bytes
    (`instance_layout_is_stable` passes unmodified) — the ring is flagged by a
    **sentinel in `uv_rect.x`**, not by a new field. Do not add one.
  - The renderer gained **additive `*_with_rings` methods** rather than changing
    four existing signatures, so the bench and goldens stayed untouched. Follow
    that pattern rather than rewriting the existing draw entry points.
  - The ring radius comes from **`CollisionParams::radius_cells` (the
    simulation's), not the scenario's**. A test
    (`the_ring_traces_a_body_that_is_not_half_a_sprite`) deliberately breaks the
    half-a-sprite coincidence with a 1.5-cell body on a 30 px sprite — keep it
    green under your projection.
  - **The shader canonical hash is pinned in FIVE manifests, not three.**
    `lab/fixtures/windows-candidate/golden/manifest.json` and
    `lab/fixtures/macos-candidate/golden/manifest.json` are read *live* by the
    mmd-lab merge gate. T8 added
    `every_tracked_manifest_pins_the_live_shader_and_atlas`, which walks the tree
    and enforces all of them on every host — if you touch a shader, that test
    tells you every manifest you owe. Run `cargo test -p mmd-lab --test merge_gate`.
  - **The `.spv` blobs are not bound to `sprite.hlsl`.** `xtask` checks recorded
    hashes only. The repo tracks no GLSL mirror despite building the Linux SPIR-V
    from one; T8 reconstructed it and **proved it reproduces both pinned blobs
    byte-for-byte before editing**. The recipe is in T8's Outputs section of its
    own ticket file — but you may not read that file, so: reconstruct, prove
    byte-for-byte reproduction of the current blobs, and only then edit.
  - **Native blob debt:** DXIL and metallib slots follow the deferred placeholder
    path on this host. The Windows and macOS reference hosts owe a native
    rebuild. Do not invent native blobs.
  - Input injection grammar is `FRAME:KEY` (e.g. `3:h`), not `h@N`.
  - The counting allocator exists only in the `frame_allocations.rs` test binary
    — an allocation test placed anywhere else passes **vacuously**.

- **Inherited from T0 — the pinned gate digest (do not recompute, do not
  re-derive):**
  `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
  produced by `cargo run -- run --agents 5000 --frames 300` on commit `df309d3`.
  Every ticket after T0 must reproduce this value byte for byte. A different
  `hash=` means the plan's core invariant is broken — stop and report `failed`
  rather than re-pinning it. The pre-T0 value
  `f647e7f590ed5814e4e61388e23836dfacb980217fb1762542ec3abfe85549b3` is
  superseded and must never reappear.
  T0 also delivered: `MAX_LIVE_AGENTS = 5_000` enforced by a single
  `check_population` at the validator dispatcher (not per family),
  `COLLISION_SCENE_MAX_AGENTS` removed, the three tracked scenes retuned to
  48 px sprites / `collision_radius_q8: 1_536` with fresh `.sha256` sidecars,
  the four `fixture_*` scenes byte-identical, `MIN_SCANNED_TESTS` at `174`, and
  `BenchPolicy::test_short()` split onto its own `test-short-v1` ladder while
  `production()` keeps the frozen phase-0 ladder verbatim.

- `crates/mmd-engine/src/render/instance.rs` — `FrameUniforms` (~L53),
  `world_to_clip` (~L101), `clip_to_pixel` (~L121), layout tests (~L128).
- `shaders/sprite.hlsl` — `cbuffer FrameUniforms` (~L11), `VSMain` (~L34),
  `PSMain` (~L50), plus T8's ring branch.
- `crates/mmd-engine/src/render/renderer.rs` — pipeline creation (~L135),
  offscreen target creation (~L285, ~L603), the two draw paths (~L354, ~L507).
- `crates/mmd-engine/src/runtime.rs` — `pack_instance_groups` (~L301),
  `pack_ring_instances` (T8), `Runtime::from_scenario` (~L107).
- `crates/mmd-engine/src/scenario.rs` — `validate_collision_scene_dims` (~L490)
  doc comment only.
- `crates/mmd-engine/tests/render_correctness.rs` — `world_to_clip_transform`
  (~L374), `packing_is_a_pure_projection_of_sim_state` (~L323),
  `world_to_clip_matches_gpu_raster` (~L945), `opaque_frame_bounds` (~L1015).
- `lab/goldens/` — every host golden moves.
- **From Depends (T8):** the ring packer and the ring shader branch; the
  unchanged gate digest this ticket must also reproduce.

**Existing signatures this ticket changes, deliberately:**

```rust
// before
pub struct FrameUniforms { pub view_size: [f32; 2], pub _pad: [f32; 2] }
pub fn world_to_clip(pos: [f32; 2], size: [f32; 2], corner: [f32; 2], view_size: [f32; 2]) -> [f32; 4];

// after — same sizes, same stride, honest mirror
pub struct FrameUniforms { pub view_size: [f32; 2], pub depth_scale: f32, pub depth_bias: f32 }
pub fn world_to_clip(pos: [f32; 2], size: [f32; 2], corner: [f32; 2], view_size: [f32; 2], depth: f32) -> [f32; 4];
```

`SpriteInstance` is **not** touched: `instance_layout_is_stable` must pass
unmodified.

## TDD

1. **Red** — the nine tests below. They fail: `iso_project` does not exist, the
   packer is still Cartesian, and `SV_Position.z` is still `0`.
2. **Green** — the three pure functions, the packer change, the uniform change,
   the shader change, the depth attachment, the second pipeline.
3. **Refactor** — one place computes `(tile_w, tile_h, origin, depth_scale,
   depth_bias)` from a `Scenario` + view size, and both packers and the uniform
   upload read it, so the CPU mirror and the GPU can never disagree.

## Test plan

| Test | Where | Asserts |
| ---- | ----- | ------- |
| `iso_projects_a_diamond` | `instance.rs` unit | cell `(0,0)`, `(w,0)`, `(0,h)`, `(w,h)` map to the diamond's top, right, left and bottom vertices, in that order |
| `iso_is_two_to_one` | `instance.rs` unit | a one-cell step in `+x` moves `tile_w/2` right and `tile_h/2` down; `+y` moves the same distance left and down |
| `the_destination_is_centred` | `instance.rs` unit | `iso_origin` puts the destination cell exactly at the view centre for all three tracked scenes |
| `depth_is_the_ground_point_not_the_quad` | `render_correctness.rs` | two agents one cell apart in `+y` get depth keys ordered `nearer > farther`, and the key is identical for all four corners of one quad |
| `depth_is_camera_independent` | `instance.rs` unit | changing `origin` leaves every agent's depth key unchanged |
| `an_agent_in_front_occludes_one_behind` (GPU-only) | `render_correctness.rs` | render two overlapping agents; the pixel where they overlap belongs to the nearer one — the whole point of the ticket |
| `rings_are_never_occluded` (GPU-only) | `render_correctness.rs` | a ring behind a sprite is still visible where they overlap |
| `offscreen_quads_are_culled` | `render_correctness.rs` | an agent placed past each of the four view edges produces no instance; one straddling an edge still does |
| `world_to_clip_matches_gpu_raster` (existing, extended) | `render_correctness.rs` | the mirror predicts `SV_Position` including `z` |
| `packing_is_a_pure_projection_of_sim_state` (existing) | `render_correctness.rs` | still passes — packing reads sim state and nothing else |
| `iso_packing_allocates_nothing` | `render_correctness.rs` | `MeasureGuard` around a warm pack → 0 allocations, cull included |

## Impl steps

- [ ] 1. `graphify query "sprite renderer pipeline offscreen target depth"` and
      `graphify path "pack_instance_groups" "SpriteRenderer"`.
- [ ] 2. Write the pure-function tests and the packer tests. Run — red.
- [ ] 3. Land `iso_project`, `iso_origin`, `iso_depth` with their tests green before
      touching the renderer.
- [ ] 4. Change `FrameUniforms` and `world_to_clip`; fix call sites.
- [ ] 5. Change `shaders/sprite.hlsl`; regenerate SPIR-V and the manifest; run
      `cargo run -p xtask -- shaders --check`. Record the DXIL/metallib
      placeholder debt for the Windows and macOS reference hosts in Outputs.
- [ ] 6. Add the depth texture, the depth-state on the sprite pipeline, and the
      second no-depth pipeline for rings.
- [ ] 7. Change both packers; add the cull.
- [ ] 8. Regenerate host goldens (`MMD_UPDATE_GOLDEN=1 …`) and **look at them** before
      committing. A golden that regenerates to a plausible-but-wrong image is the
      main risk in this ticket.
- [ ] 9. Correct the `validate_collision_scene_dims` doc comment.
- [ ] 10. `graphify update .`.

## Outputs

- A 2:1 isometric render with correct front-to-back occlusion, batching intact.
- Rings become floor ellipses, unoccluded.
- A fixed view centred on the destination, with everything outside it culled.
- Regenerated goldens (list them) and the placeholder debt for the two
  reference hosts.
- The gate digest **unchanged** from T0 — record the observed `hash=` here.
- `git diff --stat crates/mmd-engine/src/sim/` → empty, quoted in the commit
  message as the proof that the simulation did not move.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --locked`
- [ ] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` on the GPU host —
      including `an_agent_in_front_occludes_one_behind` and
      `rings_are_never_occluded`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `nix flake check`
- [ ] `cargo run -p xtask -- bootstrap --check`
- [ ] `cargo run -p xtask -- shaders --check`
- [ ] `cargo run -p xtask -- atlases --check`
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0, `hash=` equals
      T0's pinned digest
- [ ] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exits 0
- [ ] `cargo tree -e features | grep -c testkit` → 0
- [ ] `git diff --stat crates/mmd-engine/src/sim/` → empty
- [ ] `graphify update .` run
- [ ] manual check: `cargo run -- run --agents 5000` — the horde reads as an
      isometric crowd, units in front cover units behind, and pressing `H`
      shows a floor ellipse under each one. Esc to quit.
