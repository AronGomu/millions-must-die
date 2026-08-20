# T3: SpatialGrid neighbour bins

**Plan:** `./artifacts/PLAN_2026_08_08_zombie-collision.md`
**Depends:** T2
**Commit outcome:** A deterministic, zero-allocation uniform-grid neighbour index exists as `mmd_engine::sim::SpatialGrid`, with its own tests. Nothing calls it yet, so simulation behaviour is unchanged.

## Context (self-contained)

- Goal: zombies stop passing through each other, via soft separation steering — a repulsion vector summed into the flow-field vector before the single move. Finding "which agents are near me" is the part that must not cost an allocation or depend on iteration luck.
- This slice: the neighbour index alone, as a standalone type with no `Simulation` dependency. T4 wires it in.
- Out of scope here: `crates/mmd-engine/src/sim/agents.rs`, `crates/mmd-engine/src/sim/tick.rs`, the scenario, the renderer, any repulsion maths. Do **not** add a `SpatialGrid` field to `Simulation` in this ticket.
- Assumptions in force:
  - The engine forbids per-frame heap allocation (`crates/mmd-engine/src/alloc_guard.rs`, enforced per-thread by `MeasureGuard`). Every buffer is sized once in `SpatialGrid::new` and only ever refilled.
  - Determinism is a hard contract: the same inputs must produce the same bin contents in the same order, so the bucket order is ascending agent index, always.
  - Positions are in **cell space** (`f32`), the same space `Simulation::x` / `Simulation::y` use: an agent at cell (3, 4) sits at (3.5, 4.5).

## Requirements

- `SpatialGrid::new(width, height, bin_size_cells, capacity)` reserves everything up front.
- `rebuild(x, y)` is a counting sort: two passes, no allocation, deterministic.
- `agents_in_bin(bx, by)` returns agent indices in **ascending order**.
- Positions outside the world rect, and non-finite positions, clamp into range instead of panicking — a caller must never be able to turn a stray coordinate into an out-of-bounds index.
- `rebuild` panics loudly on a caller mistake: mismatched slice lengths, or more agents than `capacity`.

## Inputs

- New file: `crates/mmd-engine/src/sim/spatial.rs`.
- `crates/mmd-engine/src/sim/mod.rs` — currently exactly:
  ```rust
  //! Fixed-tick SoA agent simulation.

  mod agents;
  mod tick;

  pub use agents::{AgentsView, Simulation, quantize_cell};
  pub use tick::{ARRIVAL_RADIUS, SPEED_CELLS_PER_SEC, TICK_DT};
  ```
- `crates/mmd-engine/tests/frame_allocations.rs` — already installs `#[global_allocator] static GLOBAL: CountingAllocator` and imports `MeasureGuard, alloc_count, is_counting, reset_count` from `mmd_engine::alloc_guard`. It serialises its tests through `fn lock_alloc_tests() -> std::sync::MutexGuard<'static, ()>`. Any new allocation test must take that lock as its first statement.
- **From Depends (T2):** the scenario contract now exposes `Scenario::collision_radius_cells() -> f32` and `mmd_engine::scenario::COLLISION_Q8`. T3 does not use them — the bin size arrives as a plain `f32` argument so this type stays independent of the scenario. T4 is what connects the two.

## Code to add — copy verbatim

`crates/mmd-engine/src/sim/spatial.rs`:

