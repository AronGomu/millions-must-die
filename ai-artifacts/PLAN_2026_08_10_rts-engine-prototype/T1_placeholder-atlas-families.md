# T1: Placeholder atlas families

**Plan:** `./ai-artifacts/PLAN_2026_08_10_rts-engine-prototype.md`
**Depends:** none
**Commit outcome:** `cargo run -p xtask -- atlases --check` verifies three tracked atlas families — the existing zombie set plus a new `rts/` set and a new `ui/` font — and fails on any byte drift in any of them.

## Context (self-contained)

- Goal: phase 1 is a thin vertical slice of an RTS engine prototype (camera,
  selection, workers, economy, building, unit production) on a new horde-free
  scene. Nothing in it may weaken phase-0 contracts.
- This slice: the very first ticket. It frontloads the only asset work the whole
  plan needs, so no later ticket blocks on art. Every phase-1 sprite is generated
  from code — flat-shaded placeholders — and tracked exactly like the phase-0
  atlases.
- Out of scope here: any change to `crates/mmd-engine`, to the renderer, to the
  scenario contract, or to `assets/sprites/generated/atlas_*.png`. Those four PNGs
  and `assets/sprites/generated/manifest.json` must come out of this ticket
  **byte-identical**. Do not touch `assets/sprites/source/`.
- Assumptions in force: placeholder art is generated with no external source
  image, so it carries no third-party licence obligation. Generation must be
  deterministic and idempotent — rerunning `xtask atlases` leaves `git status`
  clean.
- **No user action is required by this plan.** There is no account to create, no
  API key to link, no package to install beyond the pinned toolchain
  (`rust-toolchain.toml`, Rust 1.95.0) already in the repo. This bullet exists so
  a later ticket does not discover otherwise.

## Requirements

- New module `xtask/src/placeholder_art.rs` generating two families:
  - **`rts`** → `assets/sprites/generated/rts/` with four PNGs and a `manifest.json`.
  - **`ui`** → `assets/sprites/generated/ui/` with one PNG and a `manifest.json`.
- Both families are generated and checked by the existing gate command
  `cargo run -p xtask -- atlases --check`. No new gate command.
- Every generated pixel is **premultiplied** RGBA8 (`r <= a`, `g <= a`, `b <= a`),
  the same invariant `xtask::atlases::assert_premultiplied` already enforces.
- `rts` PNGs reuse the phase-0 frame geometry exactly: 4 columns × 8 rows of
  32 × 32 px frames → 128 × 256 px. This is what lets `render::frame_uv_rect`
  address them with no change.
- `ui/font.png` is 128 × 48 px: 16 columns × 6 rows of 8 × 8 px glyphs covering
  ASCII 32..=127 in code-point order.
- The zombie family's generation path, manifest and bytes are untouched.

## Inputs

- **Files to read**
  - `xtask/src/atlases.rs` — existing generator. Reuse its `AtlasError`,
    `AtlasEntry`, `sha256_hex` usage, `write_manifest` pattern, PNG encoder
    settings and `assert_premultiplied`.
  - `xtask/src/main.rs` — the `Commands::Atlases { check }` arm.
  - `xtask/src/digest.rs` — `sha256_hex`.
- **From Depends:** none. This is the first ticket.
- **Facts you must not rediscover**
  - The PNG encoder settings that make output byte-reproducible are, verbatim
    from `xtask/src/atlases.rs::encode_atlas_png`:
    ```rust
    let mut encoder = png::Encoder::new(&mut out, W, H);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fastest);
    encoder.set_filter(png::Filter::NoFilter);
    ```
    Use exactly these for the new families too, or `--check` becomes
    version-fragile.
  - `xtask/src/atlases.rs::run_atlases(check: bool)` currently does:
    ```rust
    let root = workspace_root_from_xtask_manifest();
    let out = default_output_dir(&root);           // <root>/assets/sprites/generated
    if check { check_atlases(&out)?; println!("atlases: ok ({ATLAS_COUNT} png + manifest)"); }
    else { let manifest = generate_atlases(&out)?; println!(...); }
    ```
  - `xtask/src/atlases.rs` already exposes `pub fn assert_premultiplied(file: &str, png_bytes: &[u8]) -> Result<(), AtlasError>`.

