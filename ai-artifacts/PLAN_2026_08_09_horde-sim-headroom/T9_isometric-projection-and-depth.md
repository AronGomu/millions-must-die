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

- [x] 1. `graphify query "sprite renderer pipeline offscreen target depth"` and
      `graphify path "pack_instance_groups" "SpriteRenderer"`.
      (Both run; the query returned the 258-node `SpriteRenderer`/`renderer.rs`
      subgraph, the path reported `No directed path found` — the packer is a
      free function the renderer never calls, which is why the projection is
      wired through `Runtime`, not through `SpriteRenderer`.)
- [x] 2. Write the pure-function tests and the packer tests. Run — red.
      (`cargo test -p mmd-engine --lib render::instance` → `error[E0560]: struct
      instance::FrameUniforms has no field named _pad`; the six new cases could
      not even link because `iso_project`/`iso_origin`/`iso_depth`/`IsoView`
      did not exist.)
- [x] 3. Land `iso_project`, `iso_origin`, `iso_depth` with their tests green before
      touching the renderer.
      (`cargo test -p mmd-engine --lib` → `34 passed; 0 failed`, including
      `iso_projects_a_diamond`, `iso_is_two_to_one`, `the_destination_is_centred`,
      `depth_is_camera_independent`, `quad_visibility_is_a_rect_test`,
      `the_tracked_scene_list_is_complete`.)
- [x] 4. Change `FrameUniforms` and `world_to_clip`; fix call sites.
      (`FrameUniforms { view_size, depth_scale, depth_bias }` still 16 bytes —
      `instance_layout_is_stable` green, unmodified. `world_to_clip` takes
      `depth` and returns it in `[2]`.)
- [x] 5. Change `shaders/sprite.hlsl`; regenerate SPIR-V and the manifest; run
      `cargo run -p xtask -- shaders --check`. Record the DXIL/metallib
      placeholder debt for the Windows and macOS reference hosts in Outputs.
      (`shaders: ok (spirv+dxil+metallib; native DXIL/metallib regen still
      host-gated T9/T10)`; canonical hash re-pinned in **all six** manifests and
      `cargo test -p mmd-lab --test merge_gate` → `5 passed`.)
- [x] 6. Add the depth texture, the depth-state on the sprite pipeline, and the
      second no-depth pipeline for rings.
      (`MMD_REQUIRE_GPU=1` → `an_agent_in_front_occludes_one_behind ... ok`,
      `rings_are_never_occluded ... ok`.)
- [x] 7. Change both packers; add the cull.
      (`offscreen_quads_are_culled ... ok` at all four edges plus four
      straddles; `iso_packing_allocates_nothing ... ok`.)
- [x] 8. Regenerate host goldens (`MMD_UPDATE_GOLDEN=1 …`) and **look at them** before
      committing. A golden that regenerates to a plausible-but-wrong image is the
      main risk in this ticket.
      (Diffed old vs new pixel by pixel first: exactly 17 pixels moved, every one
      of them an alpha-1..65 fringe texel going to fully transparent, no geometry
      displaced. Then rendered the crop and looked at it — four intact sprites,
      same positions, clean cutout edges. See Outputs.)
