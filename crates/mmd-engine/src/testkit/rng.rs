//! Deterministic seed stream for test harnesses.

/// SplitMix64 — the reference finalizer, chosen because it is fully specified
/// by arithmetic (no table, no platform word-size dependence, no library
/// version drift), so a seed produces the same stream on every host and in
/// every process. That property is the whole point of the T29 harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next 64-bit draw.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Next draw reduced to `0..bound`. Modulo bias is irrelevant here: the
    /// stream only has to be *reproducible*, not uniform.
    pub fn next_bounded(&mut self, bound: u64) -> u64 {
        assert!(bound > 0, "bound must be > 0");
        self.next_u64() % bound
    }

    /// Independent sub-stream for a named subsystem, so adding a system later
    /// cannot shift the draws an existing system already depends on.
    pub fn derive(&self, label: &str) -> Self {
        let mut mixed = self.state;
        for b in label.as_bytes() {
            mixed = (mixed ^ u64::from(*b)).wrapping_mul(0x100_0000_01B3);
        }
        Self::new(mixed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_is_reproducible_and_advances() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        let first: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
        let second: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
        assert_eq!(first, second);
        assert!(first.windows(2).all(|w| w[0] != w[1]));
    }

    #[test]
    fn distinct_seeds_and_labels_diverge() {
        assert_ne!(
            SplitMix64::new(42).next_u64(),
            SplitMix64::new(43).next_u64()
        );
        let base = SplitMix64::new(42);
        assert_ne!(
            base.derive("camera").next_u64(),
            base.derive("selection").next_u64()
        );
    }

    #[test]
    fn known_vector_is_frozen() {
        // Locks the exact stream: a future "harmless" constant edit would
        // silently invalidate every recorded state hash.
        let mut r = SplitMix64::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn derived_vector_is_frozen() {
        // `derive` feeds `seed_spawn_placement`, so its mixing constant decides
        // every seeded state hash. Comparing two labels for inequality would
        // survive a constant change; only a golden value pins it.
        assert_eq!(
            SplitMix64::new(42).derive("spawn-placement").next_u64(),
            0x29CE_C342_3615_9021
        );
        assert_eq!(
            SplitMix64::new(0).derive("").next_u64(),
            0xE220_A839_7B1D_CDAF
        );
    }
}
