//! Sprite batch renderer over a flat 9-slot texture table: a three-layer scene
//! pass (world / overlay / UI), offscreen readback, swapchain.

use std::ffi::CStr;
use std::path::Path;

use sdl3::gpu::{
    BlendFactor, BlendOp, Buffer, BufferBinding, BufferRegion, BufferUsageFlags,
    ColorTargetBlendState, ColorTargetDescription, ColorTargetInfo, CompareOp, CullMode,
    DepthStencilState, DepthStencilTargetInfo, Device, FillMode, Filter, GraphicsPipeline,
    GraphicsPipelineTargetInfo, IndexElementSize, LoadOp, PrimitiveType, RasterizerState,
    SampleCount, Sampler, SamplerAddressMode, SamplerCreateInfo, SamplerMipmapMode, Shader,
    ShaderFormat, ShaderStage, StoreOp, Texture, TextureCreateInfo, TextureFormat, TextureRegion,
    TextureSamplerBinding, TextureTransferInfo, TextureType, TextureUsage, TransferBuffer,
    TransferBufferLocation, TransferBufferUsage, VertexAttribute, VertexBufferDescription,
    VertexElementFormat, VertexInputRate, VertexInputState,
};
use sdl3::pixels::Color;
use sdl3::video::Window;

use super::RenderError;
use super::atlas::{
    ATLAS_COUNT, ATLAS_HEIGHT_PX, ATLAS_SLOT_COUNT, ATLAS_WIDTH_PX, AtlasRgba, SPRITE_SIZE_PX,
    default_atlas_dir, frame_uv_rect, load_atlases, load_rts_atlases, load_ui_font,
};
use super::device::GpuContext;
use super::instance::{FrameUniforms, QUAD_INDICES, QUAD_VERTICES, QuadVertex, SpriteInstance};
use super::unsafe_sys;

/// Offscreen / gate resolution.
pub const VIEW_WIDTH: u32 = 1920;
pub const VIEW_HEIGHT: u32 = 1080;
/// Frames-in-flight for cycled instance uploads.
pub const FRAMES_IN_FLIGHT: usize = 2;
/// Max sprites uploaded per frame across all groups.
///
/// A GPU buffer capacity, deliberately far above the live simultaneous-agent
/// ceiling (`scenario::MAX_LIVE_AGENTS` = 5 000) rather than sized to it: the
/// buffer is allocated once at renderer construction, a frame can carry both a
/// sprite and a ring per agent, and re-tuning a device allocation every time
/// the scene ceiling moves buys nothing. Kept at its historical value; the
/// figure describes the buffer, never a population the engine will accept.
pub const MAX_INSTANCES: u32 = 100_000;

/// Depth attachment format for the sprite pass.
///
/// `D16_UNORM` is the one depth format SDL3 guarantees on every backend, and
/// 16 bits is ample here: the key is a cell's position down the map diamond in
/// `[0, 1]`, so the 480 × 270 gate scene needs 750 distinguishable levels and
/// this has 65 536.
const DEPTH_FORMAT: TextureFormat = TextureFormat::D16Unorm;

/// Depth the attachment is cleared to at the start of every pass.
///
/// `0`, not `1`: the key is the agent's position down the map diamond, so a
/// larger value is nearer the camera and the test is `GREATER`. Clearing to the
/// *smallest* key is what makes the first sprite at any pixel win.
const DEPTH_CLEAR: f32 = 0.0;

/// Default depth normalisation: screen-space `y` over the view.
///
/// Content authored directly in view pixels — the static golden scene, the GPU
/// probes — has no map diamond behind it, so "further down the screen is
/// nearer" is the only honest reading. A [`Runtime`](crate::runtime::Runtime)
/// overrides both scalars with its scene's own via
/// [`SpriteRenderer::set_depth_params`].
const DEFAULT_DEPTH_SCALE: f32 = 1.0 / VIEW_HEIGHT as f32;
const DEFAULT_DEPTH_BIAS: f32 = 0.0;

// Host shader blobs: SPIR-V (Linux), DXIL (Windows), metallib (macOS).
// macOS uses one library blob for both stages (entry points differ).
#[cfg(target_os = "linux")]
const VERT_SHADER: &[u8] = include_bytes!("../../../../shaders/generated/sprite.vert.spv");
#[cfg(target_os = "linux")]
const FRAG_SHADER: &[u8] = include_bytes!("../../../../shaders/generated/sprite.frag.spv");
#[cfg(target_os = "windows")]
const VERT_SHADER: &[u8] = include_bytes!("../../../../shaders/generated/sprite.vert.dxil");
#[cfg(target_os = "windows")]
const FRAG_SHADER: &[u8] = include_bytes!("../../../../shaders/generated/sprite.frag.dxil");
#[cfg(target_os = "macos")]
const VERT_SHADER: &[u8] = include_bytes!("../../../../shaders/generated/sprite.metallib");
#[cfg(target_os = "macos")]
const FRAG_SHADER: &[u8] = include_bytes!("../../../../shaders/generated/sprite.metallib");

/// Texture slots that are not one of the phase-0 four: the RTS sheets, then
/// the UI font.
const EXTRA_SLOT_COUNT: usize = ATLAS_SLOT_COUNT - ATLAS_COUNT;

/// One draw group: every instance in it samples the same texture slot.
#[derive(Clone, Debug)]
pub struct DrawGroup {
    /// Index into the renderer's texture table, valid in `0..ATLAS_SLOT_COUNT`.
    pub atlas_id: u32,
    pub instances: Vec<SpriteInstance>,
}

