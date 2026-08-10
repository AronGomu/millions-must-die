//! SDL3 GPU sprite renderer (host backend: Vulkan / D3D12 / Metal).

mod atlas;
mod backend;
mod camera;
mod device;
mod error;
mod golden;
mod instance;
mod renderer;
mod text;
mod unsafe_sys;

pub use atlas::{
    ATLAS_COUNT, ATLAS_HEIGHT_PX, ATLAS_SLOT_COUNT, ATLAS_WIDTH_PX, AtlasRgba, FRAME_SIZE_PX,
    RTS_FILES, SLOT_RTS_BUILDINGS, SLOT_RTS_PROPS, SLOT_RTS_SOLDIER, SLOT_RTS_WORKER, SLOT_UI_FONT,
    SPRITE_SIZE_PX, frame_uv_rect, load_atlases, load_rts_atlases, load_ui_font, rts_atlas_dir,
    ui_atlas_dir,
};
pub use backend::{
    BACKEND_D3D12, BACKEND_METAL, BACKEND_VULKAN, REQUIRED_BACKEND, REQUIRED_LINUX_BACKEND,
    REQUIRED_MACOS_ARCH, assert_device_backend, is_apple_silicon_arch, is_rejected_adapter,
    required_backend, validate_adapter_name, validate_backend_name, validate_device_props,
    validate_macos_host_arch,
};
pub use camera::{
    CAMERA_PAN_CELLS_PER_SEC, Camera, EDGE_PAN_MARGIN_PX, edge_pan_dir, screen_dir_to_cells,
};
pub use device::GpuContext;
pub use error::RenderError;
pub use golden::{
    GOLDEN_DIFF_ACTUAL_PNG, GOLDEN_DIFF_MASK_PNG, GOLDEN_DIFF_SUMMARY_JSON,
    GOLDEN_MAX_CHANNEL_DELTA_POLICY, GOLDEN_SCENE_STATIC_DEMO, GOLDEN_SCHEMA_VERSION,
    GOLDEN_STATUS_CAPTURED, GOLDEN_STATUS_PLACEHOLDER, GoldenDiffSummary, GoldenError,
    GoldenManifest, HostBinding, compare_readback, compare_readback_writing_diff,
    decode_readback_png, diff_readback, encode_rgba_png, golden_family_dir, host_binding_hashes,
    load_golden_image, load_golden_manifest, verify_report_binding, write_golden,
    write_golden_diff,
};
pub use instance::{
    FrameUniforms, ISO_DEPTH_EPSILON, ISO_TILE_H_PER_CELL, ISO_TILE_W_PER_CELL, IsoView,
    QUAD_INDICES, QUAD_VERTICES, QuadVertex, RING_SENTINEL, SpriteInstance, clip_to_pixel,
    iso_depth, iso_origin, iso_project, iso_unproject, quad_is_visible, world_to_clip,
};
pub use renderer::{
    DrawGroup, FRAMES_IN_FLIGHT, MAX_INSTANCES, Readback, ScenePass, SpriteRenderer, VIEW_HEIGHT,
    VIEW_WIDTH,
};
pub use text::{
    FONT_COLS, FONT_FIRST_CHAR, FONT_LAST_CHAR, FONT_REPLACEMENT, FONT_ROWS, GLYPH_H_PX,
    GLYPH_TRACKING_PX, GLYPH_W_PX, begin_text_group, glyph_uv_rect, push_text, text_width,
};
pub use unsafe_sys::RawFrameFence;
