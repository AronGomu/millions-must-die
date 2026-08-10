# T2: Renderer texture table + ScenePass

**Plan:** `./ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** T1
**Commit outcome:** the renderer binds any of 9 texture slots per draw group and draws a world / overlay / UI scene in one pass, with the phase-0 frame byte-identical.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a new horde-free
  scene. Phase-0 contracts stay frozen.
- This slice: the renderer today hard-codes exactly four atlas groups indexed
  `0..3`. Phase 1 needs to draw workers, soldiers, buildings, props and text.
  This ticket generalises the texture binding to a flat table and introduces the
  three-layer scene pass, and does **nothing else**.
- Out of scope here: the shader (`shaders/sprite.hlsl` is unchanged — do not
  touch it, `xtask shaders --check` must stay green), `SpriteInstance`'s 48-byte
  layout, the camera, the scenario contract, anything under `src/` or `crates/mmd-engine/src/sim/`.
- Assumptions in force: the phase-0 render golden
  (`lab/goldens/linux-vulkan/`) must compare **exactly** after this ticket.
  That is only true if, with an empty UI layer, the emitted draw order is
  identical to today's: all world groups in ascending slot order on the depth
  pipeline, then the overlay range on the depth-off pipeline.

## Requirements

- A flat texture table of 9 slots, with named constants.
- `DrawGroup.atlas_id` becomes a *slot* index into that table, valid in
  `0..ATLAS_SLOT_COUNT`.
- New `ScenePass<'a>` struct and two draw entry points taking it.
- The existing `draw_offscreen_with_rings` / `draw_to_swapchain_with_rings` /
  `draw_offscreen_readback_with_rings` / `draw_offscreen_acquire_fence`
  signatures and behaviour are preserved, reimplemented on top of `ScenePass`.
- No per-frame allocation added: the per-group range buffer is a preallocated
  field on `SpriteRenderer`.

## Inputs

- **Files to read**
  - `crates/mmd-engine/src/render/renderer.rs` — `SpriteRenderer`, `DrawGroup`,
    `draw_offscreen_into`, `validate_groups`, `MAX_INSTANCES`, `FRAMES_IN_FLIGHT`.
  - `crates/mmd-engine/src/render/atlas.rs` — `ATLAS_COUNT`, `load_atlases`,
    `frame_uv_rect`, `default_atlas_dir`.
  - `crates/mmd-engine/src/render/mod.rs` — the `pub use` list to extend.
  - `crates/mmd-engine/tests/render_correctness.rs`, `gpu_smoke.rs`, `gpu_golden.rs`.
- **From Depends (T1) — spell out, the worker cannot read T1:**
  - T1 created two new tracked asset families, each with a `manifest.json`
    written by `xtask`:
    - `assets/sprites/generated/rts/` containing `worker.png`, `soldier.png`,
      `buildings.png`, `props.png` — **each 128 × 256 px**, laid out as
      4 columns × 8 rows of 32 × 32 px frames, i.e. the same geometry the
      zombie atlases use, so `render::frame_uv_rect(dir, frame)` addresses them
      unchanged.
    - `assets/sprites/generated/ui/` containing `font.png` — **128 × 48 px**,
      16 columns × 6 rows of 8 × 8 px glyphs for ASCII 32..=127.
  - Both manifests are JSON of this shape (field names exact):
    ```json
    { "version": 1, "generator": "mmd-rts-placeholder-v1",
      "frame_width_px": 32, "frame_height_px": 32, "cols": 4, "rows": 8,
      "images": [ { "id": 0, "file": "worker.png", "sha256": "…" }, … ] }
    ```
    The `ui` manifest has `generator: "mmd-ui-font-v1"`, `frame_width_px: 8`,
    `frame_height_px: 8`, `cols: 16`, `rows: 6`, and a single image entry
    `{ "id": 0, "file": "font.png", "sha256": "…" }`.
  - Every texel in all five PNGs is premultiplied RGBA8.
  - `cargo run -p xtask -- atlases --check` already verifies both families.

## Exact design — no decisions left

### New constants in `crates/mmd-engine/src/render/atlas.rs`

