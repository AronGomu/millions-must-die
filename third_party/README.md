# Third-party native pins

Normative pin file: [`versions.toml`](versions.toml).

## SDL3

| Item | Pin |
| --- | --- |
| Source release | **3.4.12** (`release-3.4.12`) |
| Source tarball SHA-256 | `f07b958a9ac5020fb7a44cadb957f658b2149c3c8abb4f63145fac9303249db7` |
| Rust crate `sdl3` | **0.18.4** |
| Rust crate `sdl3-sys` | **0.6.7** (`0.6.7+SDL-3.4.12`) |

Pairing verified 2026-08-02 against crates.io + SDL release assets. Safe GPU surface used by the prototype (`Device`, `ShaderBuilder`, `ShaderFormat::{SPIRV,DXIL,METALLIB}`, `TransferBuffer`, frames-in-flight fences) is present in `sdl3 0.18.4`.

### Cache (outside Git)

Native source/build/prefix trees are **not** committed.

1. Set `MMD_NATIVE_CACHE` to an absolute cache root, **or** default to `$HOME/.cache/mmd/native`.
2. Layout:

```text
$MMD_NATIVE_CACHE/sdl3/3.4.12/
  src/           # extracted SDL3-3.4.12
  build-linux/
  prefix-linux/
  build-windows/   # T9
  prefix-windows/  # T9
  build-macos/     # T10
  prefix-macos/    # T10
```

3. Bootstrap (fetch once when network allowed; never from Cargo `build.rs`):

```sh
cargo run -p xtask -- bootstrap
cargo run -p xtask -- bootstrap --check
```

`--check` validates pins + lockfile pairing only; it does not download.

Linux shared-lib build commands live under `[sdl3.build.linux]` in `versions.toml`. Windows/macOS command blocks are recorded; **native Windows build/DXIL regen still needs ref-host validation (T9)** and macOS/metallib remains **T10**. Windows operator steps: [`docs/platform/windows-bootstrap.md`](../docs/platform/windows-bootstrap.md).

### Offline Cargo

- No package `build.rs` in this repo fetches network.
- After `cargo fetch` (or a warm registry cache), builds run with `CARGO_NET_OFFLINE=true`.
- Prefer linking the pinned shared lib from `prefix-<os>` via `PKG_CONFIG_PATH` (T7+). Do **not** enable `sdl3` `build-from-source` in CI/candidate gates (network + non-pinned).

### License

SDL3 retains zlib license terms from upstream. Project-authored files stay MIT-0.

## T7 runtime (Linux)

Link against pinned prefix:

```sh
export MMD_SDL3_PREFIX="${MMD_NATIVE_CACHE:-$HOME/.cache/mmd/native}/sdl3/3.4.12/prefix-linux"
export PKG_CONFIG_PATH="$MMD_SDL3_PREFIX/lib64/pkgconfig:${PKG_CONFIG_PATH:-}"
export LD_LIBRARY_PATH="$MMD_SDL3_PREFIX/lib64:/run/opengl-driver/lib:$(nix-build '<nixpkgs>' -A vulkan-loader --no-out-link)/lib:${LD_LIBRARY_PATH:-}"
# Headless / no usable DISPLAY:
export SDL_VIDEODRIVER=offscreen
cargo test -p mmd-engine --test gpu_smoke -- --ignored --test-threads=1
cargo run -- run
```

`build.rs` auto-adds rpath (Linux/macOS) or lib/bin link search (Windows) for the pinned `prefix-<os>` when present.

## T9 runtime (Windows)

See [`docs/platform/windows-bootstrap.md`](../docs/platform/windows-bootstrap.md).

```powershell
$env:MMD_SDL3_PREFIX = "$env:USERPROFILE\.cache\mmd\native\sdl3\3.4.12\prefix-windows"
$env:PATH = "$env:MMD_SDL3_PREFIX\bin;$env:PATH"
cargo test --workspace --locked
cargo run -- run --agents 50000
# expect backend=direct3d12; reject Basic Render Driver
```
