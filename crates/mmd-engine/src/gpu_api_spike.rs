//! Compile-only SDL3 GPU wrapper surface spike (T6).
//!
//! Proves selected `sdl3` / `sdl3-sys` symbols resolve at the pinned crate
//! versions without linking a native SDL3 shared library (`sdl3-sys/no-link`).
//! Runtime `Device::new` / shader load paths need the T7 bootstrap prefix.

#![allow(dead_code)]

use sdl3::gpu::{ShaderFormat, ShaderStage, TransferBufferUsage};

/// Type-level anchors for APIs the renderer will use in T7+.
pub struct GpuApiSurface;

impl GpuApiSurface {
    /// Backend format flags required by the multi-OS matrix.
    pub fn required_shader_formats() -> ShaderFormat {
        ShaderFormat::SPIRV | ShaderFormat::DXIL | ShaderFormat::METALLIB
    }

    /// Stage enums used for VS/FS blob loads.
    pub const fn stages() -> (ShaderStage, ShaderStage) {
        (ShaderStage::Vertex, ShaderStage::Fragment)
    }

    /// Transfer buffer usage flag present for instance uploads.
    pub const fn transfer_upload_usage() -> TransferBufferUsage {
        sdl3_sys::gpu::SDL_GPU_TRANSFERBUFFERUSAGE_UPLOAD
    }

    /// Confirms safe wrapper re-exports the sys shader-format constants we pin against.
    pub fn format_bits_match_sys() -> bool {
        ShaderFormat::SPIRV.0 == sdl3_sys::gpu::SDL_GPU_SHADERFORMAT_SPIRV
            && ShaderFormat::DXIL.0 == sdl3_sys::gpu::SDL_GPU_SHADERFORMAT_DXIL
            && ShaderFormat::METALLIB.0 == sdl3_sys::gpu::SDL_GPU_SHADERFORMAT_METALLIB
    }
}

// Ensure builder/device module paths exist at the pinned crate version (type-only).
#[allow(unused_imports)]
use sdl3::gpu::{Device, Shader, ShaderBuilder, TransferBuffer};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spike_symbols_resolve() {
        let fmt = GpuApiSurface::required_shader_formats();
        assert_ne!(fmt.0.0, 0);
        let (vs, fs) = GpuApiSurface::stages();
        assert!(matches!(vs, ShaderStage::Vertex));
        assert!(matches!(fs, ShaderStage::Fragment));
        let _ = GpuApiSurface::transfer_upload_usage();
        assert!(GpuApiSurface::format_bits_match_sys());
    }
}