```rust
/// Texture slots 0..=3: the phase-0 zombie skins. Unchanged.
/// Slot of the RTS worker sheet.
pub const SLOT_RTS_WORKER: u32 = 4;
/// Slot of the RTS soldier sheet.
pub const SLOT_RTS_SOLDIER: u32 = 5;
/// Slot of the RTS building/resource-node table sheet.
pub const SLOT_RTS_BUILDINGS: u32 = 6;
/// Slot of the RTS prop sheet (selection ring, placement tiles, icons, panel).
pub const SLOT_RTS_PROPS: u32 = 7;
/// Slot of the UI bitmap font.
pub const SLOT_UI_FONT: u32 = 8;
/// Total bindable texture slots. `DrawGroup::atlas_id` must be below this.
pub const ATLAS_SLOT_COUNT: usize = 9;

/// The four RTS sheets, in slot order starting at [`SLOT_RTS_WORKER`].
pub const RTS_FILES: [&str; 4] = ["worker.png", "soldier.png", "buildings.png", "props.png"];
/// Directory of the RTS placeholder family, relative to the workspace root.
pub fn rts_atlas_dir(workspace_root: &Path) -> PathBuf;
/// Directory of the UI family, relative to the workspace root.
pub fn ui_atlas_dir(workspace_root: &Path) -> PathBuf;

/// Load the four RTS sheets, hash-verified against `rts/manifest.json`.
pub fn load_rts_atlases(dir: &Path) -> Result<[AtlasRgba; 4], RenderError>;
/// Load the UI font sheet, hash-verified against `ui/manifest.json`.
pub fn load_ui_font(dir: &Path) -> Result<AtlasRgba, RenderError>;
```

`load_rts_atlases` / `load_ui_font` mirror `load_atlases`: read `manifest.json`,
reject a wrong `version`/`cols`/`rows`/`frame_*_px`/image count, then for each
entry read the PNG, require `sha256_hex(bytes) == entry.sha256`, decode to RGBA8.
A hash mismatch is `RenderError::Atlas(..)`, exactly as the zombie loader does.

### `SpriteRenderer` changes in `crates/mmd-engine/src/render/renderer.rs`

Replace `atlas_tex: [Texture<'static>; ATLAS_COUNT]` with, in addition:

```rust
/// Texture slots 4..=8: RTS sheets then the UI font. Kept separate from
/// `atlas_tex` so the phase-0 four stay exactly where they were.
extra_tex: [Texture<'static>; ATLAS_SLOT_COUNT - ATLAS_COUNT],
/// CPU copies of the five phase-1 sheets, for headless assertions.
extra_cpu: [AtlasRgba; ATLAS_SLOT_COUNT - ATLAS_COUNT],
/// Per-group `(start, count)` ranges, reserved at `ATLAS_SLOT_COUNT` so a
/// frame never grows it. Cleared, not reallocated, per draw.
group_ranges: Vec<(u32, u32)>,
```

Add:

```rust
impl SpriteRenderer {
    /// The texture bound for `slot`. Slots 0..=3 are the zombie atlases,
    /// 4..=8 the phase-1 sheets. Out of range is a caller bug and panics —
    /// `validate_scene` rejects it before any GPU work is issued.
    fn texture_at(&self, slot: u32) -> &Texture<'static>;

    /// CPU pixels for `slot`, for headless tests.
    pub fn atlas_pixels(&self, slot: u32) -> Option<&AtlasRgba>;
}
```

`SpriteRenderer::with_context(ctx, atlas_dir)` gains the two sibling
directories: it resolves them as `atlas_dir.join("rts")` and
`atlas_dir.join("ui")`, so `new(workspace_root, debug)` keeps working and a test
pointing `with_context` at a temp dir gets the temp family too.

### The scene pass

New public type in `renderer.rs`:

```rust
/// One frame's three layers, in draw order.
///
/// `world` is depth-tested. `overlay` and `ui` are not: they are drawn on the
/// same pipeline the hitbox rings already use, whose depth test *and* depth
/// write are both off. `ui` is drawn last so a panel is never occluded by a
/// world annotation.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScenePass<'a> {
    /// Depth-tested world sprites. `atlas_id` is a texture slot.
    pub world: &'a [DrawGroup],
    /// World-space annotations: hitbox rings, selection rings, placement tiles.
    /// One flat range, drawn with slot 0 bound (the ring branch samples no
    /// texture; a textured overlay instance must go in `ui` instead).
    pub overlay: &'a [SpriteInstance],
    /// Screen-space UI, grouped by texture slot.
    pub ui: &'a [DrawGroup],
}

impl<'a> ScenePass<'a> {
    /// The phase-0 shape: world groups plus a ring overlay, no UI.
    pub fn world_and_rings(world: &'a [DrawGroup], rings: &'a [SpriteInstance]) -> Self {
        Self { world, overlay: rings, ui: &[] }
    }
}
```

New entry points, replacing the private `draw_offscreen_into`'s parameter list:

