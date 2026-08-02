//! SDL3 GPU sprite renderer (host backend: Vulkan / D3D12 / Metal).

mod atlas;
mod backend;
mod device;
mod error;
mod instance;
mod renderer;
mod unsafe_sys;

pub use atlas::{
    ATLAS_COUNT, ATLAS_HEIGHT_PX, ATLAS_WIDTH_PX, AtlasRgba, SPRITE_SIZE_PX,
    expected_sprite_center_pixel, frame_uv_rect, load_atlases, sprite_pixel,
};
pub use backend::{
    BACKEND_D3D12, BACKEND_METAL, BACKEND_VULKAN, REQUIRED_BACKEND, REQUIRED_LINUX_BACKEND,
    REQUIRED_MACOS_ARCH, assert_device_backend, is_apple_silicon_arch, is_rejected_adapter,
    required_backend, validate_adapter_name, validate_backend_name, validate_device_props,
    validate_macos_host_arch,
};
pub use device::GpuContext;
pub use error::RenderError;
pub use instance::{FrameUniforms, QUAD_INDICES, QUAD_VERTICES, QuadVertex, SpriteInstance};
pub use renderer::{
    DrawGroup, FRAMES_IN_FLIGHT, MAX_INSTANCES, Readback, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH,
};
