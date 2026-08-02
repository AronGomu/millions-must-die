//! Engine core for Millions Must Die phase-0 prototype.
//!
//! Simulation, renderer, and benchmark logic land in later tickets.

pub mod nav;
pub mod scenario;

/// Crate version string from Cargo package metadata.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_nonzero() {
        assert!(!version().is_empty());
    }
}
