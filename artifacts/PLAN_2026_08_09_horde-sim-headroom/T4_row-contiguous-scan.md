# T4: Row-contiguous neighbour scan

**Plan:** `./artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** T2
**Commit outcome:** The 3×3 neighbour window is walked as three contiguous runs
instead of nine slice fetches, and an agent whose window holds only itself skips
the scan entirely. Every state hash is bit-identical.

## Context (self-contained)

- Goal: buy simulation headroom in the per-agent neighbour scan
  (`crates/mmd-engine/src/sim/collision.rs`) without spending a behavioural
  guarantee. Five changes land over T2–T6.
- This slice: the *shape* of the scan. Backlog item 9 in the research dossier
  proposed Ericson's min-corner binning to cut a 3×3 window to 2×2.
  **That does not transfer to this codebase** and must not be implemented — see
  the fence below. The transferable half is delivered instead: bin linear index
  is `bx + by * cols` and the counting sort lays buckets out in ascending bin
  index, so the three bins of one row are *already* a single contiguous run in
  `items`. Fetching them as one slice yields the identical agents in the
  identical order.
- Out of scope here — and this is a hard fence, not a preference:
  - **Do not implement min-corner binning.** Ericson's 2×2 window works because
    each *pair* is enumerated once and contributes to both members.
    `accumulate_separation` is a **gather**: for agent `i` it must see every
    neighbour `j`, and a 2×2 window anchored at `i`'s min-corner bin misses
    every `j` whose bin is one lower on either axis. Converting to a symmetric
    scatter would write `sep[j]` from agent `i`'s iteration, which destroys the
    index-disjointness T6 depends on and contradicts the decision recorded in
    `collision.rs` that a cap-truncated pair need not cancel.
  - Do not change `bin_of`, `bin_size_cells`, the neighbour cap, the coincidence
    tie-break, or the linear falloff.
  - No `sim/tick.rs`, no scenario file, no sidecar, no renderer, no shader, no
    flow field.
  - **No performance number in any doc, test name or commit message.** Perf
    gating is retired; this ticket is accepted on bit-exactness alone.
- Assumptions in force:
  - `starts` is fully populated for **every** bin, empty ones included, so an
    empty bin contributes a zero-width span inside a row slice. T2 guarantees
    this.
  - Iteration order must not change. Bit-exactness is the whole acceptance
    criterion, and the pinned digest is how it is checked.

## Requirements

- `SpatialGrid` exposes a row window: the agents in bins `bx0..=bx1` of row
  `by`, as one slice, in bin-ascending then index-ascending order.
- The row window is empty for an out-of-range row, and clamps `bx1` to the last
  column.
- `accumulate_separation_phase` fetches three row slices instead of nine bin
  slices, and its `break 'scan` at the neighbour cap still works.
- An agent whose entire 3×3 window holds one agent (itself) writes
  `sep_x[i] = 0.0; sep_y[i] = 0.0;` and moves on without touching `items`.
- `BODIED_STACK_HASH` does not move.

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

- `crates/mmd-engine/src/sim/spatial.rs` — `agents_in_bin` (line ~131), the
  `cols`, `rows`, `starts`, `items` fields.
- `crates/mmd-engine/src/sim/collision.rs` — the scan inside
  `accumulate_separation_phase`. Its current shape is:
  ```rust
  'scan: for cy in by0..=by1 {
      for cx in bx0..=bx1 {
          for &raw in grid.agents_in_bin(cx, cy) {
              let j = raw as usize;
              if j == i { continue; }
              // … distance test, coincidence tie-break, linear falloff …
              taken += 1;
              if taken == MAX_SEPARATION_NEIGHBORS { break 'scan; }
          }
      }
  }
  ```
  with the window computed just above as
  ```rust
  let (bx, by) = grid.bin_of(px, py);
  let bx0 = bx.saturating_sub(1);
  let bx1 = (bx + 1).min(last_col);
  let by0 = by.saturating_sub(1);
  let by1 = (by + 1).min(last_row);
  ```
- `crates/mmd-engine/tests/separation.rs` — spatial tests at the top of the file;
  `a_bodied_scenario_is_pinned_to_a_golden_digest` (line ~498).