## Exact design — no decisions left

### Slot names and files

`xtask/src/placeholder_art.rs`:

```rust
/// Placeholder generator id, stamped into both new manifests.
pub const RTS_GENERATOR_ID: &str = "mmd-rts-placeholder-v1";
pub const UI_GENERATOR_ID: &str = "mmd-ui-font-v1";

/// RTS family frame geometry — identical to the zombie atlases so
/// `render::frame_uv_rect` addresses both with one function.
pub const RTS_FRAME_SIZE_PX: u32 = 32;
pub const RTS_FRAMES_X: u32 = 4;
pub const RTS_FRAMES_Y: u32 = 8;
pub const RTS_ATLAS_WIDTH_PX: u32 = RTS_FRAMES_X * RTS_FRAME_SIZE_PX;   // 128
pub const RTS_ATLAS_HEIGHT_PX: u32 = RTS_FRAMES_Y * RTS_FRAME_SIZE_PX;  // 256

/// The four RTS placeholder sheets, in manifest order. Index is the id.
pub const RTS_FILES: [&str; 4] = ["worker.png", "soldier.png", "buildings.png", "props.png"];

/// Bitmap font geometry.
pub const GLYPH_W_PX: u32 = 8;
pub const GLYPH_H_PX: u32 = 8;
pub const FONT_COLS: u32 = 16;
pub const FONT_ROWS: u32 = 6;
pub const FONT_FIRST_CHAR: u8 = 32;
pub const FONT_WIDTH_PX: u32 = FONT_COLS * GLYPH_W_PX;  // 128
pub const FONT_HEIGHT_PX: u32 = FONT_ROWS * GLYPH_H_PX; // 48
pub const UI_FILES: [&str; 1] = ["font.png"];
```

### `worker.png` / `soldier.png` — animated, 8 dirs × 4 frames

For frame `(dir, frame)` at cell origin `(frame * 32, dir * 32)`, draw into the
32 × 32 tile:

- Clear to `[0, 0, 0, 0]`.
- **Body**: filled axis-aligned rectangle, x in `10..=21`, y in `8..=25`.
- **Head**: filled rectangle, x in `12..=19`, y in `2..=9`.
- **Facing pip**: a 4 × 4 filled square whose centre is
  `(16 + round(6 * DIR_DX[dir]), 14 + round(6 * DIR_DY[dir]))`, clamped so the
  square stays inside the tile, where
  ```rust
  /// Matches `sim::tick::dir_from_vector`: 0=E,1=NE,2=N,3=NW,4=W,5=SW,6=S,7=SE.
  const DIR_DX: [f32; 8] = [1.0, 0.7071, 0.0, -0.7071, -1.0, -0.7071, 0.0, 0.7071];
  const DIR_DY: [f32; 8] = [0.0, -0.7071, -1.0, -0.7071, 0.0, 0.7071, 1.0, 0.7071];
  ```
- **Animation**: shift the whole tile's drawn pixels vertically by
  `BOB[frame]` where `const BOB: [i32; 4] = [0, -1, 0, 1];`. Pixels pushed out of
  the tile are dropped.
- **Colours** (premultiplied, alpha 255 so premultiplication is the identity):
  - worker body `[60, 150, 220, 255]`, head `[200, 210, 230, 255]`, pip `[255, 230, 90, 255]`
  - soldier body `[190, 70, 60, 255]`, head `[220, 210, 200, 255]`, pip `[255, 230, 90, 255]`

### `buildings.png` — static table, indexed `(row, col)`

