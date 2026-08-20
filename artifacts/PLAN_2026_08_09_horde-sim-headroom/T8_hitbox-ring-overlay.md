# T8: Hitbox ring overlay

**Plan:** `./artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
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

- [x] 1. `graphify query "sprite instance packing draw groups renderer pipeline"` and
      `graphify path "Runtime" "SpriteRenderer"` before touching anything.
      (criterion: both commands run, subgraph + path returned — `Runtime <-- run_phase() --> SpriteRenderer`,
      2 hops, via `--undirected`; plus `graphify query "shader spirv manifest xtask check generated blobs"`.)
- [x] 2. Write the tests. Run — red.
      (criterion: `cargo test --workspace --locked --no-run` fails with `RING_SENTINEL`,
      `SpriteInstance::ring`, `is_ring`, `BoundKey::H`, `InputAction::ToggleHitboxes`,
      `Runtime::hitboxes_visible`, `Runtime::ring_instances`, `FrameOutput.rings`,
      `RING_INNER`/`RING_OUTER`/`RING_TINT`/`ring_radius_px` all unresolved.)
- [x] 3. Edit `shaders/sprite.hlsl`. Regenerate the SPIR-V and update
      `shaders/generated/manifest.json`; run `cargo run -p xtask -- shaders --check`.
      The DXIL and metallib slots follow the existing deferred-placeholder path on
      this host — **do not** invent native blobs; record in Outputs that the
      Windows and macOS reference hosts owe a native rebuild.
      (criterion: `cargo run -p xtask -- shaders --check` →
      `shaders: ok (spirv+dxil+metallib; native DXIL/metallib regen still host-gated T9/T10)`.
      Manifest re-pinned: canonical `c290e6e4…`, vert `9d0b637b…`, frag `3cd95c43…`;
      dxil stays `placeholder`/`T9`, metallib `placeholder`/`T10` — no native blobs invented.)
- [x] 4. Add `SpriteInstance::ring`.
      (criterion: `ring_instances_keep_the_pinned_layout` passes and
      `instance_layout_is_stable` passes **unmodified** — both green in
      `cargo test -p mmd-engine --lib render::instance`.)
- [x] 5. Add `pack_ring_instances` + the `Runtime` field, capacity, flag and accessor.
      (criterion: `a_ring_is_packed_for_every_agent`, `a_bodyless_scene_packs_no_rings`,
      `the_ring_traces_the_real_body` and `ring_packing_allocates_nothing` all pass.)
- [x] 6. Add `BoundKey::H` / `InputAction::ToggleHitboxes` / the binding.
      (criterion: `input_actions_are_stable` pins `ToggleHitboxes as u8 == 3`;
      `keyboard_and_script_agree` covers the new row; a bad `--inject-input` key now
      reports `valid: esc, f1, space, h`; `--inject-input 3:h` fires and exits 0.)
- [x] 7. Draw the ring slice after the atlas groups in both the offscreen and
      swapchain paths.
      (criterion: `render_offscreen` appends a fifth instance range used by BOTH
      `draw_offscreen_with_rings` and `draw_to_swapchain_with_rings`; the GPU test
      `a_ring_is_hollow` passes under `MMD_REQUIRE_GPU=1`, which is only possible if
      the ring geometry actually reached the render pass.)
- [x] 8. Regenerate the host goldens — `MMD_UPDATE_GOLDEN=1 cargo test -p mmd-engine
      --test gpu_golden -- --ignored update_host_golden` — and eyeball the new
      images before committing them. Record in Outputs that the goldens moved and
      why.
      (criterion: command exits 0; `lab/goldens/linux-vulkan/golden.png` byte-compared
      before/after → **identical** (`ad843c49…`), and the image was opened and viewed.
      Only `shader_canonical_sha256` moved, in five manifests. See Outputs.)
- [x] 9. `graphify update .`.
      (criterion: run as the last action before committing —
      `Rebuilt: 3833 nodes, 7372 edges, 239 communities`; `graphify-out/` stays unstaged.)

## Outputs

- **A visible, correct hitbox for every entity.** The ring quad is
  `2 * collision_radius_cells * cell_size_px` centred on the agent, read from
  the live scenario via `Scenario::collision_radius_cells()`. On the three
  tracked scenes that is `2 * 6.0 * 4 = 48 px` — exactly the sprite — so the
  ring sits on the sprite's edge. `RING_OUTER = 0.5` (the quad edge, i.e. the
  true contact circle) and `RING_INNER = 0.5 - 1/32` give a 1.5 px band.
  `RING_TINT = [0.0, 0.55, 0.55, 0.55]` is premultiplied cyan at 55 % alpha.
- **`H` toggles it.** `BoundKey::H` → `InputAction::ToggleHitboxes` (`= 3`,
  appended). Headless drive is `--inject-input <FRAME>:h` — note the real,
  test-enforced grammar is `FRAME:KEY`, so this ticket's `h@N` shorthand is
  spelled `3:h`. Verified: `run --scenario collision_sprite_v1.ron --frames 6
  --inject-input 3:h` exits 0 with the press fired, and a bad key now reports
  `valid: esc, f1, space, h`.
- **The ring reads the radius off the `Simulation`, not the `Scenario`.**
  Both derive it from `collision_radius_q8`, but only `CollisionParams::radius_cells`
  is the number the separation pass actually pushes on. Going through the
  scenario left two derivations free to drift, which would break the one claim
  the overlay makes. `the_ring_traces_the_real_body` asserts the two agree.
- **Shaders.** `shaders/generated/sprite.vert.spv` and `sprite.frag.spv`
  regenerated; `shaders/generated/manifest.json` re-pinned to
  `canonical a27856e6b17a26f7a3fd91945637c503690564255739c634012098b1c57aff63`,
  `vert 9d0b637b0a45480fff23b1ef5a8ee903559cdaa83828d1909d17e47e7fda09bb`,
  `frag 3cd95c43d822b57493e60955bc67823f7ab2510ee77698c3ad98e8a447eae3df`.
  **The DXIL and metallib slots are untouched placeholders (`deferred: T9` /
  `T10`) — the Windows and macOS reference hosts owe a native rebuild of these
  shaders before their backends can run this branch.**
  Reproduction: this repo tracks no GLSL mirror even though the Linux SPIR-V is
  built from one (`shaders/generated/README.md`, and `host_shader_spec` expects
  entry point `main`). The mirror was reconstructed by disassembling the pinned
  blobs and was **proved byte-identical to both tracked `.spv` files before any
  edit**, which is what makes this regeneration trustworthy. Recipe:
  `nix shell github:NixOS/nixpkgs/148bab9c1c3c53136ecb44a6ea356a0ed5b39b06#shaderc`,
  then `glslc -fshader-stage=vertex sprite.vert.glsl -o sprite.vert.spv` (and
  `fragment`), where the mirror is `shaders/sprite.hlsl` transliterated with
  `set = 1, binding = 0` for the uniform block and `set = 2, binding = 0` for
  the sampler. **Follow-up (out of scope here): nothing in
  `xtask::shaders::check_shaders` binds the `.spv` blobs to the `.hlsl` source —
  it only checks that each file matches its recorded hash — so re-pinning
  `canonical_sha256` without regenerating would pass every offline gate.**