- **From Depends (T2, with T1 and T3 also landed):**
  - `SpatialGrid` now carries `counts: Vec<u32>`, `count_stamp: Vec<u32>` and
    `stamp: u32`. `rebuild` no longer calls `starts.fill(0)`, but **still writes
    `starts[b]` for every bin `b` and `starts[n_bins]` as the total** — a row
    slice may therefore span empty bins safely.
  - `SpatialGrid::bin_count(bx, by) -> u32` is O(1) and returns `0` for an
    out-of-range or empty bin.
  - `SpatialGrid::set_stamp_for_test(u32)` exists behind
    `#[cfg(feature = "testkit")]`.
  - The scan now lives in
    `pub fn accumulate_separation_phase(x, y, grid, radius_cells, phases, phase, sep_x, sep_y)`
    (T3), whose per-agent loop is
    `let step = phases as usize; let mut i = phase as usize; while i < n { … i += step; }`.
    `pub fn accumulate_separation(x, y, grid, radius_cells, sep_x, sep_y)` is a
    wrapper over it with `phases = 1, phase = 0`.
  - Pinned digest that must not move, in
    `crates/mmd-engine/tests/separation.rs`:
    `BODIED_STACK_HASH = "81f958301624ff2033e797032d7cff8fd59344036263fdc5c6e13f00b5c80e8b"`.

## TDD

1. **Red** — write `bin_row_matches_bin_by_bin_order`,
   `bin_row_clamps_to_the_last_column`, `bin_row_is_empty_off_the_grid` and
   `a_lone_agent_accumulates_no_repulsion`. The first three fail to compile (no
   `agents_in_bin_row`).
2. **Green** — add `agents_in_bin_row`, then rewrite the scan and add the
   early-out.
3. **Refactor** — none. The body of the inner loop is copied verbatim.

## Test plan

| Test | Input | Expect |
| ---- | ---- | ---- |
| `bin_row_matches_bin_by_bin_order` | 8×8 grid, bin 1.0, 16 agents scattered across several bins in one row | `grid.agents_in_bin_row(1, 3, 2).to_vec()` equals `[1, 2, 3].iter().flat_map(\|cx\| grid.agents_in_bin(*cx, 2)).copied().collect::<Vec<_>>()` |
| `bin_row_clamps_to_the_last_column` | same grid, `agents_in_bin_row(6, 99, 0)` | equals the concatenation of bins `(6, 0)` and `(7, 0)`; does not panic |
| `bin_row_is_empty_off_the_grid` | `agents_in_bin_row(0, 2, 99)` and `agents_in_bin_row(99, 100, 0)` | both `&[]` |
| `a_lone_agent_accumulates_no_repulsion` | 32×32 grid, `CollisionParams::from_q8(128, 256)`, two agents 20 cells apart; call `accumulate_separation` directly | `sep_x == [0.0, 0.0]` and `sep_y == [0.0, 0.0]` |
| `a_bodied_scenario_is_pinned_to_a_golden_digest` | unchanged | still green, unedited — **the bit-exactness proof** |
| `separation_of_a_pair_is_equal_and_opposite` | unchanged | still green, unedited |
| `separation_is_capped_at_eight_neighbours` | unchanged | still green, unedited — proves `break 'scan` survived |
| `separation_ignores_agents_beyond_contact` | unchanged | still green, unedited |
| `a_released_stack_spreads_apart` | unchanged | still green, unedited |

## Impl steps

- [x] 1. In `crates/mmd-engine/src/sim/spatial.rs`, add directly below
      `agents_in_bin`:
      ```rust
      /// Agents in bins `bx0..=bx1` of row `by`, as one contiguous slice.
      ///
      /// A bin's linear index is `bx + by * cols` and the counting sort lays
      /// buckets out in ascending linear index, so a row window is already a
      /// single run in `items`. The slice therefore yields exactly the agents a
      /// bin-by-bin walk would visit, in exactly that order — empty bins inside
      /// the window contribute a zero-width span and are invisible.
      ///
      /// Empty for a row off the grid or a window starting past the last
      /// column; `bx1` is clamped.
      pub fn agents_in_bin_row(&self, bx0: u32, bx1: u32, by: u32) -> &[u32] {
          if by >= self.rows || bx0 >= self.cols || bx1 < bx0 {
              return &[];
          }
          let hi = bx1.min(self.cols - 1);
          let row = by * self.cols;
          let lo_bin = (bx0 + row) as usize;
          let hi_bin = (hi + row) as usize;
          let start = self.starts[lo_bin] as usize;
          let end = self.starts[hi_bin + 1] as usize;
          &self.items[start..end]
      }
      ```