```rust
//! Uniform-grid neighbour index for agent-agent queries.
//!
//! A counting sort over fixed-size bins: `rebuild` refills preallocated
//! buffers, so a tick that rebuilds the grid allocates nothing (the engine-wide
//! invariant in `crate::alloc_guard`). Bucket contents come out in ascending
//! agent index, which is what makes a neighbour scan reproducible rather than
//! merely correct.

/// Preallocated uniform bins over the world rect.
#[derive(Debug, Clone)]
pub struct SpatialGrid {
    cols: u32,
    rows: u32,
    bin_size_cells: f32,
    inv_bin_size: f32,
    /// Bin start offsets into `items`; length `cols * rows + 1`.
    starts: Vec<u32>,
    /// Write cursors during a rebuild; length `cols * rows`.
    cursor: Vec<u32>,
    /// Agent indices, bucketed; first `len` entries are live.
    items: Vec<u32>,
    len: usize,
}

impl SpatialGrid {
    /// Reserve bins covering a `width` x `height` cell world, each
    /// `bin_size_cells` on a side, for at most `capacity` agents.
    ///
    /// `bin_size_cells` is clamped to at least 1.0: a bin smaller than a cell
    /// buys nothing and multiplies the bin count.
    pub fn new(width: u32, height: u32, bin_size_cells: f32, capacity: usize) -> Self {
        let bin = if bin_size_cells.is_finite() && bin_size_cells > 1.0 {
            bin_size_cells
        } else {
            1.0
        };
        let cols = ((width as f32 / bin).ceil() as u32).max(1);
        let rows = ((height as f32 / bin).ceil() as u32).max(1);
        let n_bins = (cols as usize)
            .checked_mul(rows as usize)
            .expect("bin count overflow");
        Self {
            cols,
            rows,
            bin_size_cells: bin,
            inv_bin_size: 1.0 / bin,
            starts: vec![0; n_bins + 1],
            cursor: vec![0; n_bins],
            items: vec![0; capacity],
            len: 0,
        }
    }

    pub fn cols(&self) -> u32 {
        self.cols
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    pub fn bin_size_cells(&self) -> f32 {
        self.bin_size_cells
    }

    pub fn capacity(&self) -> usize {
        self.items.len()
    }

    /// Agents placed by the last [`Self::rebuild`].
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Bin holding a cell-space position. Out-of-rect and non-finite
    /// coordinates clamp to the nearest bin — a stray coordinate must not be
    /// able to produce an out-of-bounds index.
    pub fn bin_of(&self, x: f32, y: f32) -> (u32, u32) {
        (self.axis_bin(x, self.cols), self.axis_bin(y, self.rows))
    }

    fn axis_bin(&self, v: f32, limit: u32) -> u32 {
        if !v.is_finite() || v <= 0.0 {
            return 0;
        }
        let b = (v * self.inv_bin_size) as u32;
        b.min(limit - 1)
    }

    /// Refill the bins from SoA positions. Allocates nothing.
    ///
    /// # Panics
    /// If `x.len() != y.len()`, or the count exceeds the reserved capacity.
    pub fn rebuild(&mut self, x: &[f32], y: &[f32]) {
        assert_eq!(x.len(), y.len(), "position slices must be the same length");
        assert!(
            x.len() <= self.items.len(),
            "grid holds {} agents, got {}",
            self.items.len(),
            x.len()
        );
        self.len = x.len();

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
        // Ascending `i` with a per-bin cursor keeps each bucket sorted by agent
        // index, so a neighbour scan visits the same pairs in the same order
        // on every run and every process.
        for i in 0..self.len {
            let (bx, by) = self.bin_of(x[i], y[i]);
            let b = (bx + by * self.cols) as usize;
            let slot = self.cursor[b] as usize;
            self.items[slot] = i as u32;
            self.cursor[b] += 1;
        }
    }

    /// Agent indices in one bin, ascending. Empty for an out-of-range bin.
    pub fn agents_in_bin(&self, bx: u32, by: u32) -> &[u32] {
        if bx >= self.cols || by >= self.rows {
            return &[];
        }
        let b = (bx + by * self.cols) as usize;
        let lo = self.starts[b] as usize;
        let hi = self.starts[b + 1] as usize;
        &self.items[lo..hi]
    }
}
```

`crates/mmd-engine/src/sim/mod.rs` becomes:

```rust
//! Fixed-tick SoA agent simulation.

mod agents;
mod spatial;
mod tick;

pub use agents::{AgentsView, Simulation, quantize_cell};
pub use spatial::SpatialGrid;
pub use tick::{ARRIVAL_RADIUS, SPEED_CELLS_PER_SEC, TICK_DT};
```

## TDD

1. **Red** — write `crates/mmd-engine/tests/separation.rs` with the five tests below plus the allocation test in `frame_allocations.rs`. They fail to compile because `SpatialGrid` does not exist.
2. **Green** — add `spatial.rs` and the `mod`/`pub use` lines verbatim.
3. **Refactor** — none. Do not generalise the type; T4 is its only consumer.

## Test plan

New file `crates/mmd-engine/tests/separation.rs`, opening with
`use mmd_engine::sim::SpatialGrid;`.

| Test | Input | Expect |
| ---- | ----- | ------ |
| `spatial_bins_hold_every_agent_exactly_once` | 8×8 world, bin 1.0, positions `(0.5,0.5) (0.5,0.5) (7.5,0.5) (3.5,3.5) (7.5,7.5)` | `grid.len() == 5`; concatenating `agents_in_bin` over every `(bx, by)` yields a set equal to `{0,1,2,3,4}` with no duplicate; `agents_in_bin(0,0) == [0, 1]` |
| `spatial_bucket_order_is_ascending_agent_index` | 4×4 world, bin 1.0, 32 agents all at `(1.5, 1.5)` | `agents_in_bin(1,1)` equals `(0..32).collect::<Vec<u32>>()` — a stack must not scramble |
| `spatial_bin_size_covers_two_radii` | `SpatialGrid::new(480, 270, 7.5, 16)` | `bin_size_cells() == 7.5`, `cols() == 64`, `rows() == 36` |
| `spatial_bin_size_never_drops_below_one_cell` | `SpatialGrid::new(16, 16, 0.25, 4)` | `bin_size_cells() == 1.0`, `cols() == 16`, `rows() == 16` |
| `spatial_clamps_positions_outside_the_world` | 4×4 world, bin 1.0, positions `(-3.0, -3.0)`, `(99.0, 99.0)`, `(f32::NAN, 0.5)` | no panic; `bin_of(-3.0, -3.0) == (0, 0)`; `bin_of(99.0, 99.0) == (3, 3)`; `bin_of(f32::NAN, 0.5) == (0, 0)`; every agent still appears exactly once across all bins |
| `spatial_rebuild_is_repeatable` | rebuild the same grid twice from the same slices | the flattened bin contents are equal both times |