/// One frame's three layers, in draw order.
///
/// `world` is depth-tested. `overlay` and `ui` are not: they are drawn on the
/// same pipeline the hitbox rings already use, whose depth test *and* depth
/// write are both off. `ui` is drawn last so a panel is never occluded by a
/// world annotation.
#[derive(Clone, Copy, Debug, Default)]
pub struct ScenePass<'a> {
    /// Depth-tested world sprites. `atlas_id` is a texture slot.
    pub world: &'a [DrawGroup],
    /// World-space annotations, procedural rings only: hitbox rings and
    /// selection rings. One flat range, drawn with slot 0 bound (the ring
    /// branch samples no texture; every *textured* depth-off instance —
    /// placement tiles, the drag box, rally flags, icons, panels and glyphs —
    /// goes in `ui` instead, or it samples the zombie atlas).
    pub overlay: &'a [SpriteInstance],
    /// Screen-space UI, grouped by texture slot.
    pub ui: &'a [DrawGroup],
    /// This pass's own depth uniforms, if it has one.
    ///
    /// `None` is the phase-0 shape: the renderer falls back to its
    /// constructor-fixed `(depth_scale, depth_bias)` via
    /// [`SpriteRenderer::set_depth_params`], which keeps every legacy caller
    /// and the committed golden byte-identical. `Some` is what an RTS scene
    /// packed this tick supplies, so a panned camera's depth bias follows it
    /// instead of staying pinned to frame 0's.
    pub frame_uniforms: Option<FrameUniforms>,
}

impl<'a> ScenePass<'a> {
    /// The phase-0 shape: world groups plus a ring overlay, no UI, no
    /// per-pass frame uniforms.
    ///
    /// With `ui` empty the pass emits exactly the sequence of binds and draws
    /// the four-group path emitted before the table existed, which is what
    /// keeps the committed golden byte-identical.
    pub fn world_and_rings(world: &'a [DrawGroup], rings: &'a [SpriteInstance]) -> Self {
        Self {
            world,
            overlay: rings,
            ui: &[],
            frame_uniforms: None,
        }
    }
}

