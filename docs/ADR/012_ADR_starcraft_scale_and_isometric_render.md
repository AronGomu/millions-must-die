# ADR 012: StarCraft-scale entities, hitbox rings and isometric render

- Status: Accepted
- Date: 2026-08-09
- Supersedes in part: [ADR 004](004_ADR_sdl3_sprite_renderer_and_assets.md) —
  its "fixed atlas order; no per-frame depth sort" line no longer holds. The
  atlas order is still fixed; the "no depth sort" half is superseded by a depth
  *test*, not a sort — see Decision.
- Supplements: [ADR 009](009_ADR_agent_separation_and_collision.md),
  [ADR 010](010_ADR_separation_amortisation_and_push_priority.md),
  [ADR 011](011_ADR_parallel_separation_and_the_allocation_invariant.md)
- Plan: `artifacts/PLAN_2026_08_09_horde-sim-headroom.md`

## Context

Decision of 2026-08-09: make the game feel closer to StarCraft — fewer
simultaneous entities, but bigger and individually legible, rather than a
denser swarm at the same body size. That decision forced three things at once:
an absolute population ceiling, a body big enough that touching agents read as
touching, and a render layer able to draw that body convincingly — a visible
hitbox and a projection that gives bigger sprites a sense of depth instead of
flattening them into a top-down pile.

The constraint that made this tractable: **the projection is a render-layer
concern only.** The simulation stays in Cartesian cell space; an isometric game
is a Cartesian game with a different camera. `git diff --stat
crates/mmd-engine/src/sim/` across the render tickets is empty — nothing in
`sim::` changed to ship the resize's render half, which is exactly what kept
the gate digest `hash=864147ca3a0e09f7ebc5762b778fce193e705a2bc943ceaf67acf087581ee881`
alive across it.

## Decision

**(a) `MAX_LIVE_AGENTS = 5_000` is the absolute ceiling**, enforced by one
`check_population` call at the validator dispatcher rather than per scenario
family. Nothing in this repo is run, tested, benchmarked or claimed above it.
`technical_prototype_v1` is retuned in place — same version id, new locked
constants — rather than frozen and superseded by a v2. A v2 would leave the
loader accepting a scenario version nothing ever exercises again; retuning in
place does not.

**(b) The body grows to exactly half a sprite.** Sprites grow 30 px → 48 px on
the three tracked full-screen scenes, and `collision_radius_q8` grows to
`1_536` (6 cells, 24 px) — exactly half the new sprite width. Contact distance
(`2 × radius`) is therefore one full sprite width, so two agents the separation
pass calls "touching" are edge-to-edge in the art, not overlapping into each
other. The four `fixture_*` scenes are deliberately **not** retuned: they are
grid-shape fixtures whose bodies (`collision_radius_q8: 32`) the arrival-tick
constants in `crates/mmd-engine/tests/simulation.rs` are calibrated against,
and their populations already sit far under the new ceiling.

**(c) The hitbox ring is a shader branch on the existing instance, not a new
shader family or a fifth atlas.** A fifth atlas would break the scenario
contract's `atlas_count: 4` check and `xtask atlases --check`; a new
`SpriteInstance` field would break its pinned 48-byte layout. Instead the ring
reuses `SpriteInstance` verbatim: a sentinel value written to `uv_rect.x`
selects the shader's ring branch, which is safe because every rect
`frame_uv_rect` can emit is a ratio of non-negative integers, so a legitimate
sprite can never collide with the sentinel. The radius drawn is read from
`Simulation::collision()`, not from the scenario document — both derive it from
`collision_radius_q8`, but only the simulation's copy is the number the
separation pass actually pushes on, and a ring that could disagree with the sim
would be worse than no ring. On by default, toggled with `H`.

**(d) Isometric depth is an alpha-tested cutout against a depth test, not a
per-frame sort.** `IsoView` derives the tile size, a fixed camera origin and two
depth-normalisation scalars once per scene; both the CPU instance packer and
the GPU uniform upload read the same value, so they cannot disagree about where
an agent is. The vertex stage keys depth off the quad's *bottom* edge — the
agent's feet — so one sprite carries one depth value and cannot slice into a
neighbour. **The shipped depth test is `GREATER` against an attachment cleared
to `0`** — not `LESS` against a clear of `1`. (The ticket that built this
carried three mutually inconsistent statements of the convention along the way;
under `LESS` the horde rendered back-to-front, observed as 4482 wrong pixels
against the golden. `GREATER`/clear-`0` is the convention that shipped and the
only one this ADR records.) Because the test is `GREATER`, a depth key of
exactly `0.0` was being *discarded* rather than sorted last — on a degenerate
map this could blank an entire frame. The fix floors sprite depth at one
`D16_UNORM` quantum, costing one part in 65 536 of sort precision. The
alternative, switching to `GreaterOrEqual`, was rejected: it admits a tie
between two distinct sprites at that floor and re-blinds the exact occlusion
test the depth buffer exists to provide. Depth correctness under alpha comes
from a `clip()` cutout in the pixel shader, not a sort — pixel art is
effectively 1-bit alpha, so cutout is honest — which is what lets the 4-atlas
batching survive unchanged and keeps `SpriteInstance` at its pinned 48 bytes
(`instance_layout_is_stable` green, byte-unmodified). Rings draw last, on a
second pipeline built from the same shader modules with the depth block off, so
a unit standing in front of one never hides it.

