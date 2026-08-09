# T8: Hitbox ring overlay

**Plan:** `./ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** T0
**Commit outcome:** Every entity draws a ring at its **real** body radius —
procedural, atlas-free, zero-allocation, on by default, toggled with `H`.

## Context (self-contained)

- Soft separation is invisible today: the body radius is a number in a `.ron`
  file and nothing on screen shows where it is. After T0 the body is exactly
  half a sprite, and the whole point of that number is that you can *see* two
  agents touching edge-to-edge. This ticket makes the body visible.
- This slice: render only. `crates/mmd-engine/src/sim/` is not touched, no state
  hash moves, the `hash=` on the gate smoke is byte-for-byte T0's value.
- The ring shows `collision_radius_cells * cell_size_px`, read from the live
  scenario — never a constant, never an approximation. A ring that disagrees
  with the sim is worse than no ring.
- **Chosen approach — one shader, one branch.** `shaders/sprite.hlsl` gains a
  ring branch selected by a sentinel in the existing `uv_rect` field. Rings are
  extra `SpriteInstance` records in their own draw group, submitted after the
  four atlas groups.
- **Rejected alternatives**, recorded here so they are not re-litigated:
  - *A fifth atlas holding a ring texture* — breaks the scenario contract's
    `atlas_count: 4` check and `xtask atlases --check`, and quantises the ring
    to one radius.
  - *A new `shaders/debug_ring.hlsl` family* — needs three new generated
    artifacts (`.spv` / `.dxil` / `.metallib`), new `shaders/generated/manifest.json`
    entries, new deferred-placeholder handling in `xtask/src/shaders.rs` (whose
    `check_shaders` currently hardcodes `["sprite.vert.spv", "sprite.frag.spv"]`),
    and a native rebuild on the Windows and macOS reference hosts. Far more
    blast radius than a branch buys back.
  - *A new field on `SpriteInstance`* — breaks `instance_layout_is_stable`
    (pinned at 48 bytes) and the vertex `TEXCOORD` contract in the reflection
    manifest.
- Out of scope here: the isometric projection and depth ordering (T9 — the same
  ring shader renders an ellipse there for free, because the quad becomes 2:1),
  selection circles, health bars, any HUD change.
- Assumptions in force:
  - `MMD_UPDATE_GOLDEN=1` is the documented, reviewed way to move a host
    golden. Rings default to **on**, so the goldens move; that is expected and
    is a reviewed step of this ticket, not a surprise.
  - Zero per-frame allocation still holds: the ring buffer is reserved at load
    for `MAX_LIVE_AGENTS`.
  - `graphify` is installed. Orient with `graphify query`; end with
    `graphify update .`.

## Requirements

- `shaders/sprite.hlsl`:
  - `VSOutput` gains `float3 ring : TEXCOORD2` = `(is_ring, inner, outer)`.
    Vertex *input* layout is unchanged, so the reflection manifest's resource
    contract (1 uniform buffer, 1 texture, 1 sampler) is unchanged.
  - `VSMain`: an instance with `uv_rect.x < 0.0` is a ring. For it,
    `output.uv = input.uv` (raw unit-quad coords, no atlas lerp) and
    `output.ring = float3(1.0, uv_rect.y, uv_rect.z)`. Otherwise
    `output.ring = float3(0, 0, 0)` and the atlas lerp is exactly as today.
  - `PSMain`: when `ring.x > 0.5`, compute `d = length(input.uv - 0.5)` and
    return `tint` where `inner <= d <= outer`, else discard (`clip`). The
    sprite path is byte-identical to today.
- `crates/mmd-engine/src/render/instance.rs` gains
  `SpriteInstance::ring(pos, size, inner, outer, tint) -> Self`, which writes
  `uv_rect = [-1.0, inner, outer, 0.0]`. `SpriteInstance` **stays 48 bytes**;
  `instance_layout_is_stable` must pass unmodified.
- `crates/mmd-engine/src/runtime.rs`:
  - `pack_instance_groups` gains a sibling `pack_ring_instances(agents,
    cell_size_px, radius_cells, out: &mut Vec<SpriteInstance>)`. Quad size is
    `[2 * radius_cells * cell_size_px; 2]`, centred on the agent, so the ring
    traces the true contact circle.
  - `Runtime` gains `ring_instances: Vec<SpriteInstance>` reserved with
    `MAX_LIVE_AGENTS` capacity at load, and `hitboxes_visible: bool` defaulting
    to `true`.
  - `FrameOutput` exposes the ring slice alongside `groups`.
  - A bodyless scenario (`collision_radius_q8 == 0`) produces **zero** rings —
    there is no body to draw.
- `mmd_engine::runtime`: `BoundKey` gains `H`, `InputAction` gains
  `ToggleHitboxes`, `action_for_key` maps them. `src/input.rs` `BINDINGS` gains
  `(BoundKey::H, Keycode::H, "h")`, so `--inject-input h@N` works and
  `key_names()` lists it.
- `crates/mmd-engine/src/render/renderer.rs` draws the ring instances after the
  four atlas groups in the same pass, same pipeline, same blend state. The ring
  is atlas-free but the pipeline still has a texture bound — bind atlas 0 and
  let the branch ignore it rather than adding a second pipeline in this ticket.
- Ring colour is one premultiplied constant in `runtime.rs`, documented, with a
  visible alpha < 1 so a dense crowd of rings does not read as a solid mass.
  `inner`/`outer` are constants in normalised quad units giving a ring one to
  two pixels thick at the T0 body size.
- Zero per-frame allocation holds with rings on **and** off.

## Inputs

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

- `shaders/sprite.hlsl` — `VSOutput` (~L27), `VSMain` (~L34), `PSMain` (~L50).
- `crates/mmd-engine/src/render/instance.rs` — `SpriteInstance` (~L11),
  `QUAD_VERTICES` (~L62), the layout test (~L128).
- `crates/mmd-engine/src/runtime.rs` — `Runtime` (~L78), `from_scenario` (~L107),
  `pack_instance_groups` (~L301), `build_instance_groups` (~L332),
  `FrameOutput` (~L58).
- `crates/mmd-engine/src/render/renderer.rs` — shader includes (~L39–49),
  pipeline creation (~L135), `draw_offscreen_acquire_fence` (~L354),
  `draw_to_swapchain` (~L507).
- `src/input.rs` — `BINDINGS` (~L10), `key_names` (~L53).
- `src/run.rs` — the frame body that consumes `FrameOutput`.
- `crates/mmd-engine/tests/render_correctness.rs` — where the new render tests
  go (`nonclear_bbox` ~L149 and `opaque_frame_bounds` ~L1015 are the helpers to
  reuse).
- `crates/mmd-engine/tests/runtime_frame.rs` — `input_actions_are_stable`.
- `tests/cli_contract.rs` — the `--inject-input` cases.
- **From Depends (T0):** `MAX_LIVE_AGENTS`, the 48 px sprite and the 1 536 q8
  body; the pinned gate digest this ticket must reproduce unchanged.

**Existing signatures this ticket must preserve:**

```rust
// crates/mmd-engine/src/render/instance.rs
impl SpriteInstance {
    pub const STRIDE: u32;              // stays 48
    pub const WHITE: [f32; 4];
    pub fn new(pos: [f32; 2], size: [f32; 2], uv_rect: [f32; 4], tint: [f32; 4]) -> Self;
}
pub fn world_to_clip(pos: [f32; 2], size: [f32; 2], corner: [f32; 2], view_size: [f32; 2]) -> [f32; 4];
pub fn clip_to_pixel(clip: [f32; 4], view_size: [f32; 2]) -> [f32; 2];

