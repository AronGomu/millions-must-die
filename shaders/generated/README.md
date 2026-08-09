# Generated shader backend blobs

Canonical source: [`../sprite.hlsl`](../sprite.hlsl).

| Format | Files | Status |
| --- | --- | --- |
| SPIR-V | `sprite.vert.spv`, `sprite.frag.spv` | Built on Linux from the tracked GLSL mirror in [`../glsl/`](../glsl) with `glslc` (semantics match the HLSL). Prefer DXC/SDL_shadercross when available. |
| DXIL | `sprite.vert.dxil`, `sprite.frag.dxil` | **Placeholder** (`MMD_PLACEHOLDER_DXIL_*`) until Windows DXC regen. Load path + `direct3d12` device hooks are in engine; replace blobs + drop `deferred` on ref PC (see `docs/platform/windows-bootstrap.md`). |
| metallib | `sprite.metallib` | **Placeholder** (`MMD_PLACEHOLDER_METALLIB_*`) until macOS Metal regen. Load path + `metal` device hooks are in engine; replace blob + drop `deferred` on M4 ref (see `docs/platform/macos-bootstrap.md`). |

Verify:

```sh
cargo run -p xtask -- shaders --check
```

Do not hand-edit hashes in `manifest.json`; change source/blobs then regenerate manifest fields via xtask or update hashes after intentional blob replacement.

## Rebuilding the SPIR-V

The Linux blobs are built from [`../glsl/`](../glsl), which is a hand-maintained
mirror of the canonical HLSL. Toolchain used for the tracked blobs:
**shaderc 2026.1 / glslang 16.2.0**, target SPIR-V 1.0 (glslc's default), no
extra flags:

```sh
nix shell nixpkgs#shaderc -c bash -c '
  glslc -fshader-stage=vertex   shaders/glsl/sprite.vert.glsl -o shaders/generated/sprite.vert.spv
  glslc -fshader-stage=fragment shaders/glsl/sprite.frag.glsl -o shaders/generated/sprite.frag.spv'
```

Then re-pin `canonical_sha256` here **and in the five other manifests** that
track it (`lab/goldens/*/manifest.json`,
`lab/fixtures/*-candidate/golden/manifest.json`) —
`every_tracked_manifest_pins_the_live_shader_and_atlas` and
`cargo test -p mmd-lab --test merge_gate` enforce all six.

**Known gap, deliberately not closed here.** Nothing in the repo proves the
`.spv` still corresponds to the HLSL: `xtask -- shaders --check` re-verifies
recorded hashes, it does not recompile. Two things follow. First, when you edit
`sprite.hlsl`, editing the mirror is not optional — the checks will happily pass
on a stale blob. Second, the mirror is *tracked* precisely so the blobs are
reproducible; before this it existed only in an untracked scratch directory, and
a lost blob was unrecoverable. A future ticket should add
`xtask shaders --regen` that rebuilds into a temp dir and byte-compares.

The mirror in this tree was reconstructed by disassembling the then-pinned blobs
and **proved to reproduce both of them byte for byte before any edit**, so the
lineage from the pre-T9 artifacts is verifiable rather than asserted.

**`shaders/glsl/` is the only mirror.** A working copy may still carry an
untracked `.tmp/shader_gen/` from before this directory existed — it predates the
hitbox-ring branch, cannot produce either tracked blob, and is hidden from `git
status` by a local `.git/info/exclude` entry, so it will not announce itself.
Do not edit it and do not build from it; delete it.
