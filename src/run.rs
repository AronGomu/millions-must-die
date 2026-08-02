//! Interactive `run` entry: moving 50k flow-field horde (T8).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mmd_engine::render::{SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH};
use mmd_engine::runtime::{InputAction, Runtime};
use mmd_engine::workspace_root;
use sdl3::event::Event;

use crate::input;
use crate::overlay;

/// Fixed camera window matches gate resolution.
const WINDOW_W: u32 = VIEW_WIDTH;
const WINDOW_H: u32 = VIEW_HEIGHT;

/// CLI options for interactive / headless smoke.
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub agents: Option<u32>,
    pub scenario: Option<PathBuf>,
    /// Auto-exit after N frames (CI/smoke). `None` = interactive until quit.
    pub frames: Option<u64>,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            agents: None,
            scenario: None,
            frames: None,
        }
    }
}

/// Run full prototype: scenario → sim → instances → 4 draws.
pub fn run(opts: RunOptions) -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root_or_cwd();
    let scenario_path = opts
        .scenario
        .unwrap_or_else(|| root.join("assets/scenarios/technical_prototype_v1.ron"));

    let mut runtime = Runtime::load(&scenario_path, opts.agents)?;
    let mut renderer = SpriteRenderer::new(&root, true)?;

    println!(
        "run: backend={} view={}x{} agents={} scenario={} (engine {})",
        renderer.backend(),
        VIEW_WIDTH,
        VIEW_HEIGHT,
        runtime.agent_count(),
        scenario_path.display(),
        mmd_engine::version()
    );

    // Headless/offscreen proof: one frame → nonempty groups + checksum move.
    let frame0 = runtime.tick_and_render();
    let hash0 = frame0.state_hash;
    let group_lens: Vec<usize> = frame0.groups.iter().map(|g| g.instances.len()).collect();
    println!(
        "run: frame0 tick={} groups={group_lens:?} sim={:.3}ms upload={:.3}ms",
        frame0.tick_index, frame0.stats.sim_ms, frame0.stats.upload_ms
    );
    if group_lens.iter().any(|&n| n == 0) {
        return Err("empty instance group after first frame".into());
    }

    // Optional offscreen draw (no full readback every smoke — one draw proves GPU path).
    renderer.draw_offscreen(&frame0.groups)?;
    println!("run: offscreen draw ok (backend={})", renderer.backend());

    let frames_limit = opts.frames.or_else(|| {
        std::env::var("MMD_RUN_FRAMES")
            .ok()
            .and_then(|s| s.parse().ok())
    });
    let run_once = std::env::var_os("MMD_RUN_ONCE").is_some();
    let auto_frames = frames_limit.or(if run_once { Some(3) } else { None });

    // Visible path when display can present. Offscreen driver → CPU/GPU tick loop only.
    let offscreen_driver = std::env::var_os("SDL_VIDEODRIVER")
        .map(|v| v == "offscreen")
        .unwrap_or(false);

    let window = if offscreen_driver {
        None
    } else {
        match renderer
            .ctx
            .video
            .window("millions_must_die — T8 moving 50k", WINDOW_W, WINDOW_H)
            .position_centered()
            .build()
        {
            Ok(w) => match renderer.ctx.claim_window(&w) {
                Ok(()) => Some(w),
                Err(e) => {
                    eprintln!("run: claim_window failed ({e}); offscreen-only");
                    None
                }
            },
            Err(e) => {
                eprintln!("run: window create failed ({e}); offscreen-only");
                None
            }
        }
    };

    let Some(window) = window else {
        return finish_offscreen(&mut runtime, &mut renderer, hash0, auto_frames);
    };

    println!(
        "run: window {}x{} claimed; Esc quit, F1 overlay, Space pause",
        WINDOW_W, WINDOW_H
    );

    let mut pump = renderer.ctx.sdl.event_pump()?;
    let mut frame_i = 1u64; // already did frame0
    let mut quit = false;
    let start = Instant::now();

    // Present first frame; fall back if swapchain unsupported.
    if let Err(e) = renderer.draw_to_swapchain(&window, &frame0.groups) {
        eprintln!("run: present failed ({e}); offscreen-only");
        return finish_offscreen(&mut runtime, &mut renderer, hash0, auto_frames);
    }

    'running: loop {
        for event in pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    quit = true;
                    break 'running;
                }
                Event::KeyDown {
                    keycode: Some(kc), ..
                } => {
                    if let Some(action) = input::action_from_keycode(kc) {
                        match action {
                            InputAction::Quit => {
                                quit = true;
                                break 'running;
                            }
                            other => runtime.apply_action(other),
                        }
                    }
                }
                _ => {}
            }
        }

        let frame_start = Instant::now();
        let out = runtime.tick_and_render();
        renderer.draw_to_swapchain(&window, &out.groups)?;
        let gpu_ms = frame_start.elapsed().as_secs_f64() * 1000.0;

        if out.overlay_visible {
            // stdout HUD (no text GPU path in phase-0).
            let mut stats = out.stats;
            stats.total_ms = gpu_ms;
            println!(
                "{}",
                overlay::format_overlay(
                    renderer.backend(),
                    out.agent_count,
                    out.tick_index,
                    out.paused,
                    stats,
                )
            );
        }

        frame_i += 1;
        if let Some(limit) = auto_frames {
            if frame_i >= limit {
                break;
            }
        }

        // Soft pace toward 60 Hz when running interactively without frame cap.
        if auto_frames.is_none() {
            let elapsed = frame_start.elapsed();
            if elapsed < Duration::from_millis(16) {
                std::thread::sleep(Duration::from_millis(16) - elapsed);
            }
        }
    }

    println!(
        "run: clean exit (backend={} tick={} frames={} quit={quit} elapsed={:.2}s)",
        renderer.backend(),
        runtime.tick_index(),
        frame_i,
        start.elapsed().as_secs_f64()
    );
    Ok(())
}

fn finish_offscreen(
    runtime: &mut Runtime,
    renderer: &mut SpriteRenderer,
    hash0: [u8; 32],
    auto_frames: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut last = hash0;
    let n = auto_frames.unwrap_or(3).max(2);
    // frame0 already applied by caller; continue to n total ticks.
    let already = runtime.tick_index().max(1);
    for _ in already..n {
        let f = runtime.tick_and_render();
        renderer.draw_offscreen(&f.groups)?;
        last = f.state_hash;
    }
    if !runtime.paused() && last == hash0 {
        return Err("state hash unchanged across offscreen frames".into());
    }
    println!(
        "run: clean exit offscreen tick={} groups_ok hash_moved={} backend={}",
        runtime.tick_index(),
        last != hash0,
        renderer.backend()
    );
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
