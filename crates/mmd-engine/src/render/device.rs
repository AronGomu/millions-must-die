//! SDL3 GPU device ownership (Linux/Vulkan).

use sdl3::gpu::Device;
use sdl3::properties::{Properties, Setter};
use sdl3::video::Window;
use sdl3::{Sdl, VideoSubsystem};

use super::RenderError;
use super::backend::{REQUIRED_LINUX_BACKEND, assert_device_backend, validate_backend_name};
use super::unsafe_sys;

/// Owned SDL context + forced-Vulkan GPU device.
///
/// Drop order: `device` first, then `video`, then `sdl` (declaration order).
pub struct GpuContext {
    pub device: Device,
    pub backend: String,
    /// Keep video subsystem alive (drop would SDL_QuitSubSystem video).
    pub video: VideoSubsystem,
    /// Keep SDL alive last.
    pub sdl: Sdl,
}

impl GpuContext {
    /// Create Vulkan GPU device. Rejects non-Vulkan backends.
    pub fn new_vulkan(debug_mode: bool) -> Result<Self, RenderError> {
        validate_backend_name(REQUIRED_LINUX_BACKEND)?;

        // Headless hosts: offscreen video driver (SDL GPU still uses Vulkan).
        if std::env::var_os("SDL_VIDEODRIVER").is_none()
            && std::env::var_os("DISPLAY").is_none()
            && std::env::var_os("WAYLAND_DISPLAY").is_none()
        {
            let _ = sdl3::hint::set("SDL_VIDEODRIVER", "offscreen");
        }

        let (sdl, video) = match try_init_sdl() {
            Ok(pair) => pair,
            Err(first) => {
                let _ = sdl3::hint::set("SDL_VIDEODRIVER", "offscreen");
                try_init_sdl().map_err(|second| {
                    RenderError::Sdl(format!(
                        "SDL init failed ({first}); offscreen retry ({second})"
                    ))
                })?
            }
        };

        let device = create_device_named_vulkan(debug_mode)?;
        let backend = unsafe_sys::device_driver_name(&device);
        assert_device_backend(&backend)?;

        Ok(Self {
            device,
            backend,
            video,
            sdl,
        })
    }

    /// Claim `window` for swapchain present on this device.
    pub fn claim_window(&self, window: &Window) -> Result<(), RenderError> {
        // `with_window` consumes a Device clone (Arc); claim sticks on shared device.
        let claimed = self.device.clone().with_window(window)?;
        drop(claimed);
        Ok(())
    }
}

fn try_init_sdl() -> Result<(Sdl, VideoSubsystem), String> {
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    Ok((sdl, video))
}

fn create_device_named_vulkan(debug_mode: bool) -> Result<Device, RenderError> {
    let props = Properties::new()?;
    props.set("SDL.gpu.device.create.debugmode", debug_mode)?;
    props.set("SDL.gpu.device.create.name", REQUIRED_LINUX_BACKEND)?;
    props.set("SDL.gpu.device.create.shaders.spirv", true)?;
    // Prefer real GPU. If this fails (e.g. only lavapipe), retry without the flag.
    props.set(
        "SDL.gpu.device.create.vulkan.requirehardwareacceleration",
        true,
    )?;
    match Device::new_with_properties(props) {
        Ok(d) => Ok(d),
        Err(hard_err) => {
            let props = Properties::new()?;
            props.set("SDL.gpu.device.create.debugmode", debug_mode)?;
            props.set("SDL.gpu.device.create.name", REQUIRED_LINUX_BACKEND)?;
            props.set("SDL.gpu.device.create.shaders.spirv", true)?;
            Device::new_with_properties(props).map_err(|e| {
                RenderError::Sdl(format!(
                    "vulkan device failed (hw={hard_err}; soft={e}). Check LD_LIBRARY_PATH has libvulkan.so + ICD"
                ))
            })
        }
    }
}
