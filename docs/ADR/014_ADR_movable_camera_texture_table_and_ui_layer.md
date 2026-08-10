# ADR 014: Movable camera, texture table and the UI layer

- Status: Accepted
- Date: 2026-08-10
- Supersedes in part: [ADR 012](012_ADR_starcraft_scale_and_isometric_render.md)
  — its decision **(e) "the camera is fixed, not scrolling"** no longer holds.
  ADR 012 named scrolling, edge-pan, zoom and selection as phase-1 work; this
  record ships the first two and defers zoom. Every other decision in ADR 012
  stands: the `GREATER`/clear-`0` depth test, the alpha cutout, the 48-byte
  `SpriteInstance`, the ring sentinel, `MAX_LIVE_AGENTS = 5_000`.
- Supplements: [ADR 004](004_ADR_sdl3_sprite_renderer_and_assets.md)
- Plan: `ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`

## Context

Phase 1 needs three things the phase-0 renderer cannot do: a camera that moves,
sprites that are not zombies, and readable text on screen. Each of them threatens
something already locked — the depth normalisation, the `atlas_count: 4`
scenario contract, and the host render golden.

## Decision

**(a) The camera pans; it does not zoom.** `IsoView` gains `with_center_cell`,
which re-derives `origin` **and** `depth_bias` together. That pairing is the
decision: the depth key is `ground_y * depth_scale + depth_bias`, and
`depth_bias` carries `-origin.y`, so the key is the agent's position down the
*map* diamond and does not move when the camera does. Recomputing `origin`
without `depth_bias` would make the horde re-sort itself as the view scrolled —
the bug this note exists to prevent.

Zoom is deliberately out. It multiplies through the tile size, the depth
normalisation, the AABB cull and every render golden, and non-integer tile
scales shimmer on pixel art. Pan and edge-pan answer the phase's question; zoom
gets its own ticket in a later phase.

`render::Camera` owns a cell-space centre clamped to the closed rectangle
`0..=width` × `0..=height`, and pans at `CAMERA_PAN_CELLS_PER_SEC = 24.0` —
three times the horde walk speed, so a player can outrun the unit they just
ordered. A diagonal is **not** normalised: holding two keys pans faster on the
diagonal, as in the genre.

**(b) `iso_unproject` is the inverse, and it is exact.** Every mouse
interaction — select, order, place — is `screen → cell`. From
`sx = ox + (cx - cy)·tw/2` and `sy = oy + (cx + cy)·th/2`:
`u = (sx-ox)/(tw/2)`, `v = (sy-oy)/(th/2)`, `cx = (u+v)/2`, `cy = (v-u)/2`.
A degenerate tile yields `NaN`, not an infinity, so a caller's bounds check
rejects it instead of indexing the far edge of the grid.

**(c) `DrawGroup.atlas_id` becomes a texture *slot*, not an atlas index.**
The renderer holds a flat table of `ATLAS_SLOT_COUNT = 9` textures: `0..=3` the
phase-0 zombie skins, unchanged and byte-identical; `4` worker, `5` soldier,
`6` buildings and resource nodes, `7` props, `8` the UI font.

This was chosen over the two obvious alternatives. Growing `ATLAS_COUNT` from 4
to 8 would break `atlas_count: 4` in the scenario contract, which every family
including the fixtures is validated against, and would ripple through the whole
phase-0 test corpus and the golden. A fifth *atlas family* with its own pipeline
would double the shader and pipeline surface for content that wants the same
vertex layout. A flat table changes one integer's meaning and nothing else: the
shader, the vertex format, the blend state and `SpriteInstance`'s 48 bytes are
untouched, and `xtask shaders --check` stays green without the shader being
edited.

The phase-0 four-group array API is **kept**, not replaced. Its strict check —
exactly `ATLAS_COUNT` groups, `atlas_id == index` — still runs from the legacy
wrappers, so `RenderError::GroupCount` stays reachable and its test stays green.

**(d) The phase-1 sheets keep the phase-0 frame geometry.** Every RTS sheet is
128 × 256: 4 columns × 8 rows of 32 px frames, exactly like a zombie atlas. Unit
sheets read that grid as `(dir, frame)`; the buildings and props sheets read the
*same* grid as a static `(row, col)` table. One consequence: `frame_uv_rect`
addresses all of them with no change, and the atlas manifest checker validates
them with the same layout rules.

**(e) A frame is three layers, and the third one is new.**