```rust
pub fn draw_offscreen_scene(&mut self, scene: ScenePass<'_>) -> Result<(), RenderError>;
pub fn draw_to_swapchain_scene(&mut self, window: &Window, scene: ScenePass<'_>) -> Result<(), RenderError>;
pub fn draw_offscreen_readback_scene(&mut self, scene: ScenePass<'_>) -> Result<Readback, RenderError>;
fn draw_scene_into(&mut self, scene: ScenePass<'_>) -> Result<unsafe_sys::RawFrameFence, RenderError>;
```

The four existing public methods become one-line wrappers:

```rust
pub fn draw_offscreen_with_rings(&mut self, groups: &[DrawGroup], rings: &[SpriteInstance])
    -> Result<(), RenderError>
{ self.draw_offscreen_scene(ScenePass::world_and_rings(groups, rings)) }
```
and likewise for `draw_to_swapchain_with_rings`,
`draw_offscreen_readback_with_rings` and `draw_offscreen_acquire_fence`
(the latter calls `draw_scene_into(ScenePass::world_and_rings(groups, &[]))`).

### `draw_scene_into` body — exact order

1. `self.validate_scene(&scene)?` (below).
2. `let total = world instances + overlay.len() + ui instances;`
   `if total > MAX_INSTANCES as usize { return Err(RenderError::Sdl(format!("too many instances {total}"))); }`
   — **before** any byte is packed, exactly as today.
3. Acquire slot, `self.frame_slot = self.frame_slot.wrapping_add(1)`.
4. `self.pack_scratch.clear(); self.group_ranges.clear();`
   Push each `world` group's `(start, count)` into `group_ranges` in the order
   given, appending its instances to `pack_scratch`.
   Then the overlay range, then each `ui` group's range — recorded in two local
   `Vec`-free variables: `overlay_range: (u32, u32)` and
   `ui_range_start: usize` (the index in `group_ranges` where UI ranges begin).
   Record `world_range_len = scene.world.len()`.
5. Upload pass — byte-identical to today.
6. Render pass, `push_vertex_uniform_data(0, &uniforms)` once, colour + depth
   targets exactly as today.
7. Bind `self.pipeline`. Bind quad VB slot 0, index buffer. For each world range
   with `count > 0`: bind `texture_at(group.atlas_id)`, rebind instance buffer
   at `start * SpriteInstance::STRIDE`, `draw_indexed_primitives(6, count, 0, 0, 0)`.
8. If `overlay_range.1 > 0`: bind `self.ring_pipeline`, re-issue quad VB / index
   buffer / sampler binding with `texture_at(0)` (defensive, exactly as today),
   bind instance buffer at the overlay offset, draw.
9. For each UI range with `count > 0`: `self.ring_pipeline` is already bound if
   step 8 ran — bind it unconditionally at the top of this loop's first
   iteration instead, so a frame with an empty overlay still gets the depth-off
   pipeline. Bind `texture_at(group.atlas_id)`, rebind instance buffer, draw.
10. `device.end_render_pass(pass)`, `unsafe_sys::submit_acquire_raw_fence(device, cmd)`.

**Why the golden survives:** with `ui` empty, steps 7 and 8 emit exactly the same
sequence of binds and draws as today's `draw_offscreen_into`, and step 9 emits
nothing. The frame uniform, clear colour, depth clear, blend state, pipelines and
shader modules are untouched.

### Validation

```rust
fn validate_scene(&self, scene: &ScenePass<'_>) -> Result<(), RenderError> {
    for g in scene.world.iter().chain(scene.ui.iter()) {
        if g.atlas_id as usize >= ATLAS_SLOT_COUNT {
            return Err(RenderError::Atlas(format!(
                "draw group atlas_id {} >= ATLAS_SLOT_COUNT {ATLAS_SLOT_COUNT}",
                g.atlas_id
            )));
        }
    }
    Ok(())
}
```

The **old** `validate_groups` — `groups.len() == ATLAS_COUNT` and
`atlas_id == index` — is kept and still applied, but only from
`ScenePass::world_and_rings` callers. Implement that by having the four legacy
wrappers call `self.validate_groups(groups)?` before delegating. This is what
keeps `RenderError::GroupCount` reachable and its existing test green.

### Module exports

`crates/mmd-engine/src/render/mod.rs` — extend the `pub use` lists with
`ATLAS_SLOT_COUNT`, `RTS_FILES`, `SLOT_RTS_WORKER`, `SLOT_RTS_SOLDIER`,
`SLOT_RTS_BUILDINGS`, `SLOT_RTS_PROPS`, `SLOT_UI_FONT`, `load_rts_atlases`,
`load_ui_font`, `rts_atlas_dir`, `ui_atlas_dir` from `atlas`, and `ScenePass`
from `renderer`.

