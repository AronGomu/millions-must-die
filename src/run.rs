//! Interactive `run` entry: static Vulkan sprite slice (T7).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mmd_engine::render::SpriteRenderer;
use mmd_engine::workspace_root;
use sdl3::event::Event;
use sdl3::keyboard::Keycode;

/// Default visible window size (offscreen gate stays 1920×1080).
const WINDOW_W: u32 = 1280;
const WINDOW_H: u32 = 720;

/// Run static prototype: offscreen validate + optional visible swapchain.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root_or_cwd();
    let mut renderer = SpriteRenderer::new(&root, true)?;
    println!(
        "run: backend={} view=1920x1080 atlases=4 (engine {})",
        renderer.backend(),
        mmd_engine::version()
    );

    let groups = SpriteRenderer::static_demo_groups();
    let rb = renderer.draw_offscreen_readback(&groups)?;
    println!(
        "run: offscreen readback {}x{} ok ({} bytes)",
        rb.width,
        rb.height,
        rb.rgba.len()
    );

    // Visible path when a real display is available.
    let window = match renderer
        .ctx
        .video
        .window("millions_must_die — T7 static slice", WINDOW_W, WINDOW_H)
        .position_centered()
        .build()
    {
        Ok(w) => w,
        Err(e) => {
            eprintln!("run: window create failed ({e}); offscreen-only path complete");
            return Ok(());
        }
    };

    renderer.ctx.claim_window(&window)?;
    println!(
        "run: window {}x{} claimed; Esc/quit to exit",
        WINDOW_W, WINDOW_H
    );

    let mut pump = renderer.ctx.sdl.event_pump()?;
    let start = Instant::now();
    let min_visible = Duration::from_millis(250);
    'running: loop {
        for event in pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => break 'running,
                _ => {}
            }
        }
        renderer.draw_to_swapchain(&window, &groups)?;
        // Auto-exit after short visible period when MMD_RUN_ONCE=1 (CI/smoke).
        if std::env::var_os("MMD_RUN_ONCE").is_some() && start.elapsed() >= min_visible {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }

    println!("run: clean exit (backend={})", renderer.backend());
    Ok(())
}

fn workspace_root_or_cwd() -> PathBuf {
    let root = workspace_root();
    if root.join("assets/sprites/generated/atlas_0.png").is_file() {
        root
    } else {
        std::env::current_dir().unwrap_or(root)
    }
}
