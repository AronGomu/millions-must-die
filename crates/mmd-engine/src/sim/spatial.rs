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