- [x] 2. In `crates/mmd-engine/src/sim/collision.rs`, inside
      `accumulate_separation_phase`, immediately after the four window bounds
      (`bx0`, `bx1`, `by0`, `by1`) are computed and **before** `let mut sx`,
      insert the early-out:
      ```rust
      // A window holding one agent holds only this one. The scan below would
      // find no `j` and write zeroes, so write them without touching `items`.
      let mut window = 0usize;
      for cy in by0..=by1 {
          window += grid.agents_in_bin_row(bx0, bx1, cy).len();
      }
      if window <= 1 {
          sep_x[i] = 0.0;
          sep_y[i] = 0.0;
          i += step;
          continue;
      }
      ```
      **Note the `i += step;` before `continue;`** — the per-agent loop is a
      `while`, not a `for`, so an early `continue` must advance the index itself
      or the pass hangs.
- [x] 3. Replace the nested bin walk with the row walk. The `'scan` label moves
      to the outer `for cy`, and the middle `for cx` disappears:
      ```rust
      'scan: for cy in by0..=by1 {
          for &raw in grid.agents_in_bin_row(bx0, bx1, cy) {
              let j = raw as usize;
              if j == i {
                  continue;
              }
              // … body unchanged, verbatim …
              taken += 1;
              if taken == MAX_SEPARATION_NEIGHBORS {
                  break 'scan;
              }
          }
      }
      ```
      Copy the body between `if j == i { continue; }` and
      `if taken == MAX_SEPARATION_NEIGHBORS` **character for character**. Any
      reordering there changes the digest.
- [x] 4. Update the determinism note in the `collision.rs` module doc: change
      *"the neighbour scan visits bins in a fixed order"* to *"the neighbour
      scan visits each row of the window as one contiguous run, in a fixed
      order,"* and keep the rest of the sentence.
- [x] 5. Add the four new tests from the test plan to
      `crates/mmd-engine/tests/separation.rs` — the three spatial ones beside
      `spatial_bin_counts_match_the_bucket_lengths`, and
      `a_lone_agent_accumulates_no_repulsion` beside
      `separation_ignores_agents_beyond_contact`.
- [x] 6. Run validation. If `a_bodied_scenario_is_pinned_to_a_golden_digest`
      moves, step 3 changed the visit order — do **not** re-measure the digest;
      diff the loop body against the original.

## Outputs

- Touched: `crates/mmd-engine/src/sim/spatial.rs`,
  `crates/mmd-engine/src/sim/collision.rs`,
  `crates/mmd-engine/tests/separation.rs`.
- Public API added: `SpatialGrid::agents_in_bin_row`.
- Behaviour change: none — bit-identical output for every input.
- Migration / config: none.

## Validation

- [x] `cargo fmt --all -- --check`
- [x] `cargo test --workspace --locked` — green
- [x] `cargo test -p mmd-engine --test separation` — green, with
      `a_bodied_scenario_is_pinned_to_a_golden_digest`,
      `separation_is_capped_at_eight_neighbours` and
      `separation_of_a_pair_is_equal_and_opposite` **unedited**
- [x] `cargo test -p mmd-engine --test frame_allocations` — green
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] `cargo run -- run --agents 5000 --frames 300` — exits 0
- [x] the gate smoke `hash=` equals the T0 pinned digest, byte for byte
- [x] `graphify update .` run (graph refresh; `graphify-out/` is gitignored)
- [x] `cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300` — exits 0
- [x] `nix flake check`
- [x] app functional — every scene loads, ticks and exits; nothing observable
      changed — verified via the two `cargo run -- run` invocations above
      (offscreen draw ok, clean exit, matching digests); no windowed/GUI
      manual pass performed — see manual checklist
- [x] commit msg draft: `refactor(sim): walk each neighbour row as one contiguous slice`
