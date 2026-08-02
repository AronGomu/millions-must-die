# macOS 15 bootstrap (Metal prototype path)

Phase-0 macOS ref: **macOS 15 arm64**, base **M4 Mac mini 16 GB**.

Backend contract: force SDL GPU driver **`metal`**. **Apple Silicon only** (`aarch64` / `arm64`). **No MoltenVK / Vulkan-on-macOS path**. No sim/runtime fork — same `run` binary as Linux/Windows.

> **Status:** Linux prep (backend selection, arm64 arch gate, MoltenVK reject, metallib load path, framework/dylib prefix search, bootstrap docs) is in-tree. **Native macOS validation is still required on a real M4 Mac mini** before T10 is done.

## Toolchain

| Tool | Notes |
| --- | --- |
| Rust | `rustup` + repo `rust-toolchain.toml` (1.95.0), target `aarch64-apple-darwin` |
| Xcode 16+ | Full app + **Metal toolchain** (`xcodebuild -version`, `xcrun metal`) |
| CMake | Homebrew or Xcode CLI tools |
| GPU | Apple M4 integrated Metal (no discrete adapter name required) |

Intel Mac / `x86_64-apple-darwin` **rejected** at compile time (`compile_error!`) and via `validate_macos_host_arch`.

## Native SDL3 shared / framework

Pins: `third_party/versions.toml` (`SDL 3.4.12`, crates `sdl3 0.18.4` / `sdl3-sys 0.6.7`).

Preferred: **shared dylib** install (matches Linux layout). Framework install also supported by `build.rs` when `SDL3.framework` appears under `prefix/lib` or `prefix/Frameworks`.

```bash
# Optional cache root (default $HOME/.cache/mmd/native)
export MMD_NATIVE_CACHE="${MMD_NATIVE_CACHE:-$HOME/.cache/mmd/native}"
rel=3.4.12
root="$MMD_NATIVE_CACHE/sdl3/$rel"
src="$root/src"
build="$root/build-macos"
prefix="$root/prefix-macos"

# Fetch once (verify sha256 from versions.toml), extract to $src
# Then:
cmake -S "$src" -B "$build" -DCMAKE_BUILD_TYPE=Release \
  -DSDL_SHARED=ON -DSDL_STATIC=OFF -DSDL_TESTS=OFF -DSDL_TEST_LIBRARY=OFF \
  -DCMAKE_INSTALL_PREFIX="$prefix" \
  -DCMAKE_OSX_ARCHITECTURES=arm64
cmake --build "$build" --config Release --parallel
cmake --install "$build" --prefix "$prefix"

export MMD_SDL3_PREFIX="$prefix"
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
# dylib rpath is injected by build.rs when prefix exists
```

Offline pin check (any host):

```bash
cargo run -p xtask -- bootstrap --check
```

## metallib native regen

Canonical source: `shaders/sprite.hlsl` (entries `VSMain` / `PSMain`).

Tracked placeholder today (`MMD_PLACEHOLDER_METALLIB_*`) satisfies hash checks with `deferred=T10`. On the M4 ref, replace with a real Apple Metal library:

```bash
# Example path via SDL_shadercross or DXC metal output + metallib pack.
# Goal: one sprite.metallib containing VSMain + PSMain (SDL GPU METALLIB format).
# Adjust tool flags to match Xcode Metal toolchain on the ref host.

# After producing shaders/generated/sprite.metallib:
#   formats.metallib.status = "tracked"
#   remove formats.metallib.deferred
# Update file sha256 in shaders/generated/manifest.json.

cargo run -p xtask -- shaders --check
```

Real metallib containers must start with magic **`MTLB`**. `xtask shaders --check` accepts either:

- placeholder + `deferred=T10`, or
- native MTLB blob + `status=tracked` and no `deferred`.

Engine load path: `ShaderFormat::METALLIB`, same library bytes for vert/frag stages, entry points `VSMain` / `PSMain`.

## Run / validate matrix (macOS arm64 host only)

```bash
export MMD_SDL3_PREFIX="${HOME}/.cache/mmd/native/sdl3/3.4.12/prefix-macos"
export PKG_CONFIG_PATH="$MMD_SDL3_PREFIX/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

uname -m   # must print arm64
cargo test --workspace --locked
cargo run -p xtask -- shaders --check
cargo run -p xtask -- bootstrap --check
cargo run -- run --agents 50000

# Expect stdout: backend=metal
# Manual: adapter is Apple GPU (M4…); never MoltenVK / vulkan
```

Unit coverage available on any host:

- `metal_backend_required_contract` / `rejects_non_arm64_manifest` / `rejects_moltenvk_adapter` in `mmd-engine` backend tests
- `metal_backend_required` / `rejects_non_arm64_manifest` in `gpu_smoke`

## Failure modes

| Symptom | Action |
| --- | --- |
| `wrong GPU backend: got "vulkan"` | Do not enable MoltenVK; code forces `metal` only |
| `rejected host arch: got "x86_64"` | Use Apple Silicon Mac mini; Intel/Rosetta host not in phase-0 matrix |
| `rejected GPU adapter: "...MoltenVK..."` | Remove Vulkan ICD overlays; Metal-only |
| Shader create fails on metallib | Placeholder still present — run native Metal regen + manifest update |
| Missing `libSDL3.dylib` / framework | Set `MMD_SDL3_PREFIX` to `prefix-macos`; rebuild SDL shared |

## Related

- ADR 006 — native platforms + ref HW
- `third_party/README.md` — cache layout
- Plan ticket **T10** — full acceptance still needs the matrix above on macOS 15 arm64 (M4 Mac mini)
