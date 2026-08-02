# Windows 11 bootstrap (D3D12 prototype path)

Phase-0 Windows ref: **Windows 11 25H2 x86_64**, matched **Ryzen 5 8600G + RX 6400 4 GB + 16 GB**.

Backend contract: force SDL GPU driver **`direct3d12`**. Reject **Microsoft Basic Render Driver** (WARP/software). No sim/runtime fork — same `run` binary as Linux.

> **Status:** Linux prep (backend selection, adapter reject, DXIL load path, build.rs prefix) is in-tree. **Native Windows validation is still required on a real ref PC** before T9 is done.

## Toolchain

| Tool | Notes |
| --- | --- |
| Rust | `rustup` + repo `rust-toolchain.toml` (1.95.0) |
| CMake + VS 2022 Build Tools | MSVC x64, Windows 11 SDK |
| DXC | DirectX Shader Compiler for HLSL → DXIL (`dxc.exe` on PATH) |
| GPU driver | AMD Adrenalin for RX 6400; pin in lab manifest later (T18+) |

## Native SDL3 shared lib

Pins: `third_party/versions.toml` (`SDL 3.4.12`, crates `sdl3 0.18.4` / `sdl3-sys 0.6.7`).

```powershell
# Optional cache root (default %USERPROFILE%\.cache\mmd\native)
$env:MMD_NATIVE_CACHE = "$env:USERPROFILE\.cache\mmd\native"
$rel = "3.4.12"
$root = Join-Path $env:MMD_NATIVE_CACHE "sdl3\$rel"
$src = Join-Path $root "src"
$build = Join-Path $root "build-windows"
$prefix = Join-Path $root "prefix-windows"

# Fetch once (verify sha256 from versions.toml), extract to $src
# Then:
cmake -S $src -B $build -G "Visual Studio 17 2022" -A x64 `
  -DSDL_SHARED=ON -DSDL_STATIC=OFF -DSDL_TESTS=OFF -DSDL_TEST_LIBRARY=OFF `
  -DCMAKE_INSTALL_PREFIX=$prefix
cmake --build $build --config Release --parallel
cmake --install $build --prefix $prefix --config Release

$env:MMD_SDL3_PREFIX = $prefix
$env:PATH = "$(Join-Path $prefix 'bin');$env:PATH"
# pkg-config optional on Windows; build.rs also searches prefix lib/bin
```

Offline pin check (any host):

```powershell
cargo run -p xtask -- bootstrap --check
```

## DXIL native regen

Canonical source: `shaders/sprite.hlsl` (entries `VSMain` / `PSMain`).

Tracked placeholders today (`MMD_PLACEHOLDER_DXIL_*`) satisfy hash checks with `deferred=T9`. On the Windows ref, replace with real DXIL:

```powershell
# Example DXC invocation (adjust includes/targets to match SDL GPU DXIL SM6.0 expectations)
dxc -T vs_6_0 -E VSMain -Fo shaders/generated/sprite.vert.dxil shaders/sprite.hlsl
dxc -T ps_6_0 -E PSMain -Fo shaders/generated/sprite.frag.dxil shaders/sprite.hlsl

# Recompute sha256 in shaders/generated/manifest.json:
#   formats.dxil.status = "tracked"
#   remove formats.dxil.deferred
# Update file sha256 fields for both blobs.

cargo run -p xtask -- shaders --check
```

Real DXIL containers must start with magic **`DXBC`**. `xtask shaders --check` accepts either:

- placeholder + `deferred=T9`, or
- native DXBC blobs + `status=tracked` and no `deferred`.

## Run / validate matrix (Windows host only)

```powershell
$env:MMD_SDL3_PREFIX = "$env:USERPROFILE\.cache\mmd\native\sdl3\3.4.12\prefix-windows"
$env:PATH = "$env:MMD_SDL3_PREFIX\bin;$env:PATH"

cargo test --workspace --locked
cargo run -p xtask -- shaders --check
cargo run -p xtask -- bootstrap --check
cargo run -- run --agents 50000

# Expect stdout: backend=direct3d12
# Manual: adapter must be RX 6400 (or ref discrete), never "Microsoft Basic Render Driver"
```

Unit coverage available on any host:

- `d3d12_backend_required_contract` / `rejects_basic_renderer` in `mmd-engine` backend tests
- `rejects_basic_renderer` in `gpu_smoke`

## Failure modes

| Symptom | Action |
| --- | --- |
| `wrong GPU backend: got "vulkan"` | Ensure create name forced to `direct3d12` (code path); do not accept Vulkan-on-Windows for phase-0 gate |
| `rejected GPU adapter: "...Basic Render Driver..."` | Install/enable RX 6400 driver; disable WARP-only sessions |
| Shader create fails on DXIL | Placeholders still present — run native DXC regen + manifest update |
| Missing `SDL3.dll` | Add `prefix-windows\bin` to `PATH` or copy beside the exe |

## Related

- ADR 006 — native platforms + ref HW
- `third_party/README.md` — cache layout
- Plan ticket **T9** — full acceptance still needs the matrix above on Windows 11 25H2
