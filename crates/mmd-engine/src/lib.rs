//! Engine core for Millions Must Die phase-0 prototype.

pub mod alloc_guard;
pub mod bench;
pub mod nav;
pub mod render;
pub mod runtime;
pub mod scenario;
pub mod sim;

#[cfg(feature = "gpu-api-spike")]
pub mod gpu_api_spike;

/// Crate version string from Cargo package metadata.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Resolve workspace root from this crate manifest (`crates/mmd-engine`).
pub fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("mmd-engine lives at crates/mmd-engine")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_nonzero() {
        assert!(!version().is_empty());
    }
}
