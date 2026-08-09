# T2: Stamped bin counts

**Plan:** `./ai-artifacts/PLAN_2026_08_09_horde-sim-headroom.md`
**Depends:** T1
**Commit outcome:** `SpatialGrid::rebuild` no longer clears `starts` before
counting, and bin population is an O(1) query. Bucket contents, bucket order and
every state hash are bit-identical.

## Context (self-contained)

- Goal: buy simulation headroom in the per-agent neighbour scan
  (`crates/mmd-engine/src/sim/collision.rs`) without spending a behavioural
  guarantee. Five changes land over T2–T6.
- This slice: the neighbour index itself. Replace the per-rebuild
  `self.starts.fill(0)` pass with stamped counts, and expose the bin population
  that T4's early-out needs. **Behaviour is bit-identical**; the proof is that
  `BODIED_STACK_HASH` does not move.
- Out of scope here: `sim/tick.rs`, `sim/collision.rs`, `sim/agents.rs`,
  scenario files, sidecars. No renderer, shader or flow field. **No performance
  number in any doc, test name or commit message** — perf gating is retired and
  this ticket is accepted on bit-exactness and zero allocation, not on speed.
- Assumptions in force:
  - The prefix-sum pass over bins is **structural** to a counting sort and stays.
    Only the `fill(0)` pass goes. Reaching an O(#agents) rebuild would need a
    per-bin linked list (destroying the bucket contiguity T4 depends on) or a
    per-tick sort of touched bins; both are rejected, and perf is frozen so
    neither could be settled by measurement anyway.
  - `starts` must remain **fully populated for every bin**, including empty
    ones, because T4 reads `starts[b]` and `starts[b + 1]` for a bin that may be
    empty.
  - Buckets must stay sorted by ascending agent index. That property, not mere
    correctness, is what makes the neighbour scan reproducible across processes.

## Requirements

- `SpatialGrid::rebuild` allocates nothing (already enforced by
  `spatial_rebuild_allocates_nothing` in
  `crates/mmd-engine/tests/frame_allocations.rs`).
- `rebuild` performs no `fill` over `starts`.
- After `rebuild`, for every bin: `agents_in_bin(bx, by)` returns exactly the
  agents whose position bins there, ascending by index — unchanged from today.
- A bin that held agents on rebuild *n* and none on rebuild *n+1* reports empty.
  No ghosts.
- The stamp counter wraps safely.
- New public query `bin_count(bx, by) -> u32`, O(1), agreeing with
  `agents_in_bin(bx, by).len()` for every bin.

## Inputs

- `crates/mmd-engine/src/sim/spatial.rs` — the whole file. Current `rebuild`
  (line ~98) is:
  ```rust
  self.starts.fill(0);
  for i in 0..self.len {
      let (bx, by) = self.bin_of(x[i], y[i]);
      let b = (bx + by * self.cols) as usize;
      self.starts[b + 1] += 1;
  }
  for b in 0..self.cursor.len() {
      self.starts[b + 1] += self.starts[b];
      self.cursor[b] = self.starts[b];
  }
  for i in 0..self.len {
      let (bx, by) = self.bin_of(x[i], y[i]);
      let b = (bx + by * self.cols) as usize;
      let slot = self.cursor[b] as usize;
      self.items[slot] = i as u32;
      self.cursor[b] += 1;
  }
  ```
- `crates/mmd-engine/tests/separation.rs` — spatial tests live at the top of the
  file (`spatial_bins_hold_every_agent_exactly_once`,
  `spatial_bucket_order_is_ascending_agent_index`,
  `spatial_rebuild_is_repeatable`, …). Add the new ones beside them.
- `crates/mmd-engine/tests/frame_allocations.rs` —
  `spatial_rebuild_allocates_nothing` (line ~197) must stay green.
- **From Depends (T1):** T1 added `separation_phases`, `mass_class_count` and
  `separation_threads` to the scenario contract and to
  `CollisionParams { radius_cells, strength, phases, mass_classes, threads }`,
  all at identity `1`. **T2 reads none of them.** T1 also left these digests
  pinned and unchanged, and this ticket must keep them that way:
  - `BODIED_STACK_HASH = "81f958301624ff2033e797032d7cff8fd59344036263fdc5c6e13f00b5c80e8b"`
    in `crates/mmd-engine/tests/separation.rs`
  - `BODYLESS_GRID_PRE_SEPARATION_HASH` in the same file.

## TDD

1. **Red** — add `spatial_bin_counts_match_the_bucket_lengths`,
   `spatial_reuses_bins_without_clearing_them` and
   `spatial_survives_a_stamp_wrap` to
   `crates/mmd-engine/tests/separation.rs`. The first and third fail to compile
   (no `bin_count`, no `set_stamp_for_test`); the second fails or passes
   vacuously against today's code — write it anyway, it is the ghost-bin guard.
2. **Green** — add `counts`, `count_stamp` and `stamp` to `SpatialGrid`; rewrite
   `rebuild`; add `bin_count`; add the testkit-gated stamp setter.
3. **Refactor** — none. Do not change `bin_of`, `axis_bin`, `agents_in_bin` or
   `new`'s clamping rule.

## Test plan

| Test | Input | Expect |
| ---- | ---- | ---- |
| `spatial_bin_counts_match_the_bucket_lengths` | 8×8 grid, bin 1.0, 5 agents at the positions used by `spatial_bins_hold_every_agent_exactly_once` | for every `(bx, by)` in `0..cols × 0..rows`: `grid.bin_count(bx, by) as usize == grid.agents_in_bin(bx, by).len()` |
| `spatial_reuses_bins_without_clearing_them` | rebuild with all 4 agents at `(0.5, 0.5)`, then rebuild with all 4 at `(7.5, 7.5)` | `bin_count(0, 0) == 0`, `bin_count(7, 7) == 4`, and the total over all bins `== grid.len()` |
| `spatial_survives_a_stamp_wrap` | `set_stamp_for_test(u32::MAX)`, then two rebuilds with different positions | both rebuilds give the same buckets a fresh grid would; total over all bins `== grid.len()` on each |
| `spatial_bins_hold_every_agent_exactly_once` | unchanged | still green, unedited |
| `spatial_bucket_order_is_ascending_agent_index` | unchanged | still green, unedited |
| `spatial_rebuild_is_repeatable` | unchanged | still green, unedited |
| `spatial_rebuild_allocates_nothing` | unchanged (`frame_allocations.rs`) | still green, unedited |
| `a_bodied_scenario_is_pinned_to_a_golden_digest` | unchanged (`separation.rs`) | still green, unedited — **this is the bit-exactness proof** |

## Impl steps

- [ ] 1. In `crates/mmd-engine/src/sim/spatial.rs`, add three fields to
      `struct SpatialGrid`, after `cursor`:
      ```rust
      /// Per-bin population for the rebuild identified by `stamp`. A bin whose
      /// `count_stamp` differs is empty; its `counts` entry is stale garbage
      /// and must never be read.
      counts: Vec<u32>,
      /// Rebuild that last wrote `counts[b]`; length `cols * rows`.
      count_stamp: Vec<u32>,
      /// Monotone rebuild counter. Starts at 0 so the first rebuild's stamp of
      /// 1 marks every zero-initialised `count_stamp` entry stale.
      stamp: u32,
      ```
- [ ] 2. Initialise them in `SpatialGrid::new`: `counts: vec![0; n_bins]`,
      `count_stamp: vec![0; n_bins]`, `stamp: 0`.
- [ ] 3. Replace the body of `rebuild` after the two `assert!`s and
      `self.len = x.len();` with exactly:
      ```rust
      // Bump first: a stamp of `self.stamp` on a bin means "written this
      // rebuild". `count_stamp` is zero-initialised, so the first rebuild's
      // stamp of 1 correctly marks every bin stale.
      self.stamp = self.stamp.wrapping_add(1);
      if self.stamp == 0 {
          // Wrapped. Only reachable after u32::MAX rebuilds, but a stale bin
          // that happened to hold stamp 0 would resurrect its old population,
          // so pay the one full clear rather than carry the hazard.
          self.count_stamp.fill(0);
          self.stamp = 1;
      }
      let stamp = self.stamp;

      for i in 0..self.len {
          let (bx, by) = self.bin_of(x[i], y[i]);
          let b = (bx + by * self.cols) as usize;
          if self.count_stamp[b] != stamp {
              self.count_stamp[b] = stamp;
              self.counts[b] = 0;
          }
          self.counts[b] += 1;
      }

      // One pass, not two: the old code cleared `starts` and then prefix-summed
      // it. A stale bin reads as zero here, so the clear is gone and `starts`
      // is still fully populated for every bin — `agents_in_bin` and the row
      // window both index empty bins.
      let mut acc = 0u32;
      for b in 0..self.counts.len() {
          self.starts[b] = acc;
          self.cursor[b] = acc;
          if self.count_stamp[b] == stamp {
              acc += self.counts[b];
          }
      }
      let last = self.starts.len() - 1;
      self.starts[last] = acc;

      // Ascending `i` with a per-bin cursor keeps each bucket sorted by agent
      // index, so a neighbour scan visits the same pairs in the same order on
      // every run and every process.
      for i in 0..self.len {
          let (bx, by) = self.bin_of(x[i], y[i]);
          let b = (bx + by * self.cols) as usize;
          let slot = self.cursor[b] as usize;
          self.items[slot] = i as u32;
          self.cursor[b] += 1;
      }
      ```
- [ ] 4. Add the O(1) population query next to `agents_in_bin`:
      ```rust
      /// Agents in one bin, without touching `items`. `0` for an out-of-range
      /// or empty bin.
      pub fn bin_count(&self, bx: u32, by: u32) -> u32 {
          if bx >= self.cols || by >= self.rows {
              return 0;
          }
          let b = (bx + by * self.cols) as usize;
          if self.count_stamp[b] == self.stamp {
              self.counts[b]
          } else {
              0
          }
      }
      ```
- [ ] 5. Add the wrap hook, testkit-gated so it cannot be reached from a
      shipping build:
      ```rust
      /// Force the rebuild counter, so the wrap path is reachable in a test
      /// without performing `u32::MAX` rebuilds.
      #[cfg(feature = "testkit")]
      pub fn set_stamp_for_test(&mut self, stamp: u32) {
          self.stamp = stamp;
      }
      ```
- [ ] 6. Update the module doc at the top of `spatial.rs`: after the existing
      "counting sort over fixed-size bins" sentence, add — *"Bin populations
      carry a rebuild stamp, so a rebuild never clears them; a bin whose stamp
      is stale reads as empty. `starts` is still written for every bin, so an
      empty bin still has a valid, zero-width range."*
- [ ] 7. Write the three new tests from the test plan in
      `crates/mmd-engine/tests/separation.rs`, immediately after
      `spatial_rebuild_is_repeatable`.
- [ ] 8. Run validation. If `a_bodied_scenario_is_pinned_to_a_golden_digest`
      fails, the rewrite changed bucket order — do **not** re-measure the
      digest; fix step 3.

## Outputs

- Touched: `crates/mmd-engine/src/sim/spatial.rs`,
  `crates/mmd-engine/tests/separation.rs`.
- Public API added: `SpatialGrid::bin_count`, and
  `SpatialGrid::set_stamp_for_test` behind `#[cfg(feature = "testkit")]`.
- Behaviour change: none. Memory: two extra `Vec<u32>` of `cols * rows`,
  allocated once in `SpatialGrid::new`, never per tick.
- Migration / config: none.

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --locked` — green
- [ ] `cargo test -p mmd-engine --test separation` — green, including
      `a_bodied_scenario_is_pinned_to_a_golden_digest` **unedited**
- [ ] `cargo test -p mmd-engine --test frame_allocations` — green, including
      `spatial_rebuild_allocates_nothing` **unedited**
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo build -p mmd-engine --no-default-features --features gpu` — the
      testkit-gated hook must not leak into a shipping build
- [ ] `cargo run -- run --agents 5000 --frames 300` — exits 0
- [ ] the gate smoke `hash=` equals the T0 pinned digest, byte for byte
- [ ] `graphify update .` run (graph refresh; `graphify-out/` is gitignored)
- [ ] `nix flake check`
- [ ] app functional — scenes load, tick and exit; nothing observable changed
- [ ] commit msg draft: `refactor(sim): stamp the neighbour bins instead of clearing them each rebuild`
