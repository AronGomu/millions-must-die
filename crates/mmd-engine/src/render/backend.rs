//! Backend selection contract (Linux = Vulkan only).

use super::RenderError;

/// Linux phase-0 required GPU driver name.
pub const REQUIRED_LINUX_BACKEND: &str = "vulkan";

/// Names that must never be accepted for the Linux gate path.
#[cfg_attr(not(test), allow(dead_code))]
pub const REJECTED_BACKENDS: &[&str] = &["software", "direct3d12", "metal", "opengles2", ""];

/// Validate requested backend name before device creation.
pub fn validate_backend_name(name: &str) -> Result<(), RenderError> {
    if name != REQUIRED_LINUX_BACKEND {
        return Err(RenderError::WrongBackend {
            got: name.to_string(),
            required: REQUIRED_LINUX_BACKEND,
        });
    }
    Ok(())
}

/// Assert live device driver matches required backend.
pub fn assert_device_backend(driver: &str) -> Result<(), RenderError> {
    validate_backend_name(driver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_backend_rejected() {
        for name in REJECTED_BACKENDS {
            let err = validate_backend_name(name).expect_err("must reject");
            match err {
                RenderError::WrongBackend { got, required } => {
                    assert_eq!(got, *name);
                    assert_eq!(required, REQUIRED_LINUX_BACKEND);
                }
                other => panic!("unexpected error: {other}"),
            }
        }
        validate_backend_name(REQUIRED_LINUX_BACKEND).expect("vulkan ok");
    }
}