- **Goldens.** `lab/goldens/linux-vulkan/golden.png` was regenerated with the
  documented `MMD_UPDATE_GOLDEN=1` command and is **byte-identical** to the
  previous capture (`ad843c490b952b17dafb9622091b1626a270fbd74047cc5adf3d7b641b1a429c`)
  — the pixels did **not** move, because the golden scene is
  `SpriteRenderer::static_demo_groups()`, which draws no rings, and the sprite
  path is behaviourally unchanged. The image was opened and visually inspected
  (four sprites, no rings) before committing. What did move is
  `shader_canonical_sha256`, which is pinned in **five** tracked manifests, all
  re-pinned:
  `lab/goldens/{linux-vulkan,windows-d3d12,macos-metal}/manifest.json` and
  `lab/fixtures/{windows,macos}-candidate/golden/manifest.json`. The last two
  are live-checked by the mmd-lab merge gate, so missing them would have failed
  `merge_gate_all_pass_allows_exact_hash`. Because the golden did not move,
  `lab/fixtures/ubuntu-candidate/readback-pass.png` (byte-identical to the
  golden) stays valid and needed no regeneration.
- **The gate digest is unchanged from T0.** Observed:
  `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
  from `cargo run -- run --agents 5000 --frames 300`, exit 0 — byte for byte
  T0's value. `git diff --stat crates/mmd-engine/src/sim/` is empty.
- **Bench integrity.** `bench::runner::run_scale_point` now sets
  `hitboxes_visible = false`: the bench submits only `out.groups`, so packing
  rings it never draws would have charged the frozen phase-0 ladder's
  `upload_ms` for discarded work. Two consequences, recorded rather than fixed:
  the ladder therefore measures a configuration the app does not ship (the app
  default is rings **on**, roughly doubling per-frame pack cost — measured at
  ~1.0–1.4 ns/agent against the sprite pack's 2.02 ns/agent, so ≈5–7 µs at
  5 000 agents), and the shader's new branch/varying still costs every sprite
  draw ≈0.03 ms at the recorded 50k point (~1.8 % of `gpu_queue_latency_ms`)
  regardless of the flag, so GPU-side ladder numbers are a fresh baseline.
  Measured fill cost of the overlay itself at 5 000 agents: 11.52 Mfrag/frame,
  ≈0.166 ms upper bound, ≈1 % of the 16.67 ms budget.
- **Visual evidence gathered off-screen** (the windowed check is still manual):
  an offscreen readback of `collision_sprite_v1` at 600 agents rendered one thin
  hollow cyan ring per agent with sprites visible through them, and an A/B with
  the overlay hidden showed identical sprites and no rings.
- **`hitboxes=` added to the `run` exit line.** Without it a scripted `h` press
  produced byte-identical stdout whether the binding worked or was dropped, so
  the "`--inject-input` drives it headlessly" claim was unfalsifiable. The
  stdout contract doc in `src/run.rs` is updated and
  `cli_contract::hitbox_toggle_is_scriptable` now pins the flip end to end.
- **`every_tracked_manifest_pins_the_live_shader_and_atlas`** (new, headless)
  walks `lab/goldens/*/manifest.json` and `lab/fixtures/*/golden/manifest.json`
  and asserts each pin equals the live `host_binding_hashes`. This ticket had to
  re-pin five manifests by hand and two of them are reachable only from hardware
  this project does not own; the test turns "five manifests, all remembered"
  into "every manifest, enforced on every host".

### Follow-ups this ticket deliberately did not take (out of scope)

1. **Nothing binds the SPIR-V blobs to `shaders/sprite.hlsl`.**
   `xtask::shaders::check_shaders` verifies each artifact against its *recorded
   hash*, never against the source, and the GLSL mirror the Linux blobs are
   compiled from is not tracked. Editing the HLSL, re-pinning
   `canonical_sha256` and leaving the `.spv` alone would pass every offline
   gate. Suggested fix: track the mirror under `shaders/` and have
   `check_shaders` recompile and byte-compare when `glslc` is available.
2. **`validate_resources` compares the manifest to hardcoded literals**, not to
   SPIR-V reflection, so the advertised resource contract is an unverified
   assertion. (Verified by hand for this change.)
3. **Bench reports bind the shader only via `shader_manifest_version: 1`**, a
   schema number that cannot move when the shader does; `docs/ADR/005` claims
   the shader manifest binds the baseline, which overstates the gate.
4. **DXIL/metallib placeholders keep their old hashes** while
   `canonical_sha256` moved, and `validate_deferred_placeholders` accepts a slot
   as native on magic bytes alone — a reference host could later drop in blobs
   built from an older HLSL and pass.
5. **Stale docs falsified before this ticket:** `testkit/fixtures.rs` describes
   the collision scenes with pre-T0 numbers ("10 000 agents", "1.25-cell body",
   "3.75-cell body … 30 px sprite") when both are now 6.0 cells / 48 px, and
   `docs/ADR/004` still says the frame is "4 atlas draws" when it is now up to
   five. Both belong to T0/T7's territory.
6. **`docs/05-testing.md` key list** does not mention `H`.
7. **`.tmp/` is not gitignored**, so a `git add -A` would sweep orchestrator
   scratch files into a commit. This ticket staged explicit paths only.
8. **The renderer's ring append has no zero-allocation gate.** The bench runs
   rings off by design, so `render_offscreen`'s `extend_from_slice(rings)` and
   the fifth draw are never inside a `MeasureGuard`;
   `ring_packing_allocates_nothing` covers the runtime packer only.

## Validation

- [x] `cargo fmt --all -- --check` → clean, no output
- [x] `cargo test --workspace --locked` → all binaries `ok`, 0 failed
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` on the GPU host —
      including `a_ring_is_hollow`
      → `render_correctness`: **19 passed; 0 failed; 0 ignored**, so
      `a_ring_is_hollow`, `golden_frame_matches`, `golden_drift_fails_on_gpu`,
      `world_to_clip_matches_gpu_raster` and `renderer_smoke_device_resize_shutdown`
      all really ran instead of skipping (adapter: NVIDIA GeForce RTX 5060 Ti).
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0
- [x] `nix flake check` → `all checks passed!`
- [x] `cargo run -p xtask -- bootstrap --check` →
      `bootstrap: ok (SDL 3.4.12 / sdl3 0.18.4 / sdl3-sys 0.6.7; win/mac native host-gated T9/T10)`