// crates/mmd-engine/src/runtime.rs
pub fn pack_instance_groups(agents: AgentsView<'_>, cell_size_px: f32, sprite_size_px: f32, out: &mut [DrawGroup; ATLAS_COUNT]);
pub fn build_instance_groups(agents: AgentsView<'_>, cell_size_px: f32, sprite_size_px: f32) -> [DrawGroup; ATLAS_COUNT];
```

`pack_instance_groups` keeps its signature; rings go through a new function so
existing callers and their tests are untouched.

## TDD

1. **Red** — the seven tests below. They fail: `SpriteInstance::ring` does not
   exist, `BoundKey::H` does not exist, and no ring geometry is produced.
2. **Green** — shader branch, `ring()` constructor, `pack_ring_instances`,
   the runtime flag, the key binding, the extra draw.
3. **Refactor** — make the ring's radius derivation a single expression shared
   by the packer and the test helper, so the "ring shows the real body" claim
   cannot drift.

## Test plan

| Test | Where | Asserts |
| ---- | ----- | ------- |
| `a_ring_is_packed_for_every_agent` | `render_correctness.rs` | ring count == agent count on a bodied scene |
| `a_bodyless_scene_packs_no_rings` | `render_correctness.rs` | ring count == 0 when `collision_radius_q8 == 0` |
| `the_ring_traces_the_real_body` | `render_correctness.rs` | for a known agent, the ring quad's width equals `2 * collision_radius_cells * cell_size_px` and its centre equals the sprite's centre — derived from the scenario, not hardcoded |
| `ring_instances_keep_the_pinned_layout` | `instance.rs` unit test | `SpriteInstance::ring(..)` round-trips its `inner`/`outer` and the struct is still 48 bytes |
| `a_ring_is_hollow` (GPU-only) | `render_correctness.rs` | render one agent; the pixel at the ring's centre is background, a pixel on the ring circumference is not — this is what distinguishes a ring from a disc |
| `toggling_hitboxes_changes_only_the_rings` | `runtime_frame.rs` | `ToggleHitboxes` flips ring count between `n` and `0` and leaves `state_hash` and all four atlas groups identical |
| `input_actions_are_stable` (existing, extended) | `runtime_frame.rs` | the new action appears with a stable discriminant; `h` is an accepted `--inject-input` name |
| `ring_packing_allocates_nothing` | `render_correctness.rs` | `MeasureGuard` around a warm pack, rings on → 0 allocations |

## Impl steps

- [ ] 1. `graphify query "sprite instance packing draw groups renderer pipeline"` and
      `graphify path "Runtime" "SpriteRenderer"` before touching anything.
- [ ] 2. Write the tests. Run — red.
- [ ] 3. Edit `shaders/sprite.hlsl`. Regenerate the SPIR-V and update
      `shaders/generated/manifest.json`; run `cargo run -p xtask -- shaders --check`.
      The DXIL and metallib slots follow the existing deferred-placeholder path on
      this host — **do not** invent native blobs; record in Outputs that the
      Windows and macOS reference hosts owe a native rebuild.
- [ ] 4. Add `SpriteInstance::ring`.
- [ ] 5. Add `pack_ring_instances` + the `Runtime` field, capacity, flag and accessor.
- [ ] 6. Add `BoundKey::H` / `InputAction::ToggleHitboxes` / the binding.
- [ ] 7. Draw the ring slice after the atlas groups in both the offscreen and
      swapchain paths.
- [ ] 8. Regenerate the host goldens — `MMD_UPDATE_GOLDEN=1 cargo test -p mmd-engine
      --test gpu_golden -- --ignored update_host_golden` — and eyeball the new
      images before committing them. Record in Outputs that the goldens moved and
      why.
- [ ] 9. `graphify update .`.

## Outputs

- A visible, correct hitbox for every entity, at the body radius the sim
  actually uses.
- `H` toggles it; `--inject-input h@N` drives it headlessly.
- `shaders/generated/*` regenerated for SPIR-V; DXIL/metallib placeholders
  flagged for the reference hosts.
- Host goldens regenerated — list the files that moved.
- The gate digest is **unchanged** from T0: `hash=` must match T0's Outputs
  exactly. Record the observed value here.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --locked`
- [ ] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` on the GPU host —
      including `a_ring_is_hollow`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `nix flake check`
- [ ] `cargo run -p xtask -- bootstrap --check`
- [ ] `cargo run -p xtask -- shaders --check`
- [ ] `cargo run -p xtask -- atlases --check`
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0, `hash=` equals
      T0's pinned digest
- [ ] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` — exits 0
- [ ] `cargo tree -e features | grep -c testkit` → 0
- [ ] `git diff --stat crates/mmd-engine/src/sim/` → empty
- [ ] `graphify update .` run
- [ ] manual check: `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron`
      — every unit carries a hollow ring that touches its neighbour's ring when
      they press together; `H` hides and shows them. Esc to quit.