- [x] 9. Correct the `validate_collision_scene_dims` doc comment.
      (`crates/mmd-engine/src/scenario.rs` — now states that
      `width`/`height`/`cell_size_px` describe the cell grid, and that where the
      grid lands on screen is `IsoView`'s business.)
- [x] 10. `graphify update .`.

## Outputs

- A 2:1 isometric render with correct front-to-back occlusion, batching intact.
  Four atlas groups on the depth pipeline, then the rings on the no-depth one;
  no per-frame sort, no fifth atlas.
- Rings become floor ellipses, unoccluded.
- A fixed view centred on the destination, with everything outside it culled.
  Gate scene: `tile=8x4 origin=[540, -212] depth_scale=0.00066666666
  depth_bias=0.14133333`, frame 0 packs `groups=[195, 195, 195, 195]` of 5 000
  agents.

**What the production review changed.** Seven read-only reviewers ran over the
finished diff (correctness, perf, test-quality, contracts translated to the
CPU↔GPU + shader-blob chain, data-integrity translated to tracked artifacts and
pinned digests, plus general reviewers at GPU-resource-lifetime and at
scope-drift). Two findings were serious and they interact:

- **The ticket's own "must not get wrong" invariant was unasserted.**
  `an_agent_in_front_occludes_one_behind` originally drew the nearer sprite
  first. Under the defect it exists to catch — a vertex stage emitting the
  interpolated `world.y` instead of the quad's bottom edge — both quads emit an
  *identical* `z` at every shared pixel, the farther one still fails the test,
  and the case passes with the bug in. It now runs **both** draw orders; only
  the reversed one can fail. Proven by mutation: injecting `ground_y = world.y`
  into the mirror and rebuilding made it fail on `farther first, nearer second`
  (1 563 of 4 654 pixels) while the original order stayed green.
  The assertion is scoped to fully opaque pixels (`alpha == 255`), because
  blending is still enabled and a partial-alpha fringe legitimately composites
  differently depending on what is behind it.
- **A depth key of exactly `0.0` was discarded rather than sorted last.** Strict
  `GREATER` against a buffer cleared to `0` kills `0 > 0`. Reachable for an
  agent on the far corner of the map diamond, and systemically for a degenerate
  `IsoView`, which zeroed both scalars and would have blanked the entire frame.
  Fixed by flooring the **sprite** key at one `D16` quantum
  (`MMD_DEPTH_EPSILON` / `ISO_DEPTH_EPSILON` = 1/65 536). Deliberately *not*
  fixed with `GreaterOrEqual`, which two reviewers suggested: that lets the
  per-vertex tie through and would re-blind the case above. Rings keep an exact
  `0` so they still fail the sprite pipeline's test, which is what keeps
  `rings_are_never_occluded` falsifiable.

Also fixed: five tests that the cull had quietly weakened were restored to exact
expectations (`partitions_four_groups` per-bucket counts,
`toggling_hitboxes_changes_only_the_rings` ring count,
`the_ring_traces_the_real_body` counted multiset rather than set membership),
two tautologies removed (`world_to_clip(..)[2] == depth`, `tw / th == 2.0`),
three comments that still asserted a `LESS` test corrected, `IsoView::map_height_px`
made a stored field rather than a lossy `1/depth_scale` round-trip, a new
`frame_uniforms_layout_is_stable` pinning the uniform's *offsets* (separate from
`instance_layout_is_stable`, which stays untouched), and
`iso_origin_frames_the_destination` added so all three pure functions are
independently tested — the clamp branch previously had no coverage at all.

**And the SPIR-V is now reproducible.** Two reviewers rated this HIGH: nothing
in the repo could rebuild the `.spv`, and the only GLSL on disk was an untracked,
locally-excluded, *pre-T8* file that provably could not have produced either
blob. The mirror is now tracked at `shaders/glsl/`, and
`shaders/generated/README.md` records the toolchain, the exact command, the six
manifests to re-pin, and the gap that remains.

**Deviations from the ticket as written, and why:**

- **The depth test is `GREATER` against a buffer cleared to `0`, not `LESS`
  against `1`.** The ticket's three statements about depth cannot all hold at
  once: the key is `saturate(ground_y * depth_scale + depth_bias)` (larger =
  further down the map diamond = *nearer* the camera), the test plan requires
  `nearer > farther`, and `an_agent_in_front_occludes_one_behind` requires the
  nearer agent to win. Under `LESS` + clear `1.0` the nearer agent loses, and
  the horde renders back to front — observed directly: 4 482 of 13 300 pixels
  the nearer sprite had painted were overwritten by the farther one. Reversing
  the comparison is the one-word change that keeps the shader formula, both
  depth-key cases and the occlusion requirement all verbatim; inverting the key
  instead would have broken two of them. It also makes
  `rings_are_never_occluded` falsifiable for free: a ring emits `z = 0`, which
  fails `GREATER` against the cleared buffer everywhere, so a ring drawn on the
  sprite pipeline would not appear at all.
- **`iso_packing_allocates_nothing` lives in `frame_allocations.rs`,** not
  `render_correctness.rs`. The counting allocator is installed only in that test
  binary, so a `MeasureGuard` anywhere else records nothing and the assertion
  passes vacuously — flagged in this ticket's own Inputs.
- **`iso_origin`'s `width`/`height` clamp the destination into the grid.** They
  are otherwise unused by "put `dest` at the view centre". Scenario validation
  already rejects an out-of-bounds destination, so the clamp never fires on a
  tracked scene; it stops an in-memory grid from aiming the camera at a cell
  that does not exist.
- **`DEFAULT_DEPTH_SCALE = 1/VIEW_HEIGHT`, `DEFAULT_DEPTH_BIAS = 0` on
  `SpriteRenderer`.** Not named by the ticket, but content authored directly in
  view pixels — the static golden scene, every GPU probe — has no map diamond to
  normalise against, and "further down the screen is nearer" is the only honest
  reading for it. A `Runtime` overrides both from its `IsoView`.
