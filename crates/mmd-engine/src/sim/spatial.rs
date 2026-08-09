//! Uniform-grid neighbour index for agent-agent queries.
//!
//! A counting sort over fixed-size bins: `rebuild` refills preallocated
//! buffers, so a tick that rebuilds the grid allocates nothing (the engine-wide
//! invariant in `crate::alloc_guard`). Bucket contents come out in ascending
//! agent index, which is what makes a neighbour scan reproducible rather than
//! merely correct.
//!
//! Bin populations carry a rebuild stamp, so a rebuild never clears them; a
//! bin whose stamp is stale reads as empty. `starts` is still written for
//! every bin, so an empty bin still has a valid, zero-width range.

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
    /// Per-bin population for the rebuild identified by `stamp`. A bin whose
    /// `count_stamp` differs is empty; its `counts` entry is stale garbage
    /// and must never be read.
    counts: Vec<u32>,
    /// Rebuild that last wrote `counts[b]`; length `cols * rows`.
    count_stamp: Vec<u32>,
    /// Monotone rebuild counter. Starts at 0 so the first rebuild's stamp of
    /// 1 marks every zero-initialised `count_stamp` entry stale.
    stamp: u32,
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
            counts: vec![0; n_bins],
            count_stamp: vec![0; n_bins],
            stamp: 0,
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

    /// Force the rebuild counter, so the wrap path is reachable in a test
    /// without performing `u32::MAX` rebuilds.
    #[cfg(feature = "testkit")]
    pub fn set_stamp_for_test(&mut self, stamp: u32) {
        self.stamp = stamp;
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
}