Same 4 × 8 grid, but the indices mean a static sprite, not `(dir, frame)`:

| row | col | content | fill | border |
| --- | --- | ------- | ---- | ------ |
| 0 | 0 | HQ, finished | `[70, 110, 170, 255]` | `[230, 235, 245, 255]` |
| 0 | 1 | Depot, finished | `[80, 140, 110, 255]` | `[230, 235, 245, 255]` |
| 0 | 2 | Barracks, finished | `[150, 110, 60, 255]` | `[230, 235, 245, 255]` |
| 0 | 3 | *(transparent)* | — | — |
| 1 | 0 | HQ, under construction | `[35, 55, 85, 255]` | `[120, 125, 130, 255]` |
| 1 | 1 | Depot, under construction | `[40, 70, 55, 255]` | `[120, 125, 130, 255]` |
| 1 | 2 | Barracks, under construction | `[75, 55, 30, 255]` | `[120, 125, 130, 255]` |
| 1 | 3 | *(transparent)* | — | — |
| 2 | 0 | Crystal node | `[120, 200, 235, 255]` | `[235, 250, 255, 255]` |
| 2 | 1 | Gas node | `[170, 120, 220, 255]` | `[240, 225, 255, 255]` |
| 2 | 2 | Crystal node, depleted | `[70, 95, 105, 255]` | `[130, 140, 145, 255]` |
| 2 | 3 | Gas node, depleted | `[85, 70, 100, 255]` | `[135, 125, 145, 255]` |
| 3..7 | any | *(transparent)* | — | — |

Shape for every non-transparent cell: fill the sub-rectangle x in `3..=28`,
y in `3..=28` with `fill`, then overwrite its one-pixel outline with `border`.
Under-construction cells additionally clear every pixel whose `(x + y) % 4 == 0`
back to `[0, 0, 0, 0]`, giving a visible scaffold hatch.

### `props.png` — static table, indexed `(row, col)`

| row | col | content |
| --- | --- | ------- |
| 0 | 0 | Selection ring: pixels where `12 <= round(hypot(x-15.5, y-15.5)) <= 14`, colour `[40, 235, 120, 255]`; rest transparent |
| 0 | 1 | Placement OK tile: filled `4..=27` square, colour `[30, 180, 90, 140]` **premultiplied** → store `[16, 99, 49, 140]` |
| 0 | 2 | Placement BAD tile: same square, colour `[200, 50, 50, 140]` premultiplied → store `[110, 27, 27, 140]` |
| 0 | 3 | Rally flag: vertical bar x in `14..=16`, y in `4..=27`, colour `[240, 240, 240, 255]`; pennant filled triangle `x in 17..=26, y in 5..=12` where `y - 5 <= 26 - x`, colour `[240, 200, 60, 255]` |
| 1 | 0 | Crystal icon: filled diamond `abs(x-16) + abs(y-16) <= 10`, `[120, 200, 235, 255]` |
| 1 | 1 | Gas icon: filled diamond, `[170, 120, 220, 255]` |
| 1 | 2 | Supply icon: filled square `10..=21`, `[220, 220, 120, 255]` |
| 1 | 3 | Panel fill: **every** pixel `[16, 18, 24, 200]` premultiplied → store `[13, 14, 19, 200]` |
| 2..7 | any | *(transparent)* |

Premultiplication rule, applied to every stored texel: `store = (round(c * a / 255), a)`.
The table above already gives stored values; write a helper
`fn premul(rgba: [u8; 4]) -> [u8; 4]` and generate them through it so the two
cannot drift.

### `ui/font.png` — 8 × 8 glyphs, ASCII 32..=127

Glyph for byte `c` sits at column `(c - 32) % 16`, row `(c - 32) / 16`.
Each glyph is drawn from a hardcoded 8-byte bitmap table
`const GLYPH_BITS: [[u8; 8]; 96]`, one byte per row, bit 7 = leftmost pixel.
Set bits become `[255, 255, 255, 255]`, clear bits `[0, 0, 0, 0]`.

