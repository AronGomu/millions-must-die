# ADR 004: SDL3 Sprite Renderer + Assets

- Status: Accepted
- Date: 2026-08-02
- Superseded by: —

## Context

Prototype needs explicit batched GPU path across Vulkan, D3D12, Metal. Rust `sdl3` wrapper remains incomplete in places. Backend shader formats differ.

## Decision

Renderer:

- SDL3 GPU API through safe `sdl3` wrappers.
- `sdl3-sys` only for verified wrapper gaps; isolated module + documented safety invariants.
- Platform backend asserted: Vulkan/Linux, D3D12/Windows, Metal/macOS.
- Frames-in-flight compact instance buffers.
- 50k instances grouped into 4 atlas draws.
- 3×3 px sprites; premultiplied alpha.
- Fixed atlas order; no per-frame depth sort.
- Offscreen 1920×1080 perf target.
- Separate visible swapchain smoke.

Assets:

- 4 deterministic generated PNG atlases.
- 8 directions × 4 animation frames.
- Even agent distribution.
- Generator + hashes tracked.

Shaders/native deps:

- Canonical HLSL.
- Offline SPIR-V, DXIL, metallib tracked + hash/reflection checks.
- SDL3 source release pinned; shared native libs built/cached per OS.
- No runtime shader compiler dependency.

## Consequences

Positive:

- One renderer architecture across 3 native APIs.
- Representative atlas/UV/animation data.
- Minimal draw/state churn.
- Contributors can build from tracked shader blobs.

Negative:

- Wrapper gaps may require unsafe FFI.
- Generated binary blobs live in Git.
- 4 atlas draws do not prove future large-material batching.
- No Y-sort; overlap ordering stays fixed.

## Rejected alternatives

- SDL Renderer API: less explicit buffer/batch control.
- `wgpu`: diverges from selected SDL3 GPU direction.
- Raw `sdl3-sys` everywhere: excessive unsafe ownership burden.
- Runtime shadercross: extra shared deps + wrapper gaps.
- Production art: irrelevant proof cost.

## Validation

- Native backend assertion + software renderer rejection.
- Offscreen readback/golden per backend.
- Visible native swapchain smoke.
- Tracked shader/atlas clean regeneration.
- RenderDoc Linux/Windows; Xcode Metal diagnosis captures.

## References

- `docs/01-technical-architecture.md`
- `docs/sprite-renderer-architecture.html`
- `.tmp/IMPLEMENTATION_PLAN_technical_prototype.md` T3, T6–T10