## TDD

1. **Red** — add every test below to
   `crates/mmd-engine/tests/render_correctness.rs` (headless ones) and
   `crates/mmd-engine/tests/gpu_smoke.rs` (device ones, using the existing
   auto-skip helper in `crates/mmd-engine/tests/common/mod.rs`). Watch them fail.
2. **Green** — implement.
3. **Refactor** — collapse the four legacy wrappers to one line each. Keep green.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `slot_constants_are_dense_and_ordered` | the constants | `SLOT_RTS_WORKER == ATLAS_COUNT as u32` and each following slot is `+1`, `SLOT_UI_FONT + 1 == ATLAS_SLOT_COUNT as u32` |
| `rts_manifest_matches_generated_pngs` | `render::rts_atlas_dir(workspace_root())` | `load_rts_atlases` returns 4 images, each `128 × 256` |
| `ui_font_manifest_matches_generated_png` | `render::ui_atlas_dir(workspace_root())` | `load_ui_font` returns one image, `128 × 48` |
| `rts_atlas_hash_tamper_is_rejected` | temp dir copy with one PNG byte flipped | `Err(RenderError::Atlas(_))` |
| `ui_font_hash_tamper_is_rejected` | temp dir copy with `font.png` byte flipped | `Err(RenderError::Atlas(_))` |
| `scene_pass_default_is_empty` | `ScenePass::default()` | all three layers empty |
| `world_and_rings_leaves_ui_empty` | `ScenePass::world_and_rings(&g, &r)` | `ui.is_empty()` |
| `an_out_of_range_slot_is_rejected` (GPU) | `ScenePass { world: &[DrawGroup { atlas_id: 9, .. }], .. }` | `Err(RenderError::Atlas(_))`, and no panic |
| `legacy_four_group_contract_still_rejects_a_wrong_count` (GPU) | `draw_offscreen_with_rings(&groups[..3], &[])` | `Err(RenderError::GroupCount { got: 3, expected: 4 })` |
| `legacy_four_group_contract_still_rejects_a_shuffled_id` (GPU) | four groups with `atlas_id` `[1,0,2,3]` | `Err(RenderError::Atlas(_))` |
| `instance_budget_is_checked_before_packing` (GPU) | one group of `MAX_INSTANCES + 1` instances | `Err(RenderError::Sdl(_))` containing `too many instances` |
| `a_phase1_slot_renders` (GPU) | `ScenePass { world: &[DrawGroup { atlas_id: SLOT_RTS_WORKER, instances: vec![one opaque quad at (100,100) size 32] }], .. }` then readback | at least one pixel in `100..132 × 100..132` has `a > 0` |
| `ui_layer_draws_over_the_world` (GPU) | a world quad at `(200,200)` size 64 on slot 0, and a UI quad at the same rect on `SLOT_RTS_PROPS` using the opaque panel-fill cell | readback pixel `(232, 232)` equals the panel colour, not the world sprite's |
| `ui_layer_is_not_depth_tested` (GPU) | UI quad as above, world quad drawn *after* in slot order with a nearer depth | the UI pixel still wins |
| `empty_ui_layer_reproduces_the_phase0_frame` (GPU) | `SpriteRenderer::static_demo_groups()` drawn via `draw_offscreen_readback_with_rings(&g, &[])` **and** via `draw_offscreen_readback_scene(ScenePass::world_and_rings(&g, &[]))` | the two `Readback.rgba` buffers are byte-equal |
| `host_golden_still_matches` (GPU, existing) | unchanged | `lab/goldens/linux-vulkan/` compares exactly — **must not be regenerated** |
| `frame_pack_allocates_nothing` (existing, `frame_allocations.rs`) | unchanged | still green — `group_ranges` is reserved at construction |

**Mutation verification (mandatory).** Inject, confirm red, revert, confirm green:
1. Draw `ui` before `overlay` → kills `ui_layer_draws_over_the_world`.
2. Draw `ui` on `self.pipeline` instead of `ring_pipeline` → kills `ui_layer_is_not_depth_tested`.
3. `validate_scene` uses `>` instead of `>=` → kills `an_out_of_range_slot_is_rejected`.
4. `texture_at` returns `atlas_tex[0]` for every slot → kills `a_phase1_slot_renders` and `ui_layer_draws_over_the_world`.
5. Move the `total > MAX_INSTANCES` check after the `extend_from_slice` loop → kills `instance_budget_is_checked_before_packing` only if that test also asserts `pack_scratch` capacity is unchanged; add that assertion.
6. Drop the legacy `validate_groups` call from the wrappers → kills the two legacy-contract tests.