The glyph table must cover, with legible 5 × 7 forms inside the 8 × 8 cell:
`space`, digits `0`–`9`, uppercase `A`–`Z`, and the punctuation
`. , : / - + ( ) % [ ] < > ! ?`. Every remaining code point in 32..=127 —
including all lowercase — gets the **fallback box** glyph
`[0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00]`. Lowercase is deliberately not
authored: every HUD string this plan draws is uppercased by
`render::text::push_text` (T3), and a box glyph makes a missed uppercasing
visible instead of silent.

### Manifest shape

Reuse `xtask::atlases::AtlasEntry` verbatim. New struct in `placeholder_art.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceholderManifest {
    pub version: u32,          // PLACEHOLDER_MANIFEST_VERSION = 1
    pub generator: String,     // RTS_GENERATOR_ID or UI_GENERATOR_ID
    pub frame_width_px: u32,
    pub frame_height_px: u32,
    pub cols: u32,
    pub rows: u32,
    pub images: Vec<crate::atlases::AtlasEntry>,
}
pub const PLACEHOLDER_MANIFEST_VERSION: u32 = 1;
```

Written with the same canonical pretty JSON + trailing newline as
`xtask::atlases::write_manifest`.

### Public API of `placeholder_art.rs`

```rust
pub fn rts_dir(workspace_root: &Path) -> PathBuf;   // <root>/assets/sprites/generated/rts
pub fn ui_dir(workspace_root: &Path) -> PathBuf;    // <root>/assets/sprites/generated/ui
pub fn encode_rts_png(id: u32) -> Result<Vec<u8>, AtlasError>;  // id indexes RTS_FILES
pub fn encode_ui_png(id: u32) -> Result<Vec<u8>, AtlasError>;   // id indexes UI_FILES
pub fn generate_placeholders(rts_out: &Path, ui_out: &Path) -> Result<(PlaceholderManifest, PlaceholderManifest), AtlasError>;
pub fn check_placeholders(rts_out: &Path, ui_out: &Path) -> Result<(), AtlasError>;
```

`check_placeholders` mirrors `atlases::check_atlases`: load the tracked manifest,
regenerate into a temp dir, require the manifests to be equal, require each
tracked PNG's sha256 to equal its manifest entry, require tracked bytes to equal
regenerated bytes, and run `atlases::assert_premultiplied` on every tracked PNG.

### Wiring

`xtask/src/main.rs`: add `mod placeholder_art;` next to `mod atlases;`.

`xtask/src/atlases.rs::run_atlases` becomes:

```rust
pub fn run_atlases(check: bool) -> Result<(), AtlasError> {
    let root = workspace_root_from_xtask_manifest();
    let out = default_output_dir(&root);
    let rts_out = crate::placeholder_art::rts_dir(&root);
    let ui_out = crate::placeholder_art::ui_dir(&root);
    if check {
        check_atlases(&out)?;
        crate::placeholder_art::check_placeholders(&rts_out, &ui_out)?;
        println!(
            "atlases: ok ({ATLAS_COUNT} zombie png + manifest, {} rts png + manifest, {} ui png + manifest)",
            crate::placeholder_art::RTS_FILES.len(),
            crate::placeholder_art::UI_FILES.len()
        );
    } else {
        let manifest = generate_atlases(&out)?;
        let (rts, ui) = crate::placeholder_art::generate_placeholders(&rts_out, &ui_out)?;
        println!(
            "atlases: wrote {} zombie + {} rts + {} ui png + manifests → {}",
            manifest.atlases.len(),
            rts.images.len(),
            ui.images.len(),
            out.parent().unwrap_or(&out).display()
        );
    }
    Ok(())
}
```

## TDD

