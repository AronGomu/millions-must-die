//! SDL3 GPU sprite renderer (host backend: Vulkan / D3D12 / Metal).

mod atlas;
mod backend;
mod device;
mod error;
mod golden;
mod instance;
mod renderer;
mod unsafe_sys;

pub use atlas::{
    ATLAS_COUNT, ATLAS_HEIGHT_PX, ATLAS_WIDTH_PX, AtlasRgba, FRAME_SIZE_PX, SPRITE_SIZE_PX,
    frame_uv_rect, load_atlases,
};
pub use backend::{
    BACKEND_D3D12, BACKEND_METAL, BACKEND_VULKAN, REQUIRED_BACKEND, REQUIRED_LINUX_BACKEND,
    REQUIRED_MACOS_ARCH, assert_device_backend, is_apple_silicon_arch, is_rejected_adapter,
    required_backend, validate_adapter_name, validate_backend_name, validate_device_props,
    validate_macos_host_arch,
};
pub use device::GpuContext;
pub use error::RenderError;
pub use golden::{
    GOLDEN_MAX_CHANNEL_DELTA_POLICY, GOLDEN_SCENE_STATIC_DEMO, GOLDEN_SCHEMA_VERSION,
    GOLDEN_STATUS_CAPTURED, GOLDEN_STATUS_PLACEHOLDER, GoldenError, GoldenManifest, HostBinding,
    compare_readback, decode_readback_png, encode_rgba_png, golden_family_dir, host_binding_hashes,
    load_golden_image, load_golden_manifest, verify_report_binding, write_golden,
};
pub use instance::{FrameUniforms, QUAD_INDICES, QUAD_VERTICES, QuadVertex, SpriteInstance};
pub use renderer::{
    DrawGroup, FRAMES_IN_FLIGHT, MAX_INSTANCES, Readback, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH,
};