**(e) The camera is fixed, not scrolling.** 5 000 agents at 48 px under a 2:1
projection with an 8×4 px tile puts the 480×270 grid at 3 000×1 500 screen px —
larger than a 1920×1080 view. The shipped camera is a fixed offset centring the
destination cell, plus an AABB reject for quads fully outside the view.
Scrolling, edge-pan, zoom and selection are Phase 1 work, named here so a
reader does not mistake the fixed view for an oversight.

## Consequences

- No public sim API changed shape; every pinned state-hash test that does not
  touch the render path is untouched by this decision.
- **Resolved in T10 (was a known follow-up).** The ring ellipse originally
  shipped at `[2r·tile_w, 2r·tile_h]`, the size the ticket specified, and this
  ADR recorded the `√2` gap against the exact projection as an accepted margin.
  That judgement was wrong: the projection `[[tw/2, -tw/2], [th/2, th/2]]` maps
  a radius-`r` circle to the **axis-aligned** screen ellipse with semi-axes
  `r·tw/√2` and `r·th/√2` (`M·Mᵀ` is the diagonal `[[tw²/2, 0], [0, th²/2]]`, so
  no shear survives to tilt it), and the shipped quad was that ellipse's
  bounding box under the shear rather than its image. The consequence was not a
  cosmetic margin: two agents at exactly contact distance rendered with
  **overlapping** rings instead of tangent ones, so the overlay could not be
  used to judge contact — the one question it exists to answer. T10 divides both
  axes by `√2`; for the tracked scenes (`r = 6`, `tw = 8`, `th = 4`) the quad is
  `67.9 × 33.9 px`, not `96 × 48`. The quad stays axis-aligned and **the shader
  is unchanged**. The regression guard is
  `the_rings_of_two_touching_bodies_are_tangent`, which asserts the geometric
  property rather than re-deriving the formula, so the two cannot drift together
  again. No state hash moved: the ring is drawn from simulation state and never
  feeds back into it (`864147ca…1ee881` reproduced).
- **Known gap, not fixable inside this plan.** Nothing binds the checked-in
  `.spv` blobs to `shaders/sprite.hlsl`. `xtask shaders --check` verifies
  recorded hashes only, so a future edit could re-pin `canonical_sha256`
  against stale blobs and pass every offline gate. The repo also carries no
  GLSL mirror of the shader despite building the Linux SPIR-V from one path.
  The canonical hash is pinned in five manifests — `shaders/generated/manifest.json`,
  the three `lab/goldens/*/manifest.json`, and
  `lab/fixtures/{windows,macos}-candidate/golden/manifest.json` — and
  `every_tracked_manifest_pins_the_live_shader_and_atlas` enforces all five on
  every host, but pinning agreement is not the same guarantee as pinning
  correctness against source.
- **Native blob debt.** The DXIL and metallib slots referenced by the Windows
  and macOS manifests are deferred placeholders on this (Linux) host. The
  Windows and macOS reference hosts owe a native shader rebuild before their
  backends can run what this ADR describes.
- **The frozen phase-0 bench ladder is no longer comparable at any rung.** The
  measured frame changed four ways at once in this plan — population, body
  size, projection, and an extra render pipeline for the ring — so no rung of
  the ladder frozen at phase-0 close means what it used to mean. This is a
  statement about comparability, not a number; the ladder stays frozen and
  non-gating regardless. Separately, `cargo run -- bench` run **without**
  `--test-policy` now hits this ceiling at its second tier, because
  `BenchPolicy::production()` still walks the frozen phase-0 ladder while
  `BenchPolicy::test_short()` was moved onto its own `test-short-v1` ladder.
  The perf tool is retired and non-gating, so this was left rather than
  redesigned.
- **The merge gate's pixel limb is tautological.** The Ubuntu candidate
  fixture the merge gate diffs against is a byte-copy of its own golden, so
  that comparison currently proves the gate can compare two identical images,
  not that the render is correct on that host. Recorded as a known gap, not
  fixed here.
- **No claim of speed is made here, by anyone, about this engine.** Nothing
  above has been measured on this code; performance measurement is retired for
  phase 0 and no number here gates a merge.