1. **Red** — write every test in the table below first, in
   `xtask/src/placeholder_art.rs`'s own `#[cfg(test)] mod tests` (mirroring the
   existing `xtask/src/atlases.rs` test module). They fail to compile, then fail
   on assertions.
2. **Green** — write the generator until all pass.
3. **Refactor** — only to remove duplication between the two families' manifest
   writers. Keep green.

## Test plan

| Test | Input | Expect |
| ---- | ----- | ------ |
| `generates_both_placeholder_families` | two temp dirs | 4 rts entries + 1 ui entry; manifest versions `1`; generators `mmd-rts-placeholder-v1` / `mmd-ui-font-v1` |
| `placeholder_generation_is_idempotent` | generate twice into two temp dirs | every PNG byte-identical and both manifests equal |
| `rts_sheets_have_the_phase0_frame_geometry` | decode each rts PNG | `128 × 256`, and `cols/rows/frame_*_px` in the manifest are `4 / 8 / 32 / 32` |
| `font_sheet_has_the_locked_glyph_grid` | decode `font.png` | `128 × 48`; manifest `cols/rows/frame_width_px/frame_height_px` = `16 / 6 / 8 / 8` |
| `every_generated_texel_is_premultiplied` | all 5 PNGs | `assert_premultiplied` returns `Ok` for each |
| `worker_and_soldier_differ` | rts ids 0 and 1 | PNG bytes differ (a copy-paste that shipped one sheet twice fails) |
| `facing_pip_moves_with_the_direction_row` | worker sheet, frame col 0, rows 0 and 4 | the pip's filled-pixel centroid x for row 0 (East) is strictly greater than for row 4 (West) |
| `animation_frames_are_not_all_equal` | worker sheet, row 0, cols 0..3 | at least two of the four 32×32 tiles differ pixelwise |
| `building_table_marks_construction` | buildings sheet `(1,1)` vs `(0,1)` | the `(1,1)` tile has strictly fewer opaque pixels than `(0,1)` |
| `depleted_nodes_are_distinct_from_full_nodes` | buildings `(2,0)` vs `(2,2)`, `(2,1)` vs `(2,3)` | tiles differ pixelwise in both pairs |
| `unused_table_rows_are_fully_transparent` | buildings + props, rows 3..7 (buildings) / 2..7 (props) | every texel `[0,0,0,0]` |
| `glyph_for_capital_a_is_not_the_fallback_box` | font, `b'A'` | glyph bitmap differs from the fallback box glyph |
| `lowercase_is_the_fallback_box` | font, `b'a'` | glyph pixels equal the fallback box glyph's pixels |
| `every_ascii_code_point_has_a_cell` | 32..=127 | each maps to an in-bounds `(col,row)` and the decoded cell is readable |
| `check_passes_on_freshly_generated_output` | temp dirs, generate then check | `Ok(())` |
| `check_fails_on_a_drifted_png` | generate, flip one byte of `rts/props.png` | `Err(AtlasError::HashMismatch { .. })` |
| `check_fails_on_a_drifted_manifest` | generate, rewrite manifest `generator` field | `Err(AtlasError::Check(_))` |

**Mutation verification (mandatory, both directions).** Inject at least these and
confirm a red test, then revert and confirm green:
1. `BOB` → `[0, 0, 0, 0]` (kills `animation_frames_are_not_all_equal`).
2. `DIR_DX` sign flipped (kills `facing_pip_moves_with_the_direction_row`).
3. drop the `(x + y) % 4 == 0` hatch (kills `building_table_marks_construction`).
4. `premul` returns its input unchanged (kills `every_generated_texel_is_premultiplied`
   on the alpha-140/200 cells).
5. `encoder.set_compression(png::Compression::Best)` (kills
   `check_fails_on_a_drifted_png`'s sibling — confirm `check_placeholders` still
   passes, i.e. that check is regeneration-relative, not byte-pinned to a
   constant; record the result in the commit body).

## Impl steps