New test in `crates/mmd-engine/tests/frame_allocations.rs`:

| Test | Input | Expect |
| ---- | ----- | ------ |
| `spatial_rebuild_allocates_nothing` | `SpatialGrid::new(64, 64, 1.0, 512)` built **before** the guard, warmed with one rebuild, then 8 rebuilds inside `MeasureGuard::enter()` | `guard.allocations() == 0`; `guard.assert_zero()` does not panic |

Exact shape of that test — the lock must come first, and construction must sit
outside the guard, or the reservation itself counts:

```rust
#[test]
fn spatial_rebuild_allocates_nothing() {
    let _lock = lock_alloc_tests();
    reset_count();

    let n = 512;
    let mut grid = SpatialGrid::new(64, 64, 1.0, n);
    let xs: Vec<f32> = (0..n).map(|i| (i % 64) as f32 + 0.5).collect();
    let ys: Vec<f32> = (0..n).map(|i| (i / 64) as f32 + 0.5).collect();
    grid.rebuild(&xs, &ys); // warm-up: any lazy growth happens here

    let guard = MeasureGuard::enter();
    for _ in 0..8 {
        grid.rebuild(&xs, &ys);
        std::hint::black_box(grid.len());
    }
    assert_eq!(guard.allocations(), 0);
    guard.assert_zero();
}
```

## Impl steps

- [x] 1. Create `crates/mmd-engine/tests/separation.rs` with the six tests from the table above; run `cargo test -p mmd-engine --test separation` and confirm it fails to compile (red).
- [x] 2. Add `use mmd_engine::sim::SpatialGrid;` to the imports of `crates/mmd-engine/tests/frame_allocations.rs` and append `spatial_rebuild_allocates_nothing` verbatim from above.
- [x] 3. Create `crates/mmd-engine/src/sim/spatial.rs` with the module contents given above, verbatim.
- [x] 4. Replace `crates/mmd-engine/src/sim/mod.rs` with the seven-line version given above.
- [x] 5. Run `cargo test -p mmd-engine --test separation` → six tests pass.
- [x] 6. Run `cargo test -p mmd-engine --test frame_allocations` → six tests pass, including the new one.
- [x] 7. Run `cargo clippy --workspace --all-targets --all-features -- -D warnings` and fix any lint in the new file only (expect `len_without_is_empty` to be satisfied already — `is_empty` is provided).
- [x] 8. Run the full validation list below.

## Outputs

- Files touched: `crates/mmd-engine/src/sim/spatial.rs` (new), `crates/mmd-engine/src/sim/mod.rs`, `crates/mmd-engine/tests/separation.rs` (new), `crates/mmd-engine/tests/frame_allocations.rs`.
- Public API added (quoted verbatim by T4):
  - `mmd_engine::sim::SpatialGrid`
  - `SpatialGrid::new(width: u32, height: u32, bin_size_cells: f32, capacity: usize) -> SpatialGrid`
  - `SpatialGrid::cols(&self) -> u32`, `rows(&self) -> u32`, `bin_size_cells(&self) -> f32`, `capacity(&self) -> usize`, `len(&self) -> usize`, `is_empty(&self) -> bool`
  - `SpatialGrid::rebuild(&mut self, x: &[f32], y: &[f32])`
  - `SpatialGrid::bin_of(&self, x: f32, y: f32) -> (u32, u32)`
  - `SpatialGrid::agents_in_bin(&self, bx: u32, by: u32) -> &[u32]`
- Behaviour change: none — no caller.
- Migrate / config: none.

## Validation

- [x] `cargo test -p mmd-engine --test separation` → 6 passed
- [x] `cargo test -p mmd-engine --test frame_allocations` → 6 passed
- [x] `cargo fmt --all -- --check` → exit 0
- [x] `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0
- [x] `MMD_REQUIRE_GPU=1 cargo test --workspace --locked` → green
- [x] `cargo run -- run --agents 50000 --frames 300` → exit 0; the `hash=` on the `clean exit` line is unchanged from T2 (nothing consumes the grid yet)
- [x] `cargo tree -e features -p millions_must_die | grep -c testkit` → prints `0` (the shipping binary must not pull the harness in; `grep -c` exits 1 on a zero count, which is the passing case here)
- [x] app functional — no broken path from this slice
- [x] commit msg draft: `feat(sim): add a zero-alloc uniform-grid neighbour index`
