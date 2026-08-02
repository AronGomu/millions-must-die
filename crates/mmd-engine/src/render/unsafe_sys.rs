//! Narrow `sdl3-sys` escape hatch.
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
//! 4. `present_clear` consumes the command buffer (submit or cancel). Caller
//!    must not use `cmd` after this returns.
//! 5. No other module may call `sdl3_sys` GPU entry points directly.

use std::ffi::CStr;
use std::ptr;

use sdl3::gpu::{CommandBuffer, CopyPass, Device, Texture, TransferBuffer};
use sdl3::video::Window;
use sdl3_sys::gpu::{
    SDL_BeginGPURenderPass, SDL_CancelGPUCommandBuffer, SDL_DownloadFromGPUTexture,
    SDL_EndGPURenderPass, SDL_GPU_LOADOP_CLEAR, SDL_GPU_STOREOP_STORE, SDL_GPUColorTargetInfo,
    SDL_GPUTextureRegion, SDL_GPUTextureTransferInfo, SDL_GetGPUDeviceDriver,
    SDL_SubmitGPUCommandBuffer, SDL_WaitAndAcquireGPUSwapchainTexture,
};
use sdl3_sys::pixels::SDL_FColor;

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