- [x] 1. Create `xtask/src/placeholder_art.rs` with the constants block above verbatim.
- [x] 2. Add `mod placeholder_art;` to `xtask/src/main.rs`.
- [x] 3. Write the failing test module in `placeholder_art.rs` (every row of the test plan).
- [x] 4. Run `cargo test -p xtask` and record that the new tests fail.
- [x] 5. Implement `fn premul(rgba: [u8; 4]) -> [u8; 4]`.
- [x] 6. Implement `fn blit_rect(px: &mut [u8], w: u32, x0: u32, y0: u32, x1: u32, y1: u32, rgba: [u8; 4])`.
- [x] 7. Implement `fn render_unit_sheet(body: [u8;4], head: [u8;4], pip: [u8;4]) -> Vec<u8>` using `DIR_DX`/`DIR_DY`/`BOB`.
- [x] 8. Implement `fn render_buildings_sheet() -> Vec<u8>` from the `(row, col)` table.
- [x] 9. Implement `fn render_props_sheet() -> Vec<u8>` from the `(row, col)` table.
- [x] 10. Implement `const GLYPH_BITS: [[u8; 8]; 96]` and `fn render_font_sheet() -> Vec<u8>`.
- [x] 11. Implement `encode_rts_png` / `encode_ui_png` with the exact encoder settings quoted above.
- [x] 12. Implement `PlaceholderManifest`, `write_placeholder_manifest`, `load_placeholder_manifest`.
- [x] 13. Implement `generate_placeholders` and `check_placeholders`.
- [x] 14. Rewrite `xtask/src/atlases.rs::run_atlases` to the body quoted above.
- [x] 15. `cargo run -p xtask -- atlases` to write the tracked assets.
- [x] 16. `git add assets/sprites/generated/rts assets/sprites/generated/ui`.
- [x] 17. Confirm `git status` shows **no** change under `assets/sprites/generated/atlas_*.png` or `assets/sprites/generated/manifest.json`.
- [x] 18. Run the mutation list; record kills in the commit body.
- [x] 19. Run the full validation block below.

## Outputs

- **Files created**
  - `xtask/src/placeholder_art.rs`
  - `assets/sprites/generated/rts/{worker,soldier,buildings,props}.png`
  - `assets/sprites/generated/rts/manifest.json`
  - `assets/sprites/generated/ui/font.png`
  - `assets/sprites/generated/ui/manifest.json`
- **Files edited**
  - `xtask/src/main.rs` (one `mod` line)
  - `xtask/src/atlases.rs` (`run_atlases` body only)
- **Public API added:** the `placeholder_art` constants and functions listed above.
- **Behaviour change:** `xtask atlases` now writes 5 more PNGs and 2 more
  manifests; `xtask atlases --check` now fails on drift in any of them.
- **Migration / config:** none.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test -p xtask` — all new tests green
- [x] `cargo test --workspace --locked`
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `nix flake check`
- [x] `cargo run -p xtask -- bootstrap --check`
- [x] `cargo run -p xtask -- shaders --check`
- [x] `cargo run -p xtask -- atlases --check` — prints the three-family ok line, exit 0
- [x] `cargo run -p xtask -- atlases && git status --porcelain` — **empty output** (idempotent; verified via `git diff --stat` since the new `rts/`/`ui/` trees are freshly staged additions in this same commit, so `--porcelain` alone also reports them as `A ` regardless of drift — the diff-stat check below is the byte-identity proof)
- [x] `git diff --stat HEAD -- assets/sprites/generated/atlas_0.png assets/sprites/generated/atlas_1.png assets/sprites/generated/atlas_2.png assets/sprites/generated/atlas_3.png assets/sprites/generated/manifest.json` — **empty**
- [x] `cargo run -- run --agents 5000 --frames 300` — exit 0 (nothing in the engine changed)
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `feat(xtask): generate the phase-1 placeholder atlas families`