/// CPU readback of offscreen target.
#[derive(Clone, Debug)]
pub struct Readback {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Readback {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

/// Static-slice renderer resources.
///
/// Field order matters: `ctx` (owns `Device`) is last so GPU objects release first.
pub struct SpriteRenderer {
    pipeline: GraphicsPipeline,
    /// Overlay pipeline: same shader modules, depth test **and** write off.
    ///
    /// Rings annotate bodies; a unit standing in front of one must not hide the
    /// annotation. This is the second pipeline T8 deliberately deferred.
    ring_pipeline: GraphicsPipeline,
    quad_vb: Buffer,
    quad_ib: Buffer,
    instance_bufs: [Buffer; FRAMES_IN_FLIGHT],
    upload_xfer: TransferBuffer,
    download_xfer: TransferBuffer,
    atlases_cpu: [AtlasRgba; ATLAS_COUNT],
    atlas_tex: [Texture<'static>; ATLAS_COUNT],
    /// Texture slots 4..=8: RTS sheets then the UI font. Kept separate from
    /// `atlas_tex` so the phase-0 four stay exactly where they were.
    extra_tex: [Texture<'static>; EXTRA_SLOT_COUNT],
    /// CPU copies of the five phase-1 sheets, for headless assertions.
    extra_cpu: [AtlasRgba; EXTRA_SLOT_COUNT],
    sampler: Sampler,
    offscreen: Texture<'static>,
    /// Depth attachment for the sprite pass, cleared to [`DEPTH_CLEAR`] every
    /// frame.
    depth: Texture<'static>,
    /// `FrameUniforms::depth_scale` for every frame this renderer draws.
    depth_scale: f32,
    /// `FrameUniforms::depth_bias` for every frame this renderer draws.
    depth_bias: f32,
    frame_slot: usize,
    /// Reused contiguous pack for GPU upload (reserved at construct).
    pack_scratch: Vec<SpriteInstance>,
    /// Per-group `(start, count)` ranges, world groups first then UI groups.
    /// Cleared, not reallocated, per draw. Reserved at [`ATLAS_SLOT_COUNT`],
    /// which covers any scene that names each slot at most once — the shape
    /// the table exists for.
    group_ranges: Vec<(u32, u32)>,
    /// Device/SDL ownership — must drop after all GPU objects above.
    pub ctx: GpuContext,
}

impl SpriteRenderer {
    /// Build renderer; loads atlases from workspace `assets/sprites/generated`.
    pub fn new(workspace_root: &Path, debug_mode: bool) -> Result<Self, RenderError> {
        let ctx = GpuContext::new(debug_mode)?;
        let atlas_dir = default_atlas_dir(workspace_root);
        Self::with_context(ctx, &atlas_dir)
    }

    /// Build from existing context + atlas directory.
    pub fn with_context(ctx: GpuContext, atlas_dir: &Path) -> Result<Self, RenderError> {
        let device = &ctx.device;
        let atlases_cpu = load_atlases(atlas_dir)?;
        // Sibling directories of the phase-0 four, so `new(workspace_root, ..)`
        // keeps working and a test pointing this at a temp dir gets the temp
        // families too.
        let rts_cpu = load_rts_atlases(&atlas_dir.join("rts"))?;
        let ui_font_cpu = load_ui_font(&atlas_dir.join("ui"))?;
        // Destructured rather than collected: the slot order of the table is
        // the thing being asserted here, and this form stops compiling if
        // either family's size moves out from under `EXTRA_SLOT_COUNT`.
        let [worker, soldier, buildings, props] = rts_cpu;
        let extra_cpu: [AtlasRgba; EXTRA_SLOT_COUNT] =
            [worker, soldier, buildings, props, ui_font_cpu];

        let (format, vert_entry, frag_entry) = host_shader_spec();
        let vert = device
            .create_shader()
            .with_code(format, VERT_SHADER, ShaderStage::Vertex)
            .with_uniform_buffers(1)
            .with_entrypoint(vert_entry)
            .build()?;
        let frag = device
            .create_shader()
            .with_code(format, FRAG_SHADER, ShaderStage::Fragment)
            .with_samplers(1)
            .with_entrypoint(frag_entry)
            .build()?;

        let blend = ColorTargetBlendState::new()
            .with_enable_blend(true)
            .with_src_color_blendfactor(BlendFactor::One)
            .with_dst_color_blendfactor(BlendFactor::OneMinusSrcAlpha)
            .with_color_blend_op(BlendOp::Add)
            .with_src_alpha_blendfactor(BlendFactor::One)
            .with_dst_alpha_blendfactor(BlendFactor::OneMinusSrcAlpha)
            .with_alpha_blend_op(BlendOp::Add);

        // Bound to named locals rather than written inline: both pipelines are
        // built from exactly the same vertex layout and colour target, and
        // `VertexInputState` borrows these slices for as long as it lives.
        let buffer_descs = [
            VertexBufferDescription::new()
                .with_slot(0)
                .with_pitch(QuadVertex::STRIDE)
                .with_input_rate(VertexInputRate::Vertex)
                .with_instance_step_rate(0),
            VertexBufferDescription::new()
                .with_slot(1)
                .with_pitch(SpriteInstance::STRIDE)
                .with_input_rate(VertexInputRate::Instance)
                // SDL3 requires instance_step_rate == 0 for every slot.
                .with_instance_step_rate(0),
        ];
        let attributes = [
            VertexAttribute::new()
                .with_location(0)
                .with_buffer_slot(0)
                .with_format(VertexElementFormat::Float2)
                .with_offset(0),
            VertexAttribute::new()
                .with_location(1)
                .with_buffer_slot(0)
                .with_format(VertexElementFormat::Float2)
                .with_offset(8),
            VertexAttribute::new()
                .with_location(2)
                .with_buffer_slot(1)
                .with_format(VertexElementFormat::Float2)
                .with_offset(0),
            VertexAttribute::new()
                .with_location(3)
                .with_buffer_slot(1)
                .with_format(VertexElementFormat::Float2)
                .with_offset(8),
            VertexAttribute::new()
                .with_location(4)
                .with_buffer_slot(1)
                .with_format(VertexElementFormat::Float4)
                .with_offset(16),
            VertexAttribute::new()
                .with_location(5)
                .with_buffer_slot(1)
                .with_format(VertexElementFormat::Float4)
                .with_offset(32),
        ];
        let color_targets = [ColorTargetDescription::new()
            .with_format(TextureFormat::R8g8b8a8Unorm)
            .with_blend_state(blend)];

        // Sprites: depth-tested and depth-writing, so a unit standing in front
        // of another covers it without anyone sorting a thing. Correctness
        // under alpha comes from the fragment stage's cutout, not from order.
        //
        // The comparison is `GREATER` against a buffer cleared to [`DEPTH_CLEAR`]
        // = 0, because the key the vertex stage emits is the agent's position
        // *down the map diamond*: larger is further down the screen, which in an
        // isometric view is nearer the camera. A `LESS` test would draw the
        // horde back to front and let the rank behind cover the rank in front.
        let pipeline = build_pipeline(
            device,
            &vert,
            &frag,
            &buffer_descs,
            &attributes,
            &color_targets,
            DepthStencilState::new()
                .with_compare_op(CompareOp::Greater)
                .with_enable_depth_test(true)
                .with_enable_depth_write(true),
        )?;
        // Rings: the *same* shader modules, with the depth block off. A debug
        // overlay that a unit could stand in front of would stop annotating the
        // thing it exists to annotate.
        let ring_pipeline = build_pipeline(
            device,
            &vert,
            &frag,
            &buffer_descs,
            &attributes,
            &color_targets,
            DepthStencilState::new()
                .with_enable_depth_test(false)
                .with_enable_depth_write(false),
        )?;
        drop(vert);
        drop(frag);

        let cmd = device.acquire_command_buffer()?;
        let copy_pass = device.begin_copy_pass(&cmd)?;

        let staging = device
            .create_transfer_buffer()
            .with_usage(TransferBufferUsage::UPLOAD)
            .with_size(
                (ATLAS_WIDTH_PX * ATLAS_HEIGHT_PX * 4)
                    .max(std::mem::size_of_val(&QUAD_VERTICES) as u32)
                    .max(std::mem::size_of_val(&QUAD_INDICES) as u32)
                    .max(MAX_INSTANCES * SpriteInstance::STRIDE),
            )
            .build()?;

        let quad_vb = upload_slice(
            device,
            &staging,
            &copy_pass,
            BufferUsageFlags::VERTEX,
            &QUAD_VERTICES,
        )?;
        let quad_ib = upload_slice(
            device,
            &staging,
            &copy_pass,
            BufferUsageFlags::INDEX,
            &QUAD_INDICES,
        )?;

        let mut atlas_tex_vec = Vec::with_capacity(ATLAS_COUNT);
        for atlas in &atlases_cpu {
            atlas_tex_vec.push(upload_atlas_texture(device, &staging, &copy_pass, atlas)?);
        }
        let atlas_tex: [Texture<'static>; ATLAS_COUNT] = atlas_tex_vec
            .try_into()
            .map_err(|_| RenderError::Atlas("atlas tex count".into()))?;

        let mut extra_tex_vec = Vec::with_capacity(EXTRA_SLOT_COUNT);
        for sheet in &extra_cpu {
            extra_tex_vec.push(upload_atlas_texture(device, &staging, &copy_pass, sheet)?);
        }
        let extra_tex: [Texture<'static>; EXTRA_SLOT_COUNT] = extra_tex_vec
            .try_into()
            .map_err(|_| RenderError::Atlas("extra tex count".into()))?;

        device.end_copy_pass(copy_pass);
        cmd.submit()?;

        let mut instance_bufs_vec = Vec::with_capacity(FRAMES_IN_FLIGHT);
        for _ in 0..FRAMES_IN_FLIGHT {
            instance_bufs_vec.push(
                device
                    .create_buffer()
                    .with_usage(BufferUsageFlags::VERTEX)
                    .with_size(MAX_INSTANCES * SpriteInstance::STRIDE)
                    .build()?,
            );
        }
        let instance_bufs: [Buffer; FRAMES_IN_FLIGHT] = instance_bufs_vec
            .try_into()
            .map_err(|_| RenderError::Sdl("instance bufs".into()))?;

        let upload_xfer = device
            .create_transfer_buffer()
            .with_usage(TransferBufferUsage::UPLOAD)
            .with_size(MAX_INSTANCES * SpriteInstance::STRIDE)
            .build()?;
        let download_xfer = device
            .create_transfer_buffer()
            .with_usage(TransferBufferUsage::DOWNLOAD)
            .with_size(VIEW_WIDTH * VIEW_HEIGHT * 4)
            .build()?;

        let sampler = device.create_sampler(
            SamplerCreateInfo::new()
                .with_min_filter(Filter::Nearest)
                .with_mag_filter(Filter::Nearest)
                .with_mipmap_mode(SamplerMipmapMode::Nearest)
                .with_address_mode_u(SamplerAddressMode::ClampToEdge)
                .with_address_mode_v(SamplerAddressMode::ClampToEdge)
                .with_address_mode_w(SamplerAddressMode::ClampToEdge),
        )?;

        let offscreen = device.create_texture(
            TextureCreateInfo::new()
                .with_type(TextureType::_2D)
                .with_format(TextureFormat::R8g8b8a8Unorm)
                .with_width(VIEW_WIDTH)
                .with_height(VIEW_HEIGHT)
                .with_layer_count_or_depth(1)
                .with_num_levels(1)
                .with_sample_count(SampleCount::NoMultiSampling)
                .with_usage(TextureUsage::COLOR_TARGET | TextureUsage::SAMPLER),
        )?;

        // Never sampled and never read back: it exists only so the sprite pass
        // can resolve front-to-back within one submit.
        let depth = device.create_texture(
            TextureCreateInfo::new()
                .with_type(TextureType::_2D)
                .with_format(DEPTH_FORMAT)
                .with_width(VIEW_WIDTH)
                .with_height(VIEW_HEIGHT)
                .with_layer_count_or_depth(1)
                .with_num_levels(1)
                .with_sample_count(SampleCount::NoMultiSampling)
                .with_usage(TextureUsage::DEPTH_STENCIL_TARGET),
        )?;

        let out = Self {
            pipeline,
            ring_pipeline,
            quad_vb,
            quad_ib,
            instance_bufs,
            upload_xfer,
            download_xfer,
            atlases_cpu,
            atlas_tex,
            extra_tex,
            extra_cpu,
            sampler,
            offscreen,
            depth,
            depth_scale: DEFAULT_DEPTH_SCALE,
            depth_bias: DEFAULT_DEPTH_BIAS,
            frame_slot: 0,
            pack_scratch: Vec::with_capacity(MAX_INSTANCES as usize),
            group_ranges: Vec::with_capacity(ATLAS_SLOT_COUNT),
            ctx,
        };
        Ok(out)
    }

    pub fn backend(&self) -> &str {
        &self.ctx.backend
    }

    pub fn atlases(&self) -> &[AtlasRgba; ATLAS_COUNT] {
        &self.atlases_cpu
    }

    /// CPU pixels for `slot`, for headless tests. `None` past the table.
    ///
    /// Slots 0..=3 are the zombie atlases, 4..=8 the phase-1 sheets.
    pub fn atlas_pixels(&self, slot: u32) -> Option<&AtlasRgba> {
        let i = slot as usize;
        match self.atlases_cpu.get(i) {
            Some(atlas) => Some(atlas),
            // `i >= ATLAS_COUNT` on this arm, so the subtraction cannot wrap.
            None => self.extra_cpu.get(i - ATLAS_COUNT),
        }
    }

    /// Capacity of the per-frame instance pack buffer.
    ///
    /// Reserved once at construction and never grown: a frame that reallocated
    /// it would have packed past [`MAX_INSTANCES`] before the budget guard
    /// reported the error. Exposed so that ordering is testable rather than
    /// merely commented.
    pub fn pack_capacity(&self) -> usize {
        self.pack_scratch.capacity()
    }

    /// Point the vertex stage's depth normalisation at a scene's own map.
    ///
    /// The camera is fixed, so this is set once per scene from
    /// [`IsoView`](super::IsoView) rather than per frame — see
    /// `Runtime::iso_view`. Left alone, the renderer normalises over the view
    /// height ([`DEFAULT_DEPTH_SCALE`]), which is what content authored
    /// directly in view pixels wants.
    pub fn set_depth_params(&mut self, depth_scale: f32, depth_bias: f32) {
        self.depth_scale = depth_scale;
        self.depth_bias = depth_bias;
    }

    /// The `(depth_scale, depth_bias)` this renderer uploads each frame.
    pub fn depth_params(&self) -> (f32, f32) {
        (self.depth_scale, self.depth_bias)
    }

    /// Canonical static scene: one sprite per atlas at fixed positions.
    pub fn static_demo_groups() -> [DrawGroup; ATLAS_COUNT] {
        let size = [SPRITE_SIZE_PX as f32, SPRITE_SIZE_PX as f32];
        let uv = frame_uv_rect(0, 0);
        let positions = [
            [100.0, 100.0],
            [200.0, 100.0],
            [300.0, 100.0],
            [400.0, 100.0],
        ];
        std::array::from_fn(|i| DrawGroup {
            atlas_id: i as u32,
            instances: vec![SpriteInstance::new(
                positions[i],
                size,
                uv,
                SpriteInstance::WHITE,
            )],
        })
    }

    /// Upload instances + draw 4 atlas groups and a hitbox-ring overlay into
    /// offscreen 1920×1080 (no readback).
    ///
    /// Pass an empty `rings` slice for the plain 4-group frame; the ring draw
    /// is skipped entirely rather than issued empty.
    pub fn draw_offscreen_with_rings(
        &mut self,
        groups: &[DrawGroup],
        rings: &[SpriteInstance],
    ) -> Result<(), RenderError> {
        self.validate_groups(groups)?;
        self.draw_offscreen_scene(ScenePass::world_and_rings(groups, rings))
    }

    /// Draw a whole [`ScenePass`] into the offscreen target (no readback).
    pub fn draw_offscreen_scene(&mut self, scene: ScenePass<'_>) -> Result<(), RenderError> {
        // Waited, not dropped: this path has no other backpressure, so a
        // caller that submits faster than the GPU retires (the RTS loop does,
        // and did so as soon as it stopped digesting the world every frame)
        // grows the queue without bound until the driver kills the device
        // with `VK_ERROR_DEVICE_LOST`. One frame in flight is enough for a
        // headless run, and this is a correctness bound, not a timing claim.
        self.draw_scene_into(scene)?.wait();
        Ok(())
    }

    /// Draw offscreen and return fence for the render submit (bench 2-frame queue).
    ///
    /// Caller owns backpressure: must not reuse a slot until its prior fence completes.
    /// Fence completion latency is submit→signal proxy — not true GPU execution time.
    ///
    /// Returns a raw fence (`unsafe_sys` escape): the safe sdl3 `Fence`
    /// allocates an `Arc` per acquire, which would put one Rust heap
    /// allocation inside every measured bench frame and trip the zero-alloc
    /// gate. Drop releases the fence C-side.
    pub fn draw_offscreen_acquire_fence(
        &mut self,
        groups: &[DrawGroup],
    ) -> Result<unsafe_sys::RawFrameFence, RenderError> {
        self.validate_groups(groups)?;
        self.draw_scene_into(ScenePass::world_and_rings(groups, &[]))
    }

    /// Draw one [`ScenePass`] into the offscreen target and return the render
    /// submit's fence.
    ///
    /// All three layers ride the *same* pass, shader modules and blend state —
    /// they are extra ranges in one instance buffer, not separate techniques.
    /// The world layer runs on the depth-tested pipeline; the overlay and the
    /// UI run on the pipeline whose depth test and depth write are both off,
    /// so a unit standing in front of a ring never hides it and a panel is
    /// never occluded by a world annotation.
    ///
    /// For the overlay the fragment stage picks the ring or diagonal-line branch
    /// off a sentinel in `uv_rect` ([`SpriteInstance::ring`],
    /// [`SpriteInstance::diagonal_line`]), so the texture bound for that draw is
    /// never sampled; slot 0 is bound anyway because the pipeline declares one
    /// sampler and a draw must not leave it unbound.
    fn draw_scene_into(
        &mut self,
        scene: ScenePass<'_>,
    ) -> Result<unsafe_sys::RawFrameFence, RenderError> {
        self.validate_scene(&scene)?;

        // Checked before the first byte is written: `pack_scratch` is reserved
        // at `MAX_INSTANCES` exactly so a frame never grows it, and a guard
        // that ran after the appends would let the overflowing frame realloc
        // (and permanently double the buffer) before reporting the error.
        let total: usize = scene
            .world
            .iter()
            .chain(scene.ui.iter())
            .map(|g| g.instances.len())
            .sum::<usize>()
            + scene.overlay.len();
        if total > MAX_INSTANCES as usize {
            return Err(RenderError::Sdl(format!("too many instances {total}")));
        }

        let device = &self.ctx.device;
        let slot = self.frame_slot % FRAMES_IN_FLIGHT;
        self.frame_slot = self.frame_slot.wrapping_add(1);

        // Pack instances contiguously into reused scratch; record per-group
        // ranges. World ranges first, then UI ranges — the overlay sits
        // between them in the buffer but keeps its own local range, because it
        // is one flat run rather than a group list.
        self.pack_scratch.clear();
        self.group_ranges.clear();
        for g in scene.world {
            let start = self.pack_scratch.len() as u32;
            self.pack_scratch.extend_from_slice(&g.instances);
            self.group_ranges.push((start, g.instances.len() as u32));
        }
        let world_range_len = scene.world.len();
        // The overlay goes after the world so it lands on top of the sprites it
        // annotates, and before the UI so a panel covers it.
        let overlay_range = {
            let start = self.pack_scratch.len() as u32;
            self.pack_scratch.extend_from_slice(scene.overlay);
            (start, scene.overlay.len() as u32)
        };
        for g in scene.ui {
            let start = self.pack_scratch.len() as u32;
            self.pack_scratch.extend_from_slice(&g.instances);
            self.group_ranges.push((start, g.instances.len() as u32));
        }
        debug_assert!(self.pack_scratch.len() <= MAX_INSTANCES as usize);

        // Upload instances (cycled buffer).
        {
            let cmd = device.acquire_command_buffer()?;
            let copy = device.begin_copy_pass(&cmd)?;
            let nbytes = (self.pack_scratch.len() * std::mem::size_of::<SpriteInstance>()) as u32;
            if nbytes > 0 {
                let mut map = self.upload_xfer.map::<SpriteInstance>(device, true);
                map.mem_mut()[..self.pack_scratch.len()].copy_from_slice(&self.pack_scratch);
                map.unmap();
                copy.upload_to_gpu_buffer(
                    TransferBufferLocation::new()
                        .with_offset(0)
                        .with_transfer_buffer(&self.upload_xfer),
                    BufferRegion::new()
                        .with_buffer(&self.instance_bufs[slot])
                        .with_offset(0)
                        .with_size(nbytes),
                    true,
                );
            }
            device.end_copy_pass(copy);
            cmd.submit()?;
        }

        // Render pass → offscreen; acquire fence for queue-depth + latency proxy.
        let cmd = device.acquire_command_buffer()?;
        let uniforms = scene.frame_uniforms.unwrap_or(FrameUniforms {
            view_size: [VIEW_WIDTH as f32, VIEW_HEIGHT as f32],
            depth_scale: self.depth_scale,
            depth_bias: self.depth_bias,
        });
        cmd.push_vertex_uniform_data(0, &uniforms);

        let color_targets = [ColorTargetInfo::default()
            .with_texture(&self.offscreen)
            .with_load_op(LoadOp::CLEAR)
            .with_store_op(StoreOp::STORE)
            .with_clear_color(Color::RGBA(0, 0, 0, 0))];
        // Cleared to the far plane every frame and discarded at the end — the
        // buffer is scratch for one pass, never read by anything else.
        let depth_target = DepthStencilTargetInfo::new()
            .with_texture(&mut self.depth)
            .with_clear_depth(DEPTH_CLEAR)
            .with_load_op(LoadOp::CLEAR)
            .with_store_op(StoreOp::DONT_CARE)
            .with_stencil_load_op(LoadOp::DONT_CARE)
            .with_stencil_store_op(StoreOp::DONT_CARE);
        let pass = device.begin_render_pass(&cmd, &color_targets, Some(&depth_target))?;
        pass.bind_graphics_pipeline(&self.pipeline);
        pass.bind_vertex_buffers(
            0,
            &[
                BufferBinding::new()
                    .with_buffer(&self.quad_vb)
                    .with_offset(0),
                BufferBinding::new()
                    .with_buffer(&self.instance_bufs[slot])
                    .with_offset(0),
            ],
        );
        pass.bind_index_buffer(
            &BufferBinding::new()
                .with_buffer(&self.quad_ib)
                .with_offset(0),
            IndexElementSize::_16BIT,
        );

        for (i, (start, count)) in self.group_ranges[..world_range_len].iter().enumerate() {
            if *count == 0 {
                continue;
            }
            pass.bind_fragment_samplers(
                0,
                &[TextureSamplerBinding::new()
                    .with_texture(texture_at(
                        &self.atlas_tex,
                        &self.extra_tex,
                        scene.world[i].atlas_id,
                    ))
                    .with_sampler(&self.sampler)],
            );
            // Re-bind instance buffer with per-group byte offset.
            let byte_off = start * SpriteInstance::STRIDE;
            pass.bind_vertex_buffers(
                1,
                &[BufferBinding::new()
                    .with_buffer(&self.instance_bufs[slot])
                    .with_offset(byte_off)],
            );
            pass.draw_indexed_primitives(6, *count, 0, 0, 0);
        }

        // Hitbox rings: after every world group, same pass and same shader
        // modules, on the pipeline whose depth block is off.
        let (ring_start, ring_count) = overlay_range;
        if ring_count > 0 {
            pass.bind_graphics_pipeline(&self.ring_pipeline);
            // Slot 1 below is the bind this draw genuinely needs — it moves to
            // the ring range's byte offset. Slot 0, the index buffer and the
            // sampler are re-issued defensively; every backend dedupes an
            // identical rebind, so they cost nothing, and SDL3 does not
            // *document* what survives a pipeline switch.
            //
            // The one thing deliberately NOT re-issued is the frame uniform:
            // `push_vertex_uniform_data` is documented to hold its value for the
            // whole command buffer, and the ring draw depends on that.
            pass.bind_vertex_buffers(
                0,
                &[BufferBinding::new()
                    .with_buffer(&self.quad_vb)
                    .with_offset(0)],
            );
            pass.bind_index_buffer(
                &BufferBinding::new()
                    .with_buffer(&self.quad_ib)
                    .with_offset(0),
                IndexElementSize::_16BIT,
            );
            // Slot 0 satisfies the pipeline's one declared sampler; the ring
            // branch never samples it. Without this bind, a frame whose world
            // groups were all empty would draw with no texture bound at all.
            pass.bind_fragment_samplers(
                0,
                &[TextureSamplerBinding::new()
                    .with_texture(texture_at(&self.atlas_tex, &self.extra_tex, 0))
                    .with_sampler(&self.sampler)],
            );
            pass.bind_vertex_buffers(
                1,
                &[BufferBinding::new()
                    .with_buffer(&self.instance_bufs[slot])
                    .with_offset(ring_start * SpriteInstance::STRIDE)],
            );
            pass.draw_indexed_primitives(6, ring_count, 0, 0, 0);
        }

        // Screen-space UI last, so a panel covers both the world and the
        // annotations over it. The depth-off pipeline is bound on the first
        // *drawn* UI group rather than under `if !scene.ui.is_empty()`, so a
        // frame with an empty overlay still gets it — and a frame with an
        // empty UI layer emits nothing here at all, which is what leaves the
        // phase-0 sequence untouched.
        let mut ui_pipeline_bound = false;
        for (i, (start, count)) in self.group_ranges[world_range_len..].iter().enumerate() {
            if *count == 0 {
                continue;
            }
            if !ui_pipeline_bound {
                pass.bind_graphics_pipeline(&self.ring_pipeline);
                // Re-issued for the same defensive reason as the overlay's:
                // SDL3 does not document what survives a pipeline switch.
                pass.bind_vertex_buffers(
                    0,
                    &[BufferBinding::new()
                        .with_buffer(&self.quad_vb)
                        .with_offset(0)],
                );
                pass.bind_index_buffer(
                    &BufferBinding::new()
                        .with_buffer(&self.quad_ib)
                        .with_offset(0),
                    IndexElementSize::_16BIT,
                );
                ui_pipeline_bound = true;
            }
            pass.bind_fragment_samplers(
                0,
                &[TextureSamplerBinding::new()
                    .with_texture(texture_at(
                        &self.atlas_tex,
                        &self.extra_tex,
                        scene.ui[i].atlas_id,
                    ))
                    .with_sampler(&self.sampler)],
            );
            pass.bind_vertex_buffers(
                1,
                &[BufferBinding::new()
                    .with_buffer(&self.instance_bufs[slot])
                    .with_offset(start * SpriteInstance::STRIDE)],
            );
            pass.draw_indexed_primitives(6, *count, 0, 0, 0);
        }

        device.end_render_pass(pass);
        unsafe_sys::submit_acquire_raw_fence(device, cmd).map_err(RenderError::Sdl)
    }

    /// Draw groups into offscreen 1920×1080 and read RGBA8 pixels back.
    pub fn draw_offscreen_readback(
        &mut self,
        groups: &[DrawGroup],
    ) -> Result<Readback, RenderError> {
        self.draw_offscreen_readback_with_rings(groups, &[])
    }

    /// [`Self::draw_offscreen_readback`] with a hitbox-ring overlay.
    pub fn draw_offscreen_readback_with_rings(
        &mut self,
        groups: &[DrawGroup],
        rings: &[SpriteInstance],
    ) -> Result<Readback, RenderError> {
        self.validate_groups(groups)?;
        self.draw_offscreen_readback_scene(ScenePass::world_and_rings(groups, rings))
    }

    /// [`Self::draw_offscreen_scene`] followed by a readback of the target.
    pub fn draw_offscreen_readback_scene(
        &mut self,
        scene: ScenePass<'_>,
    ) -> Result<Readback, RenderError> {
        self.draw_offscreen_scene(scene)?;
        self.readback_offscreen()
    }

    /// Download current offscreen target (after [`Self::draw_offscreen`]).
    pub fn readback_offscreen(&mut self) -> Result<Readback, RenderError> {
        let device = &self.ctx.device;
        let rgba_bytes = VIEW_WIDTH * VIEW_HEIGHT * 4;
        let cmd = device.acquire_command_buffer()?;
        let copy = device.begin_copy_pass(&cmd)?;
        unsafe_sys::download_texture(
            &copy,
            &self.offscreen,
            VIEW_WIDTH,
            VIEW_HEIGHT,
            &self.download_xfer,
        );
        device.end_copy_pass(copy);
        let fence = cmd.submit_and_acquire_fence(device)?;
        device.wait_fences(true, &[fence])?;

        let mut pixels = vec![0u8; rgba_bytes as usize];
        {
            let map = self.download_xfer.map::<u8>(device, false);
            pixels.copy_from_slice(&map.mem()[..rgba_bytes as usize]);
            map.unmap();
        }

        Ok(Readback {
            width: VIEW_WIDTH,
            height: VIEW_HEIGHT,
            rgba: pixels,
        })
    }

    /// Draw groups to offscreen, blit to claimed window swapchain.
    ///
    /// Offscreen path is authoritative for pixels. Swapchain present uses the
    /// unsafe blit seam — sdl3 0.18.4 ties swapchain `Texture` lifetime to
    /// `&mut CommandBuffer`, clashing with `begin_render_pass(&cmd)`.
    pub fn draw_to_swapchain(
        &mut self,
        window: &Window,
        groups: &[DrawGroup],
    ) -> Result<(), RenderError> {
        self.draw_to_swapchain_with_rings(window, groups, &[])
    }

    /// [`Self::draw_to_swapchain`] with a hitbox-ring overlay.
    pub fn draw_to_swapchain_with_rings(
        &mut self,
        window: &Window,
        groups: &[DrawGroup],
        rings: &[SpriteInstance],
    ) -> Result<(), RenderError> {
        self.validate_groups(groups)?;
        self.draw_to_swapchain_scene(window, ScenePass::world_and_rings(groups, rings))
    }

    /// [`Self::draw_offscreen_scene`] followed by a blit to the window's
    /// swapchain.
    pub fn draw_to_swapchain_scene(
        &mut self,
        window: &Window,
        scene: ScenePass<'_>,
    ) -> Result<(), RenderError> {
        self.draw_offscreen_scene(scene)?;
        let cmd = self.ctx.device.acquire_command_buffer()?;
        unsafe_sys::present_blit(
            &self.ctx.device,
            window,
            cmd,
            &self.offscreen,
            VIEW_WIDTH,
            VIEW_HEIGHT,
        )
        .map_err(RenderError::Sdl)?;
        Ok(())
    }

    /// Every grouped layer names a slot the table actually has.
    ///
    /// Runs before any GPU work is issued, which is what lets `texture_at`
    /// index the table directly instead of returning an `Option` nobody could
    /// act on mid-pass.
    fn validate_scene(&self, scene: &ScenePass<'_>) -> Result<(), RenderError> {
        for g in scene.world.iter().chain(scene.ui.iter()) {
            if g.atlas_id as usize >= ATLAS_SLOT_COUNT {
                return Err(RenderError::Atlas(format!(
                    "draw group atlas_id {} >= ATLAS_SLOT_COUNT {ATLAS_SLOT_COUNT}",
                    g.atlas_id
                )));
            }
        }
        Ok(())
    }

    /// The phase-0 contract: exactly [`ATLAS_COUNT`] groups, each naming its
    /// own index. Applied only by the four legacy entry points — a
    /// [`ScenePass`] is free to name any slot in any order.
    fn validate_groups(&self, groups: &[DrawGroup]) -> Result<(), RenderError> {
        if groups.len() != ATLAS_COUNT {
            return Err(RenderError::GroupCount {
                got: groups.len(),
                expected: ATLAS_COUNT,
            });
        }
        for (i, g) in groups.iter().enumerate() {
            if g.atlas_id as usize != i {
                return Err(RenderError::Atlas(format!(
                    "group {i} atlas_id {}",
                    g.atlas_id
                )));
            }
        }
        Ok(())
    }
}

/// The texture bound for `slot`. Slots 0..=3 are the zombie atlases, 4..=8 the
/// phase-1 sheets. Out of range is a caller bug and panics — `validate_scene`
/// rejects it before any GPU work is issued.
///
/// A free function rather than a `&self` method on purpose: the render pass
/// holds `&mut self.depth` for its whole lifetime, so a method taking `&self`
/// could not be called from inside it. Taking the two arrays directly keeps
/// the borrows disjoint.
fn texture_at<'t>(
    atlas_tex: &'t [Texture<'static>; ATLAS_COUNT],
    extra_tex: &'t [Texture<'static>; EXTRA_SLOT_COUNT],
    slot: u32,
) -> &'t Texture<'static> {
    let i = slot as usize;
    match atlas_tex.get(i) {
        Some(tex) => tex,
        // `i >= ATLAS_COUNT` on this arm, so the subtraction cannot wrap.
        None => &extra_tex[i - ATLAS_COUNT],
    }
}

/// Host shader format + entry points.
/// Linux SPIR-V from GLSL mirror uses `main`; DXIL/metallib from HLSL keep VSMain/PSMain.
fn host_shader_spec() -> (ShaderFormat, &'static CStr, &'static CStr) {
    #[cfg(target_os = "linux")]
    {
        (ShaderFormat::SPIRV, c"main", c"main")
    }
    #[cfg(target_os = "windows")]
    {
        (ShaderFormat::DXIL, c"VSMain", c"PSMain")
    }
    #[cfg(target_os = "macos")]
    {
        (ShaderFormat::METALLIB, c"VSMain", c"PSMain")
    }
}

/// Build one graphics pipeline over the shared vertex layout and colour
/// target, differing only in its depth-stencil block.
///
/// The sprite pass and the ring overlay run the *same* shader modules; the only
/// thing that separates them is whether they read and write depth. Threading
/// that through one builder is what keeps "same shaders, different depth state"
/// a fact rather than a comment.
#[allow(clippy::too_many_arguments)]
fn build_pipeline(
    device: &Device,
    vert: &Shader,
    frag: &Shader,
    buffer_descs: &[VertexBufferDescription],
    attributes: &[VertexAttribute],
    color_targets: &[ColorTargetDescription],
    depth_state: DepthStencilState,
) -> Result<GraphicsPipeline, RenderError> {
    Ok(device
        .create_graphics_pipeline()
        .with_primitive_type(PrimitiveType::TriangleList)
        .with_vertex_shader(vert)
        .with_fragment_shader(frag)
        .with_vertex_input_state(
            VertexInputState::new()
                .with_vertex_buffer_descriptions(buffer_descs)
                .with_vertex_attributes(attributes),
        )
        .with_rasterizer_state(
            RasterizerState::new()
                .with_fill_mode(FillMode::Fill)
                .with_cull_mode(CullMode::None),
        )
        .with_depth_stencil_state(depth_state)
        .with_target_info(
            GraphicsPipelineTargetInfo::new()
                .with_color_target_descriptions(color_targets)
                .with_depth_stencil_format(DEPTH_FORMAT)
                .with_has_depth_stencil_target(true),
        )
        .build()?)
}

fn upload_slice<T: Copy>(
    device: &Device,
    transfer: &TransferBuffer,
    copy_pass: &sdl3::gpu::CopyPass,
    usage: BufferUsageFlags,
    data: &[T],
) -> Result<Buffer, RenderError> {
    let nbytes = std::mem::size_of_val(data) as u32;
    let buffer = device
        .create_buffer()
        .with_size(nbytes)
        .with_usage(usage)
        .build()?;
    let mut map = transfer.map::<T>(device, true);
    map.mem_mut()[..data.len()].copy_from_slice(data);
    map.unmap();
    copy_pass.upload_to_gpu_buffer(
        TransferBufferLocation::new()
            .with_offset(0)
            .with_transfer_buffer(transfer),
        BufferRegion::new()
            .with_buffer(&buffer)
            .with_offset(0)
            .with_size(nbytes),
        true,
    );
    Ok(buffer)
}

fn upload_atlas_texture(
    device: &Device,
    transfer: &TransferBuffer,
    copy_pass: &sdl3::gpu::CopyPass,
    atlas: &AtlasRgba,
) -> Result<Texture<'static>, RenderError> {
    let texture = device.create_texture(
        TextureCreateInfo::new()
            .with_type(TextureType::_2D)
            .with_format(TextureFormat::R8g8b8a8Unorm)
            .with_width(atlas.width)
            .with_height(atlas.height)
            .with_layer_count_or_depth(1)
            .with_num_levels(1)
            .with_sample_count(SampleCount::NoMultiSampling)
            .with_usage(TextureUsage::SAMPLER),
    )?;
    let mut map = transfer.map::<u8>(device, true);
    map.mem_mut()[..atlas.rgba.len()].copy_from_slice(&atlas.rgba);
    map.unmap();
    copy_pass.upload_to_gpu_texture(
        TextureTransferInfo::new()
            .with_transfer_buffer(transfer)
            .with_offset(0)
            .with_pixels_per_row(atlas.width)
            .with_rows_per_layer(atlas.height),
        TextureRegion::new()
            .with_texture(&texture)
            .with_mip_level(0)
            .with_layer(0)
            .with_x(0)
            .with_y(0)
            .with_z(0)
            .with_width(atlas.width)
            .with_height(atlas.height)
            .with_depth(1),
        false,
    );
    Ok(texture)
}