- **The ring ellipse is `[2r·tile_w, 2r·tile_h]` exactly as specified, which is
  √2 larger than the geometrically exact projection.** A circle of radius `r`
  cells projects to an ellipse with semi-axes `r·tile_w/√2` by `r·tile_h/√2`
  (the screen-x direction in cell space is `(1,-1)/√2`, so a diameter along it
  spans `2r·tile_w/√2`). The specified quad is therefore a factor √2 wide and
  tall. It reads plausibly on screen and the overlay is debug-only, so it ships
  as written rather than being redesigned here — but a follow-up wanting the
  ring to trace the *exact* contact circle should divide
  `runtime::ring_quad_size_px` by `√2`.

**Artifacts that moved:**

- `shaders/sprite.hlsl` → canonical
  `7dfbeaf8753c5a1490581547b6ac0123b0e41366dab87e0df19684fe0eee660d`,
  re-pinned in all **six** manifests: `shaders/generated/manifest.json`,
  `lab/goldens/{linux-vulkan,windows-d3d12,macos-metal}/manifest.json`,
  `lab/fixtures/{windows,macos}-candidate/golden/manifest.json`.
- `shaders/generated/sprite.vert.spv` →
  `a49479c9cb5dfa7b22e369305f3654cce7502aaff90cb1e5d78cac7f22d46cea`,
  `sprite.frag.spv` →
  `3018045553e22a90720ed94ec099cd963d1ef45c094de55f897d8ec059f5bc84`.
  Built from the GLSL mirror, which this ticket now **tracks** at
  `shaders/glsl/sprite.{vert,frag}.glsl`. The mirror was reconstructed from
  `spirv-dis` of the previously-pinned blobs and **proved to reproduce both of
  them byte for byte before any edit** (`glslc` from `nixpkgs#shaderc` 2026.1 /
  glslang 16.2.0, `-fshader-stage=vertex|fragment`, no other flags), so the
  lineage from the pre-T9 artifacts is verifiable rather than asserted.
  Before this the mirror lived only in an untracked scratch directory — and the
  one GLSL file actually on disk was a *pre-T8* copy that provably could not
  have produced either blob, which made the blobs unreproducible and the file a
  trap. `shaders/generated/README.md` now carries the recipe and the residual
  gap: `xtask -- shaders --check` re-verifies recorded hashes, it does not
  recompile, so nothing mechanically proves a blob still matches the HLSL.
- `lab/goldens/linux-vulkan/golden.png` → 17 pixels of 2 073 600 changed, all
  of them alpha-1..65 fringe texels now discarded by the `clip()` cutout
  (`(108,105) [3,3,3,6]→[0,0,0,0]`, `(308,105) [6,6,5,65]→[0,0,0,0]`, …). No
  sprite moved; the four demo sprites were rendered, cropped and inspected.
- `lab/fixtures/ubuntu-candidate/readback-pass.png` — the merge gate's fake
  Ubuntu lane readback, which was byte-identical to the pre-T9 golden
  (`ad843c49…`) and must track it, or the ubuntu lane blocks on the same 17
  pixels. Now `90202a86…`, identical to the new golden.
- **Native blob debt, unchanged and still owed:** DXIL and metallib stay
  deferred placeholders (`deferred: T9` / `deferred: T10`). This host has no
  DXC and no Metal toolchain, and inventing native blobs would be worse than
  the placeholder. The Windows and macOS reference hosts owe a native rebuild of
  `sprite.vert.dxil` / `sprite.frag.dxil` / `sprite.metallib` from the new
  canonical HLSL before their backends can run this shader at all.

**Surfaced, deliberately NOT changed here — for the plan owner:**

- **The frozen phase-0 bench ladder is no longer comparable at any rung.** The
  measured frame changed in four ways at once: the cull drops ~20 % of instances
  (~48 KB less upload, ~2.3 M fewer fragments at the 5 000 rung), the depth
  attachment is cleared and written, `clip()` kills ~46 % of surviving
  fragments, and the quad anchor moved. The runner already protects
  comparability for the *ring* overlay (`set_hitboxes_visible(false)`), but the
  cull and depth are unconditional and bypass that. Post-T9 samples must not be
  compared to the recorded pre-T9 evidence without re-baselining as a new epoch.
- **The ring overlay's fill rate exactly doubled** (48 × 48 → 96 × 48 px per
  ring), netting ~1.6× after the cull — and the bench cannot see it, because the
  bench runs with rings off. Inherent to the 2:1 ellipse the ticket specifies.
- **Blend is still enabled on a pipeline that is now an alpha-tested depth
  writer.** `clip()` costs early-depth-*write* on every mainstream driver;
  dropping blend would save ~25 MB/frame of colour read traffic but would harden
  the sprites' anti-aliased edges. A visual decision, out of scope here. The
  same interaction leaves a faint dark halo on the 0.5–1.0 alpha fringe.
- **`set_depth_params` is sticky renderer state** with no binding to the groups
  being drawn. A caller that packed with one projection and drew with another
  would get a *blank* frame, not a visibly wrong one. Passing `&IsoView` into
  the draw entry points is the fix, and is an API-shape decision for Phase 1.
