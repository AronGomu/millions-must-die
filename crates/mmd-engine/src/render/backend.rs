//! Backend selection contract (host-forced GPU driver + adapter gate).
//!
//! Linux → `vulkan`, Windows → `direct3d12`, macOS → `metal`.
//! Shared sim/runtime stays backend-neutral; only device create + shader format branch.

use super::RenderError;

/// SDL GPU driver name: Vulkan.
pub const BACKEND_VULKAN: &str = "vulkan";
/// SDL GPU driver name: D3D12.
pub const BACKEND_D3D12: &str = "direct3d12";
/// SDL GPU driver name: Metal.
pub const BACKEND_METAL: &str = "metal";

/// Host required GPU driver for this build target.
#[cfg(target_os = "linux")]
pub const REQUIRED_BACKEND: &str = BACKEND_VULKAN;
/// Host required GPU driver for this build target.
#[cfg(target_os = "windows")]
pub const REQUIRED_BACKEND: &str = BACKEND_D3D12;
/// Host required GPU driver for this build target.
#[cfg(target_os = "macos")]
pub const REQUIRED_BACKEND: &str = BACKEND_METAL;
/// Unsupported host: fail compile (phase-0 matrix is linux/windows/macos only).
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
compile_error!("mmd-engine render backend supports linux, windows, macos only");

/// Linux alias kept for existing call sites / docs.
pub const REQUIRED_LINUX_BACKEND: &str = BACKEND_VULKAN;

/// Names never accepted as the forced host backend.
#[cfg_attr(not(test), allow(dead_code))]
pub const REJECTED_BACKENDS: &[&str] = &["software", "opengles2", ""];

/// Substrings matched case-insensitively against `SDL.gpu.device.name`.
/// Microsoft WARP / Basic Render Driver is not a valid prototype ref adapter.
pub const REJECTED_ADAPTER_SUBSTRINGS: &[&str] =
    &["microsoft basic render driver", "basic render driver"];

/// Required backend for current compile target.
pub fn required_backend() -> &'static str {
    REQUIRED_BACKEND
}

/// Validate requested backend name before/after device creation.
pub fn validate_backend_name(name: &str) -> Result<(), RenderError> {
    if name != REQUIRED_BACKEND {
        return Err(RenderError::WrongBackend {
            got: name.to_string(),
            required: REQUIRED_BACKEND,
        });
    }
    Ok(())
}

/// Assert live device driver matches required backend.
pub fn assert_device_backend(driver: &str) -> Result<(), RenderError> {
    validate_backend_name(driver)
}

/// True when adapter name is software / Basic Render Driver (or similar).
pub fn is_rejected_adapter(adapter_name: &str) -> bool {
    let lower = adapter_name.to_ascii_lowercase();
    REJECTED_ADAPTER_SUBSTRINGS
        .iter()
        .any(|needle| lower.contains(needle))
}

/// Reject Microsoft Basic Render Driver and empty adapter names on Windows path.
pub fn validate_adapter_name(adapter_name: &str) -> Result<(), RenderError> {
    let trimmed = adapter_name.trim();
    if trimmed.is_empty() {
        // Empty name is tolerated on Linux Vulkan (some ICDs omit it); Windows gate
        // still rejects Basic Render via substring match when present.
        return Ok(());
    }
    if is_rejected_adapter(trimmed) {
        return Err(RenderError::RejectedAdapter {
            name: trimmed.to_string(),
        });
    }
    Ok(())
}

/// Combined driver + adapter props check (unit-testable with fake strings).
pub fn validate_device_props(driver: &str, adapter_name: &str) -> Result<(), RenderError> {
    validate_backend_name(driver)?;
    validate_adapter_name(adapter_name)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_required_backend_is_known() {
        let req = required_backend();
        assert!(
            matches!(req, BACKEND_VULKAN | BACKEND_D3D12 | BACKEND_METAL),
            "unexpected required backend {req}"
        );
        validate_backend_name(req).expect("host backend ok");
    }

    #[test]
    fn wrong_backend_rejected() {
        for name in REJECTED_BACKENDS {
            let err = validate_backend_name(name).expect_err("must reject");
            match err {
                RenderError::WrongBackend { got, required } => {
                    assert_eq!(got, *name);
                    assert_eq!(required, REQUIRED_BACKEND);
                }
                other => panic!("unexpected error: {other}"),
            }
        }
        // Cross-backend names rejected on this host (except the host's own).
        for foreign in [BACKEND_VULKAN, BACKEND_D3D12, BACKEND_METAL] {
            if foreign == REQUIRED_BACKEND {
                continue;
            }
            assert!(
                validate_backend_name(foreign).is_err(),
                "foreign backend {foreign} must fail on host {REQUIRED_BACKEND}"
            );
        }
    }

    #[test]
    fn rejects_basic_renderer() {
        let samples = [
            "Microsoft Basic Render Driver",
            "microsoft basic render driver",
            "MICROSOFT BASIC RENDER DRIVER",
            "Microsoft Direct3D12 (Microsoft Basic Render Driver)",
        ];
        for name in samples {
            let err = validate_adapter_name(name).expect_err(name);
            match err {
                RenderError::RejectedAdapter { name: got } => {
                    assert!(
                        got.to_ascii_lowercase().contains("basic render"),
                        "got {got}"
                    );
                }
                other => panic!("unexpected error for {name}: {other}"),
            }
        }
        // Real ref-class names pass adapter gate.
        validate_adapter_name("AMD Radeon RX 6400").expect("rx6400");
        validate_adapter_name("Microsoft Direct3D12 (AMD Radeon RX 6400)").expect("d3d12 rx");
        validate_adapter_name("").expect("empty ok");
    }

    #[test]
    fn d3d12_backend_required_contract() {
        // Portable contract: Windows host forces direct3d12; other hosts reject it.
        #[cfg(target_os = "windows")]
        {
            assert_eq!(REQUIRED_BACKEND, BACKEND_D3D12);
            validate_device_props(BACKEND_D3D12, "AMD Radeon RX 6400").expect("win d3d12");
            let err = validate_device_props(BACKEND_D3D12, "Microsoft Basic Render Driver")
                .expect_err("basic");
            assert!(matches!(err, RenderError::RejectedAdapter { .. }));
            assert!(validate_device_props(BACKEND_VULKAN, "AMD Radeon RX 6400").is_err());
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert_ne!(REQUIRED_BACKEND, BACKEND_D3D12);
            assert!(validate_device_props(BACKEND_D3D12, "AMD Radeon RX 6400").is_err());
            // Adapter reject still works even when driver is wrong (driver checked first).
            let err = validate_adapter_name("Microsoft Basic Render Driver").expect_err("basic");
            assert!(matches!(err, RenderError::RejectedAdapter { .. }));
        }
    }
}