## Impl steps

- [x] 1. Add the slot constants and `RTS_FILES` to `crates/mmd-engine/src/render/atlas.rs`.
- [x] 2. Add `rts_atlas_dir` / `ui_atlas_dir` to the same file.
- [x] 3. Add a private `PlaceholderManifest` deserialiser to `atlas.rs` matching the JSON shape quoted in Inputs.
- [x] 4. Implement `load_rts_atlases` and `load_ui_font` with per-file sha256 verification.
- [x] 5. Extend `crates/mmd-engine/src/render/mod.rs` exports.
- [x] 6. Write the failing headless tests in `crates/mmd-engine/tests/render_correctness.rs`.
- [x] 7. Write the failing GPU tests in `crates/mmd-engine/tests/gpu_smoke.rs`. **Deviation:** written in `crates/mmd-engine/tests/render_correctness.rs` instead. The auto-skip helper the ticket cites is `renderer_or_skip` in `render_correctness.rs`, not `tests/common/mod.rs` (which has no GPU helper), and integration-test binaries cannot share a private helper. `gpu_smoke.rs`'s own module doc routes correctness cases that must *run by default* to `render_correctness.rs`; its device tests are `#[ignore]`, so tests placed there would never run under the ticket's `cargo test --workspace` validation command.
- [x] 8. Run `MMD_REQUIRE_GPU=1 cargo test -p mmd-engine` and record the failures.
- [x] 9. Add `ScenePass` + `ScenePass::world_and_rings` to `renderer.rs`.
- [x] 10. Add `extra_tex`, `extra_cpu`, `group_ranges` fields to `SpriteRenderer`; reserve `group_ranges` with `Vec::with_capacity(ATLAS_SLOT_COUNT)` in the constructor.
- [x] 11. Load and upload the five new textures in `with_context`, from `atlas_dir.join("rts")` and `atlas_dir.join("ui")`.
- [x] 12. Add `texture_at` and `atlas_pixels`.
- [x] 13. Rename `draw_offscreen_into` to `draw_scene_into` and rewrite its body to the ten steps above.
- [x] 14. Add `validate_scene`; keep `validate_groups` and call it from the legacy wrappers.
- [x] 15. Add `draw_offscreen_scene`, `draw_to_swapchain_scene`, `draw_offscreen_readback_scene`.
- [x] 16. Reduce `draw_offscreen_with_rings`, `draw_to_swapchain_with_rings`, `draw_offscreen_readback_with_rings`, `draw_offscreen_acquire_fence` to wrappers.
- [x] 17. Run the mutation list; record kills in the commit body.
- [x] 18. Run the full validation block.

## Outputs

- **Files edited**
  - `crates/mmd-engine/src/render/atlas.rs`
  - `crates/mmd-engine/src/render/renderer.rs`
  - `crates/mmd-engine/src/render/mod.rs`
  - `crates/mmd-engine/tests/render_correctness.rs`
  - ~~`crates/mmd-engine/tests/gpu_smoke.rs`~~ — not edited; see the deviation
    note on Impl step 7.
- **Public API added:** `ScenePass`, `draw_offscreen_scene`,
  `draw_to_swapchain_scene`, `draw_offscreen_readback_scene`,
  `SpriteRenderer::atlas_pixels`, the nine slot constants,
  `load_rts_atlases`, `load_ui_font`, `rts_atlas_dir`, `ui_atlas_dir`.
  Also `SpriteRenderer::pack_capacity`, required by mandatory mutation 5
  ("add that assertion") — `pack_scratch` is private, so the budget guard's
  before-vs-after-packing ordering had no other observation seam.
- **Behaviour change:** a draw group may now name any of 9 texture slots; a
  screen-space UI layer draws last with depth off.
- **Migration / config:** none. `shaders/sprite.hlsl` is untouched.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked`
- [x] `VK_DRIVER_FILES=/nonexistent cargo test --workspace --locked` — GPU cases skip cleanly, nothing fails
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check`
- [x] `cargo run -p xtask -- shaders --check` — green, shader untouched
- [x] `cargo run -p xtask -- atlases --check`
- [x] `git diff --stat HEAD -- lab/goldens/` — **empty**: the golden is compared, never regenerated by this ticket
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exit 0
- [x] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` — exit 0
- [x] `cargo tree -e features | grep -c testkit` — `0`
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `feat(render): draw from a flat texture table behind a scene pass`
