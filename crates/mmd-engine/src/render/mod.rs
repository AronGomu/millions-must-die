//! SDL3 GPU sprite renderer (Linux/Vulkan static slice).

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
pub use backend::{REQUIRED_LINUX_BACKEND, assert_device_backend, validate_backend_name};
pub use device::GpuContext;
pub use error::RenderError;
pub use instance::{FrameUniforms, QUAD_INDICES, QUAD_VERTICES, QuadVertex, SpriteInstance};
pub use renderer::{
    DrawGroup, FRAMES_IN_FLIGHT, MAX_INSTANCES, Readback, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH,
};
