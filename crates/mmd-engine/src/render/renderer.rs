//! Static sprite batch renderer: 4 atlas groups, offscreen readback, swapchain.

use std::ffi::CStr;
use std::path::Path;

use sdl3::gpu::{
    BlendFactor, BlendOp, Buffer, BufferBinding, BufferRegion, BufferUsageFlags,
    ColorTargetBlendState, ColorTargetDescription, ColorTargetInfo, CullMode, Device, FillMode,
    Filter, GraphicsPipeline, GraphicsPipelineTargetInfo, IndexElementSize, LoadOp, PrimitiveType,
    RasterizerState, SampleCount, Sampler, SamplerAddressMode, SamplerCreateInfo,
    SamplerMipmapMode, ShaderFormat, ShaderStage, StoreOp, Texture, TextureCreateInfo,
    TextureFormat, TextureRegion, TextureSamplerBinding, TextureTransferInfo, TextureType,
    TextureUsage, TransferBuffer, TransferBufferLocation, TransferBufferUsage, VertexAttribute,
    VertexBufferDescription, VertexElementFormat, VertexInputRate, VertexInputState,
};
use sdl3::pixels::Color;
use sdl3::video::Window;

use super::RenderError;
use super::atlas::{
    ATLAS_COUNT, ATLAS_HEIGHT_PX, ATLAS_WIDTH_PX, AtlasRgba, SPRITE_SIZE_PX, default_atlas_dir,
    frame_uv_rect, load_atlases,
};
use super::device::GpuContext;
use super::instance::{FrameUniforms, QUAD_INDICES, QUAD_VERTICES, QuadVertex, SpriteInstance};
use super::unsafe_sys;

/// Offscreen / gate resolution.
pub const VIEW_WIDTH: u32 = 1920;
pub const VIEW_HEIGHT: u32 = 1080;
/// Frames-in-flight for cycled instance uploads.
pub const FRAMES_IN_FLIGHT: usize = 2;
/// Max sprites uploaded per frame across all groups (hard 50k; stretch 100k).
pub const MAX_INSTANCES: u32 = 100_000;

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

/// One atlas draw group.
#[derive(Clone, Debug)]
pub struct DrawGroup {
    pub atlas_id: u32,
    pub instances: Vec<SpriteInstance>,
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
    quad_vb: Buffer,
    quad_ib: Buffer,
    instance_bufs: [Buffer; FRAMES_IN_FLIGHT],
    upload_xfer: TransferBuffer,
    download_xfer: TransferBuffer,
    atlases_cpu: [AtlasRgba; ATLAS_COUNT],
    atlas_tex: [Texture<'static>; ATLAS_COUNT],
    sampler: Sampler,
    offscreen: Texture<'static>,
    frame_slot: usize,
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

        let pipeline = device
            .create_graphics_pipeline()
            .with_primitive_type(PrimitiveType::TriangleList)
            .with_vertex_shader(&vert)
            .with_fragment_shader(&frag)
            .with_vertex_input_state(
                VertexInputState::new()
                    .with_vertex_buffer_descriptions(&[
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
                    ])
                    .with_vertex_attributes(&[
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
                    ]),
            )
            .with_rasterizer_state(
                RasterizerState::new()
                    .with_fill_mode(FillMode::Fill)
                    .with_cull_mode(CullMode::None),
            )
            .with_target_info(
                GraphicsPipelineTargetInfo::new().with_color_target_descriptions(&[
                    ColorTargetDescription::new()
                        .with_format(TextureFormat::R8g8b8a8Unorm)
                        .with_blend_state(blend),
                ]),
            )
            .build()?;
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

        let out = Self {
            pipeline,
            quad_vb,
            quad_ib,
            instance_bufs,
            upload_xfer,
            download_xfer,
            atlases_cpu,
            atlas_tex,
            sampler,
            offscreen,
            frame_slot: 0,
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

    /// Upload instances + draw 4 atlas groups into offscreen 1920×1080 (no readback).
    pub fn draw_offscreen(&mut self, groups: &[DrawGroup]) -> Result<(), RenderError> {
        self.validate_groups(groups)?;

        let device = &self.ctx.device;
        let slot = self.frame_slot % FRAMES_IN_FLIGHT;
        self.frame_slot = self.frame_slot.wrapping_add(1);

        // Pack instances contiguously; record per-group ranges.
        let mut packed: Vec<SpriteInstance> = Vec::new();
        let mut ranges: [(u32, u32); ATLAS_COUNT] = [(0, 0); ATLAS_COUNT];
        for (i, g) in groups.iter().enumerate() {
            let start = packed.len() as u32;
            packed.extend_from_slice(&g.instances);
            let count = g.instances.len() as u32;
            ranges[i] = (start, count);
        }
        if packed.len() as u32 > MAX_INSTANCES {
            return Err(RenderError::Sdl(format!(
                "too many instances {}",
                packed.len()
            )));
        }

        // Upload instances (cycled buffer).
        {
            let cmd = device.acquire_command_buffer()?;
            let copy = device.begin_copy_pass(&cmd)?;
            let nbytes = (packed.len() * std::mem::size_of::<SpriteInstance>()) as u32;
            if nbytes > 0 {
                let mut map = self.upload_xfer.map::<SpriteInstance>(device, true);
                map.mem_mut()[..packed.len()].copy_from_slice(&packed);
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

        // Render pass → offscreen.
        {
            let cmd = device.acquire_command_buffer()?;
            let uniforms = FrameUniforms {
                view_size: [VIEW_WIDTH as f32, VIEW_HEIGHT as f32],
                _pad: [0.0, 0.0],
            };
            cmd.push_vertex_uniform_data(0, &uniforms);

            let color_targets = [ColorTargetInfo::default()
                .with_texture(&self.offscreen)
                .with_load_op(LoadOp::CLEAR)
                .with_store_op(StoreOp::STORE)
                .with_clear_color(Color::RGBA(0, 0, 0, 0))];
            let pass = device.begin_render_pass(&cmd, &color_targets, None)?;
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

            for (atlas_i, (start, count)) in ranges.iter().enumerate() {
                if *count == 0 {
                    continue;
                }
                pass.bind_fragment_samplers(
                    0,
                    &[TextureSamplerBinding::new()
                        .with_texture(&self.atlas_tex[atlas_i])
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

            device.end_render_pass(pass);
            cmd.submit()?;
        }

        Ok(())
    }

    /// Draw groups into offscreen 1920×1080 and read RGBA8 pixels back.
    pub fn draw_offscreen_readback(
        &mut self,
        groups: &[DrawGroup],
    ) -> Result<Readback, RenderError> {
        self.draw_offscreen(groups)?;
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
        self.draw_offscreen(groups)?;
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
