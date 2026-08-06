//! Narrow `sdl3-sys` escape hatch.
//!
//! Command buffers are submitted via raw SDL and then `mem::forget` so the safe
//! wrapper does not double-free; clippy `forget_non_drop` is expected here.
//!
//! # Invariants
//!
//! 1. Callers hold a live `sdl3::gpu::Device` / `CopyPass` / `TransferBuffer` /
//!    `Texture` for every raw pointer passed here. Pointers must not outlive
//!    those RAII owners.
//! 2. `download_texture` may only run inside an active copy pass on the same
//!    device that owns `texture` and `transfer`.
//! 3. After download, caller must submit with a fence and wait before mapping
//!    the transfer buffer.
//! 4. `present_clear` / `present_blit` consume the command buffer (submit or
//!    cancel). Caller must not use `cmd` after these return.
//! 5. No other module may call `sdl3_sys` GPU entry points directly.

#![allow(clippy::forget_non_drop)]
#![allow(clippy::field_reassign_with_default)]

use std::ffi::CStr;
use std::ptr;

use sdl3::gpu::{CommandBuffer, CopyPass, Device, Texture, TransferBuffer};
use sdl3::properties::Properties;
use sdl3::video::Window;
use sdl3_sys::gpu::{
    SDL_BeginGPURenderPass, SDL_BlitGPUTexture, SDL_CancelGPUCommandBuffer,
    SDL_DownloadFromGPUTexture, SDL_EndGPURenderPass, SDL_GPU_FILTER_NEAREST, SDL_GPU_LOADOP_CLEAR,
    SDL_GPU_STOREOP_STORE, SDL_GPUBlitInfo, SDL_GPUBlitRegion, SDL_GPUColorTargetInfo,
    SDL_GPUDevice, SDL_GPUFence, SDL_GPUTextureRegion, SDL_GPUTextureTransferInfo,
    SDL_GetGPUDeviceDriver, SDL_GetGPUDeviceProperties, SDL_QueryGPUFence, SDL_ReleaseGPUFence,
    SDL_ReleaseWindowFromGPUDevice, SDL_SubmitGPUCommandBuffer,
    SDL_SubmitGPUCommandBufferAndAcquireFence, SDL_WaitAndAcquireGPUSwapchainTexture,
    SDL_WaitForGPUFences, SDL_WaitForGPUIdle,
};
use sdl3_sys::pixels::SDL_FColor;
use sdl3_sys::surface::SDL_FLIP_NONE;

/// Raw per-frame GPU fence, released on drop via C call only.
///
/// Why: sdl3 0.18.4's safe `Fence` wraps `Arc<FenceContainer>` — `Arc::new`
/// is one project-Rust heap allocation inside every measured bench frame,
/// which trips the zero-alloc gate (limit 0). `Device::wait_fences` further
/// collects a `Vec` of raw handles per call. This raw handle keeps the
/// measured loop allocation-free: query/wait/release are plain SDL C calls.
///
/// # Safety invariants
/// - The owning `Device` outlives every `RawFrameFence` created from it
///   (bench drains its fence queue before renderer teardown).
/// - Single-threaded use on the bench loop.
#[derive(Debug)]
pub struct RawFrameFence {
    fence: *mut SDL_GPUFence,
    device: *mut SDL_GPUDevice,
}

impl RawFrameFence {
    /// Non-blocking completion query (`SDL_QueryGPUFence`).
    pub fn query(&self) -> bool {
        unsafe { SDL_QueryGPUFence(self.device, self.fence) }
    }

    /// Block until the fence signals (`SDL_WaitForGPUFences`, single handle,
    /// stack storage — no heap use).
    pub fn wait(&self) {
        unsafe {
            SDL_WaitForGPUFences(self.device, true, &self.fence, 1);
        }
    }
}

impl Drop for RawFrameFence {
    fn drop(&mut self) {
        unsafe {
            SDL_ReleaseGPUFence(self.device, self.fence);
        }
    }
}

