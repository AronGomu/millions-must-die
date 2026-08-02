//! Engine core for Millions Must Die phase-0 prototype.
//!
//! Scenario, navigation, simulation, renderer, and benchmark logic land in later tickets.

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
