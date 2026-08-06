//! Renderer errors.

use thiserror::Error;

/// Fallible render / device paths.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("wrong GPU backend: got {got:?}, required {required:?}")]
    WrongBackend { got: String, required: &'static str },

    #[error(
        "rejected GPU adapter: {name:?} (Basic Render Driver / MoltenVK / software not allowed)"
    )]
    RejectedAdapter { name: String },

    #[error(
        "rejected host arch: got {got:?}, required {required:?} (macOS Metal = Apple Silicon only)"
    )]
    RejectedHostArch { got: String, required: &'static str },

    #[error("SDL error: {0}")]
    Sdl(String),

    #[error("GPU device unavailable on this host: {0}")]
    DeviceUnavailable(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("atlas error: {0}")]
    Atlas(String),

    #[error("shader error: {0}")]
    Shader(String),

    #[error("readback size mismatch: got {got_w}x{got_h}, expected {want_w}x{want_h}")]
    ReadbackSize {
        got_w: u32,
        got_h: u32,
        want_w: u32,
        want_h: u32,
    },

    #[error("pixel mismatch at ({x},{y}): got {got:?}, expected {expected:?}")]
    PixelMismatch {
        x: u32,
        y: u32,
        got: [u8; 4],
        expected: [u8; 4],
    },

    #[error("group count mismatch: got {got}, expected {expected}")]
    GroupCount { got: usize, expected: usize },
}

impl RenderError {
    /// True only when this host has **no usable GPU device at all** — SDL
    /// could not initialise, or the required backend refused to create a
    /// device.
    ///
    /// GPU-bound tests skip on `true` and *fail* on `false`. That asymmetry is
    /// the point: a drifted atlas, a rejected software adapter, or a broken
    /// group contract are real defects and must never be mistaken for "we are
    /// running in a headless shell". Widening this predicate turns the skip
    /// path into a hole in the merge gate.
    ///
    /// Known narrowness, deliberate: a host whose *only* Vulkan ICD is a
    /// software rasterizer (lavapipe) does create a device and then trips
    /// [`super::validate_adapter_name`], yielding [`Self::RejectedAdapter`] —
    /// so it fails rather than skips. Reclassifying that would also excuse the
    /// macOS "never MoltenVK" policy rejection, which must stay a hard
    /// failure, and the two cannot be told apart on a host that has neither.
    /// Left loud on purpose; `MMD_REQUIRE_GPU=1` covers the opposite risk.
    pub fn is_device_unavailable(&self) -> bool {
        matches!(self, Self::DeviceUnavailable(_))
    }
}

impl From<sdl3::Error> for RenderError {
    fn from(value: sdl3::Error) -> Self {
        RenderError::Sdl(value.to_string())
    }
}

impl From<sdl3::properties::PropertiesError> for RenderError {
    fn from(value: sdl3::properties::PropertiesError) -> Self {
        RenderError::Sdl(format!("{value:?}"))
    }
}
