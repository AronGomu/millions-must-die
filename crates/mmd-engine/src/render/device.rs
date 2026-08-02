//! SDL3 GPU device ownership (host-forced backend).

use sdl3::gpu::Device;
use sdl3::properties::{Properties, Setter};
use sdl3::video::Window;
use sdl3::{Sdl, VideoSubsystem};

use super::RenderError;
#[cfg(target_os = "macos")]
use super::backend::validate_macos_host_arch;
use super::backend::{
    REQUIRED_BACKEND, assert_device_backend, validate_adapter_name, validate_backend_name,
};
use super::unsafe_sys;

/// Owned SDL context + host-forced GPU device.
///
/// Drop order: `device` first, then `video`, then `sdl` (declaration order).
pub struct GpuContext {
    pub device: Device,
    pub backend: String,
    /// Adapter name from `SDL.gpu.device.name` (may be empty on some ICDs).
    pub adapter: String,
    /// Keep video subsystem alive (drop would SDL_QuitSubSystem video).
    pub video: VideoSubsystem,
    /// Keep SDL alive last.
    pub sdl: Sdl,
}

impl GpuContext {
    /// Create host-required GPU device (Vulkan/Linux, D3D12/Windows, Metal/macOS).
    pub fn new(debug_mode: bool) -> Result<Self, RenderError> {
        validate_backend_name(REQUIRED_BACKEND)?;
        // Portable arch gate also runs on macOS before SDL init (compile-time aarch64 + runtime).
        #[cfg(target_os = "macos")]
        {
            validate_macos_host_arch(std::env::consts::ARCH)?;
        }

        // Headless hosts: offscreen video driver (GPU backend still forced).
        // macOS has no DISPLAY/WAYLAND; leave default video unless caller set SDL_VIDEODRIVER.
        if std::env::var_os("SDL_VIDEODRIVER").is_none()
            && std::env::var_os("DISPLAY").is_none()
            && std::env::var_os("WAYLAND_DISPLAY").is_none()
            && !cfg!(target_os = "macos")
            && !cfg!(target_os = "windows")
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

        let device = create_host_device(debug_mode)?;
        let backend = unsafe_sys::device_driver_name(&device);
        assert_device_backend(&backend)?;
        let adapter = unsafe_sys::device_adapter_name(&device);
        validate_adapter_name(&adapter)?;

        Ok(Self {
            device,
            backend,
            adapter,
            video,
            sdl,
        })
    }

    /// Linux alias: same as [`Self::new`].
    #[deprecated(note = "use GpuContext::new; backend is host-selected")]
    pub fn new_vulkan(debug_mode: bool) -> Result<Self, RenderError> {
        Self::new(debug_mode)
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

fn create_host_device(debug_mode: bool) -> Result<Device, RenderError> {
    #[cfg(target_os = "linux")]
    {
        create_device_linux_vulkan(debug_mode)
    }
    #[cfg(target_os = "windows")]
    {
        create_device_windows_d3d12(debug_mode)
    }
    #[cfg(target_os = "macos")]
    {
        create_device_macos_metal(debug_mode)
    }
}

#[cfg(target_os = "linux")]
fn create_device_linux_vulkan(debug_mode: bool) -> Result<Device, RenderError> {
    let props = Properties::new()?;
    props.set("SDL.gpu.device.create.debugmode", debug_mode)?;
    props.set("SDL.gpu.device.create.name", REQUIRED_BACKEND)?;
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
            props.set("SDL.gpu.device.create.name", REQUIRED_BACKEND)?;
            props.set("SDL.gpu.device.create.shaders.spirv", true)?;
            Device::new_with_properties(props).map_err(|e| {
                RenderError::Sdl(format!(
                    "vulkan device failed (hw={hard_err}; soft={e}). Check LD_LIBRARY_PATH has libvulkan.so + ICD"
                ))
            })
        }
    }
}

/// Force D3D12 + DXIL. Rejects non-D3D12 selection at props + post-create assert.
#[cfg(target_os = "windows")]
fn create_device_windows_d3d12(debug_mode: bool) -> Result<Device, RenderError> {
    let props = Properties::new()?;
    props.set("SDL.gpu.device.create.debugmode", debug_mode)?;
    props.set("SDL.gpu.device.create.name", REQUIRED_BACKEND)?;
    props.set("SDL.gpu.device.create.shaders.dxil", true)?;
    // Prefer discrete / high-performance adapter when driver offers the choice.
    props.set("SDL.gpu.device.create.preferlowpower", false)?;
    Device::new_with_properties(props).map_err(|e| {
        RenderError::Sdl(format!(
            "direct3d12 device failed ({e}). Need Windows 11 + D3D12 GPU (not Basic Render Driver); SDL3.dll on PATH"
        ))
    })
}

/// Force Metal + metallib. No Vulkan/MoltenVK props. Apple Silicon only (caller + compile gate).
#[cfg(target_os = "macos")]
fn create_device_macos_metal(debug_mode: bool) -> Result<Device, RenderError> {
    let props = Properties::new()?;
    props.set("SDL.gpu.device.create.debugmode", debug_mode)?;
    // Explicit driver name — never fall through to MoltenVK/Vulkan.
    props.set("SDL.gpu.device.create.name", REQUIRED_BACKEND)?;
    props.set("SDL.gpu.device.create.shaders.metallib", true)?;
    // Do not enable SPIR-V on macOS (would invite MoltenVK).
    props.set("SDL.gpu.device.create.shaders.spirv", false)?;
    props.set("SDL.gpu.device.create.preferlowpower", false)?;
    Device::new_with_properties(props).map_err(|e| {
        RenderError::Sdl(format!(
            "metal device failed ({e}). Need macOS 15 arm64 + Metal + native metallib; no MoltenVK. See docs/platform/macos-bootstrap.md"
        ))
    })
}
