# Generated shader backend blobs

Canonical source: [`../sprite.hlsl`](../sprite.hlsl).

| Format | Files | Status |
| --- | --- | --- |
| SPIR-V | `sprite.vert.spv`, `sprite.frag.spv` | Generated on Linux via GLSL mirror + `glslc` (semantics match HLSL). Prefer DXC/SDL_shadercross when available. |
| DXIL | `sprite.vert.dxil`, `sprite.frag.dxil` | **Placeholder** (`MMD_PLACEHOLDER_DXIL_*`) until Windows DXC regen. Load path + `direct3d12` device hooks are in engine; replace blobs + drop `deferred` on ref PC (see `docs/platform/windows-bootstrap.md`). |
| metallib | `sprite.metallib` | **Placeholder** (`MMD_PLACEHOLDER_METALLIB_*`) until macOS Metal regen. Load path + `metal` device hooks are in engine; replace blob + drop `deferred` on M4 ref (see `docs/platform/macos-bootstrap.md`). |

Verify:

```sh
cargo run -p xtask -- shaders --check
```

Do not hand-edit hashes in `manifest.json`; change source/blobs then regenerate manifest fields via xtask or update hashes after intentional blob replacement.