- **`canonical_sha256` is a single top-level manifest field**, so it cannot say
  "the SPIR-V was built from X but the DXIL from something older". It matters
  when the Windows and macOS reference hosts drop in native blobs.
- **The merge gate's pixel limb is tautological**: `lab/fixtures/ubuntu-candidate/readback-pass.png`
  is a byte-copy of the golden it is compared against, so that comparison cannot
  fail. Pre-existing, not introduced here — and the *binding* limb is live and
  did real work (an un-re-pinned manifest would have failed it). A
  `readback-fail.png` fixture would give the pixel limb a proven failure mode.
- **`lab/fixtures/{windows,macos}-candidate/golden/manifest.json` say
  `status: "captured"`** for synthetic 8 × 8 images no shader ever rendered. A
  distinct `fixture-synthetic` status would make "which hosts are actually
  verified" answerable from the status field.

**Invariants, observed:**

- Gate digest **unchanged** from T0: `cargo run --release -- run --agents 5000
  --frames 300` → `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`.
- `git diff --stat crates/mmd-engine/src/sim/` → empty.
- `instance_layout_is_stable` green, unmodified; `SpriteInstance` still 48 bytes.

## Validation

- [x] `cargo fmt --all -- --check` → clean (no diff).
- [x] `cargo test --workspace --locked` → **519 passed, 0 failed**.
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` on the GPU host —
      including `an_agent_in_front_occludes_one_behind` and
      `rings_are_never_occluded`
      → **519 passed, 0 failed**, and `grep -c '^SKIP '` on that run is `0`, so
      every GPU case really executed rather than skipping;
      `an_agent_in_front_occludes_one_behind ... ok`,
      `rings_are_never_occluded ... ok`, `world_to_clip_matches_gpu_raster ... ok`,
      `golden_frame_matches ... ok`, plus the two cases the review added,
      `frame_uniforms_layout_is_stable ... ok`,
      `iso_origin_frames_the_destination ... ok` and
      `shader_defines_match_their_rust_mirrors ... ok`.
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
      → exit 0, no output.
- [x] `nix flake check` → `all checks passed!`
- [x] `cargo run -p xtask -- bootstrap --check`
      → `bootstrap: ok (SDL 3.4.12 / sdl3 0.18.4 / sdl3-sys 0.6.7; win/mac native host-gated T9/T10)`
- [x] `cargo run -p xtask -- shaders --check`
      → `shaders: ok (spirv+dxil+metallib; native DXIL/metallib regen still host-gated T9/T10)`
- [x] `cargo run -p xtask -- atlases --check` → `atlases: ok (4 png + manifest)`
- [x] `cargo run -- run --agents 5000 --frames 300` — exits 0, `hash=` equals
      T0's pinned digest
      → `run: clean exit mode=window backend=vulkan tick=300 frames=300
      hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exits 0
      → `tick=300 frames=300 hash=3df604770021490eb416b38bcd9bc4a25bb417638010c458d5563a17578d268a`, exit 0.
- [x] `cargo test -p mmd-lab --test merge_gate` → `5 passed, 0 failed`.
      **What that does and does not prove.** Its *binding* limb is live and did
      real work here: an un-re-pinned manifest fails it, which is how the six
      manifests were caught the first time. Its *pixel* limb cannot fail —
      `lab/fixtures/ubuntu-candidate/readback-pass.png` is a byte-for-byte copy
      of the golden it is compared against (both `90202a86…dde1`), so that
      comparison is a file against itself. Pre-existing, not introduced here,
      but T9 is the first change to materially move rasterization, so it is
      worth stating plainly: the pixel claim for this ticket rests on the GPU
      probes and the reviewed golden, not on the merge gate.
- [x] `cargo tree -e features | grep -c testkit` → `0`
- [x] `git diff --stat crates/mmd-engine/src/sim/` → empty
- [x] `graphify update .` run
- [ ] manual check: `cargo run -- run --agents 5000` — the horde reads as an
      **isometric** crowd on a 2:1 floor, units lower on screen cover the units
      behind them, and pressing `H` shows a floor **ellipse** (twice as wide as
      tall) under each one. Esc to quit.
      *Windowed; not run headless and not gating this ticket.* See
      `ai-artifacts/manual_test_checklist.md` § `T9 isometric-projection-and-depth`.
      The automatable part was substituted: a real `Runtime` frame (5 000 agents,
      900 ticks, `collision_mid_v1`) was rendered offscreen to PNG and inspected —
      the crowd's leading edge is a clean 2:1 diagonal, nearer units cover the
      rank behind with no slicing, and the ring pass draws cyan ellipses over
      everything.