- [x] `cargo run -p xtask -- shaders --check` →
      `shaders: ok (spirv+dxil+metallib; native DXIL/metallib regen still host-gated T9/T10)`
- [x] `cargo run -p xtask -- atlases --check` → `atlases: ok (4 png + manifest)`
      (the scenario contract's `atlas_count: 4` is unchanged — the ring is
      procedural, there is no fifth atlas)
- [x] `cargo run -- run --agents 5000 --frames 300` — exits 0, `hash=` equals
      T0's pinned digest
      → `clean exit mode=window backend=vulkan tick=300 frames=300
      hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
- [x] `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300` — exits 0
      → `clean exit mode=window ... tick=300 frames=300 hash=5561f201e804…`
- [x] `cargo test -p mmd-lab --test merge_gate` → 5 passed, 0 failed
      (added by this ticket: `merge_gate_all_pass_allows_exact_hash` binds the
      *live* `shader_canonical_sha256` against
      `lab/fixtures/{windows,macos}-candidate/golden/manifest.json`, so it is the
      gate that proves the five-manifest re-pin was complete. Re-pinning only the
      three `lab/goldens/*` manifests would have failed here.)
- [x] `cargo tree -e features | grep -c testkit` → 0
- [x] `git diff --stat crates/mmd-engine/src/sim/` → empty
- [x] `graphify update .` run → `Rebuilt: 3833 nodes, 7372 edges, 239 communities`
      (`graphify-out/` is gitignored and is never staged)
- [ ] manual check: `cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron`
      — every unit carries a hollow ring that touches its neighbour's ring when
      they press together; `H` hides and shows them. Esc to quit.
      **Deliberately left unchecked — needs a real window and a human.** Not run
      headless and does not gate this ticket. Steps are in
      `artifacts/manual_test_checklist.md` → `## T8 hitbox-ring-overlay`.
      Off-screen substitute evidence (an offscreen readback rendered to PNG and
      viewed, plus an A/B with the overlay hidden) is recorded in Outputs.
