//! Renderer errors.

use thiserror::Error;

/// Fallible render / device paths.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("wrong GPU backend: got {got:?}, required {required:?}")]
    WrongBackend { got: String, required: &'static str },

    #[error("rejected GPU adapter: {name:?} (Basic Render Driver / software not allowed)")]
    RejectedAdapter { name: String },

    #[error("SDL error: {0}")]
    Sdl(String),

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