```rust
pub struct ScenePass<'a> {
    pub world: &'a [DrawGroup],        // depth-tested
    pub overlay: &'a [SpriteInstance], // depth off, procedural rings only
    pub ui: &'a [DrawGroup],           // depth off, textured, drawn last
}
```

`world` draws on the existing depth pipeline; `overlay` and `ui` on the existing
ring pipeline, whose depth test and depth write are both off. No third pipeline,
no new shader module.

`overlay` binds texture slot 0 and is honest **only for procedural ring
instances** — the ring branch samples no texture, and slot 0 is bound merely
because a draw must not leave the pipeline's one declared sampler unbound.
Textured depth-off content must go in `ui`. That is why the selection ring is a
procedural ring rather than a sprite: it costs no texture bind, it reuses the
shader branch the hitbox overlay already proved, and it is sized by the same
`ring_quad_size_px` expression, so "the ring shows the shape the world uses"
stays one derivation. Its tint is green, deliberately not the hitbox overlay's
cyan — two rings meaning different things in one colour would be worse than no
ring.

**(f) The golden survives because the draw order is unchanged when `ui` is
empty.** `draw_offscreen_with_rings(groups, rings)` is reimplemented as
`draw_offscreen_scene(ScenePass::world_and_rings(groups, rings))`, and with an
empty UI layer that emits exactly the phase-0 sequence of binds and draws. The
host golden under `lab/goldens/linux-vulkan/` is **compared, never regenerated**,
by every ticket in this plan.

**(g) Text is a bitmap font in slot 8, generated from code.** 8 × 8 glyphs,
16 × 6 cells, ASCII 32..=127. Real forms exist for space, digits, uppercase and
a fixed punctuation set; **everything else in range, including all lowercase, is
a fallback box glyph**, and `push_text` uppercases ASCII before lookup. A box is
therefore visible proof of a genuinely unmapped character rather than a silently
wrong one. Space advances the cursor and emits **no** instance — a blank cell
would be an invisible quad the GPU still rasterises, and the HUD pads with
spaces.

**(h) All phase-1 art is procedurally generated and hash-tracked.** `xtask
atlases --check` now verifies three families — the pinned CC0 zombie set,
`rts/`, and `ui/` — by regenerating each into a temp directory and byte-comparing
against the tracked PNGs, with the same premultiplied-alpha assertion. The
placeholders carry no third-party licence obligation, and real art later is a
generator swap with no code change.

## Consequences

- ADR 012's "the camera is fixed" line is history. Everything else in it stands.
- The depth uniform is now set per **scene** from a camera that moves, rather
  than once per scene from a fixed origin. `depth_key_is_camera_independent` is
  the guard: one cell's depth key under three camera centres must be equal.
- `SpriteInstance` is still 48 bytes and `shaders/sprite.hlsl` is still
  byte-unmodified. Both are asserted.
- The `overlay`/`ui` split is a real rule a future contributor can get wrong:
  a textured instance pushed into `overlay` will sample the zombie atlas.
  It is documented on the field, on the packer, and in `AGENT.md`.
- Real art will change every hash in `assets/sprites/generated/rts/manifest.json`
  and `ui/manifest.json`. That is the intended failure mode — the check exists
  so an accidental change is loud and a deliberate one is reviewed.
- Nothing here claims a speed. The texture table adds up to five extra texture
  binds per frame; no number is measured, published, or gated on.
- **Corrected during implementation.** The `overlay`/`ui` split shipped
  stricter than the field's own doc comment read. As built, `overlay` carries
  **only** procedural rings — the horde's hitbox rings and the RTS selection
  rings — and every textured depth-off element lives in a `ui` draw group:
  placement tiles, the drag box, the rally flag, the resource and supply icons,
  the HUD panel fill (slot 7, `rts/props.png`) and every glyph (slot 8,
  `ui/font.png`). The stale comment on `ScenePass::overlay`, which still listed
  placement tiles as overlay content, was corrected to match the code rather
  than the code loosened to match it. `selection_rings_are_procedural` and
  `hud_uses_only_the_two_ui_groups` pin the rule.
- **Corrected during implementation.** `SpriteRenderer::pack_capacity` was
  added as a read-only observation seam so a mutation test can see that the
  per-frame pack buffer is reserved once and never grown, instead of the claim
  living only in a comment. A stale note in `render/instance.rs` still listing
  zoom as phase-1 work was corrected too: decision (a) defers it.