/// Submit `cmd` and acquire a raw fence (consumes `cmd`; no Rust heap use on
/// the success path).
///
/// # Safety invariants
/// - `cmd` was acquired from `device` and is fully recorded (all passes ended).
/// - On return, `cmd` is consumed; do not reuse.
/// - Returned fence must not outlive `device` (see `RawFrameFence`).
pub fn submit_acquire_raw_fence(
    device: &Device,
    cmd: CommandBuffer,
) -> Result<RawFrameFence, String> {
    let raw = unsafe { SDL_SubmitGPUCommandBufferAndAcquireFence(cmd.raw()) };
    std::mem::forget(cmd);
    if raw.is_null() {
        return Err("SubmitGPUCommandBufferAndAcquireFence failed".into());
    }
    Ok(RawFrameFence {
        fence: raw,
        device: device.raw(),
    })
}

/// Read SDL GPU driver name for `device` (`"vulkan"`, `"direct3d12"`, `"metal"`).
///
/// # Safety invariants
/// `device` must be a live `Device` from this process.
pub fn device_driver_name(device: &Device) -> String {
    unsafe {
        let ptr = SDL_GetGPUDeviceDriver(device.raw());
        if ptr.is_null() {
            return String::new();
        }
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// Read adapter name from device properties (`SDL.gpu.device.name`).
///
/// Used to reject Microsoft Basic Render Driver. Empty string when unavailable.
///
/// # Safety invariants
/// `device` must be a live `Device` from this process. Properties ID is owned by
/// SDL device; wrap as constant so Drop does not destroy it.
pub fn device_adapter_name(device: &Device) -> String {
    unsafe {
        let props_id = SDL_GetGPUDeviceProperties(device.raw());
        if props_id == 0 {
            return String::new();
        }
        let props = Properties::const_from_ll(props_id);
        props
            .get_string("SDL.gpu.device.name", "")
            .unwrap_or_default()
    }
}

/// Release `window` from `device`, tearing down its swapchain.
///
/// The safe sdl3 0.18.4 wrapper has no counterpart to `with_window`: claiming
/// registers the window inside the device and *destroying the window while it
/// is still claimed leaves the device holding a dangling swapchain*, which
/// faults on the next device call. Every claim must be paired with this before
/// the window is dropped.
///
/// Idempotent per SDL: releasing an unclaimed window is a no-op.
///
/// Drains the device first (`SDL_WaitForGPUIdle`). The present path submits
/// without acquiring a fence, so at the call site there is generally still work
/// referencing the swapchain; SDL does not document a wait inside
/// `SDL_ReleaseWindowFromGPUDevice`, so this function owns that ordering
/// rather than leaving it as a precondition no caller could satisfy.
///
/// # Safety invariants
/// - `device` and `window` are live and from this process.
/// - Called from the thread that created `window` (SDL requirement).
pub fn release_window(device: &Device, window: &Window) {
    unsafe {
        SDL_WaitForGPUIdle(device.raw());
        SDL_ReleaseWindowFromGPUDevice(device.raw(), window.raw());
    }
}

/// Enqueue GPU→CPU texture download into `transfer` (full 2D level 0).
///
/// # Safety invariants
/// - `copy_pass` is the active pass from `device.begin_copy_pass`.
/// - `texture` created on same device; usage includes color target (or sampler).
/// - `transfer` is `TransferBufferUsage::DOWNLOAD` and large enough for
///   `width * height * 4` bytes (RGBA8 tightly packed).
/// - Do not map `transfer` until the enclosing command buffer fence signals.
pub fn download_texture(
    copy_pass: &CopyPass,
    texture: &Texture,
    width: u32,
    height: u32,
    transfer: &TransferBuffer,
) {
    let source = SDL_GPUTextureRegion {
        texture: texture.raw(),
        mip_level: 0,
        layer: 0,
        x: 0,
        y: 0,
        z: 0,
        w: width,
        h: height,
        d: 1,
    };
    let destination = SDL_GPUTextureTransferInfo {
        transfer_buffer: transfer.raw(),
        offset: 0,
        pixels_per_row: width,
        rows_per_layer: height,
    };
    unsafe {
        SDL_DownloadFromGPUTexture(copy_pass.raw(), &source, &destination);
    }
}

/// Acquire swapchain, clear to `rgb`, submit. Works around sdl3 lifetime clash
/// (`Texture<'a>` tied to `&mut CommandBuffer` vs `begin_render_pass(&cmd)`).
///
/// # Safety invariants
/// - `device` claimed `window` via `ClaimWindowForGPUDevice`.
/// - `cmd` freshly acquired and not otherwise recorded.
/// - On return, `cmd` is consumed (submitted or cancelled); do not reuse.
#[allow(dead_code)] // kept for clear-only fallback / non-blit hosts
pub fn present_clear(
    device: &Device,
    window: &Window,
    cmd: CommandBuffer,
    r: f32,
    g: f32,
    b: f32,
) -> Result<(), String> {
    let _ = device;
    let mut swapchain = ptr::null_mut();
    let mut width = 0u32;
    let mut height = 0u32;
    let ok = unsafe {
        SDL_WaitAndAcquireGPUSwapchainTexture(
            cmd.raw(),
            window.raw(),
            &mut swapchain,
            &mut width,
            &mut height,
        )
    };
    if !ok || swapchain.is_null() {
        unsafe {
            SDL_CancelGPUCommandBuffer(cmd.raw());
        }
        std::mem::forget(cmd);
        return Ok(());
    }

    let mut info = SDL_GPUColorTargetInfo::default();
    info.texture = swapchain;
    info.clear_color = SDL_FColor { r, g, b, a: 1.0 };
    info.load_op = SDL_GPU_LOADOP_CLEAR;
    info.store_op = SDL_GPU_STOREOP_STORE;
    info.cycle = false;

    let pass = unsafe { SDL_BeginGPURenderPass(cmd.raw(), &info, 1, ptr::null()) };
    if pass.is_null() {
        unsafe {
            SDL_CancelGPUCommandBuffer(cmd.raw());
        }
        std::mem::forget(cmd);
        return Err("BeginGPURenderPass failed".into());
    }
    unsafe {
        SDL_EndGPURenderPass(pass);
        if !SDL_SubmitGPUCommandBuffer(cmd.raw()) {
            std::mem::forget(cmd);
            return Err("SubmitGPUCommandBuffer failed".into());
        }
    }
    std::mem::forget(cmd);
    let _ = (width, height);
    Ok(())
}

/// Acquire swapchain, blit `source` (full `src_w`×`src_h`) into it, submit.
///
/// Scales with nearest filter when swapchain size ≠ source. Must not run inside
/// another pass. `source` needs `SAMPLER` usage.
///
/// # Safety invariants
/// - `device` claimed `window` via `ClaimWindowForGPUDevice`.
/// - `cmd` freshly acquired; not otherwise recorded.
/// - `source` live texture on same device; usage includes sampler.
/// - On return, `cmd` is consumed; do not reuse.
pub fn present_blit(
    device: &Device,
    window: &Window,
    cmd: CommandBuffer,
    source: &Texture,
    src_w: u32,
    src_h: u32,
) -> Result<(), String> {
    let _ = device;
    let mut swapchain = ptr::null_mut();
    let mut width = 0u32;
    let mut height = 0u32;
    let ok = unsafe {
        SDL_WaitAndAcquireGPUSwapchainTexture(
            cmd.raw(),
            window.raw(),
            &mut swapchain,
            &mut width,
            &mut height,
        )
    };
    if !ok || swapchain.is_null() {
        unsafe {
            SDL_CancelGPUCommandBuffer(cmd.raw());
        }
        std::mem::forget(cmd);
        return Ok(());
    }

    let info = SDL_GPUBlitInfo {
        source: SDL_GPUBlitRegion {
            texture: source.raw(),
            mip_level: 0,
            layer_or_depth_plane: 0,
            x: 0,
            y: 0,
            w: src_w,
            h: src_h,
        },
        destination: SDL_GPUBlitRegion {
            texture: swapchain,
            mip_level: 0,
            layer_or_depth_plane: 0,
            x: 0,
            y: 0,
            w: width,
            h: height,
        },
        load_op: SDL_GPU_LOADOP_CLEAR,
        clear_color: SDL_FColor {
            r: 12.0 / 255.0,
            g: 16.0 / 255.0,
            b: 28.0 / 255.0,
            a: 1.0,
        },
        flip_mode: SDL_FLIP_NONE,
        filter: SDL_GPU_FILTER_NEAREST,
        cycle: false,
        ..Default::default()
    };

    unsafe {
        SDL_BlitGPUTexture(cmd.raw(), &info);
        if !SDL_SubmitGPUCommandBuffer(cmd.raw()) {
            std::mem::forget(cmd);
            return Err("SubmitGPUCommandBuffer failed".into());
        }
    }
    std::mem::forget(cmd);
    Ok(())
}
