//! Optional HUD text for interactive `run`.

use mmd_engine::runtime::FrameStats;

/// Format overlay lines: backend, count, total/sim/upload ms.
pub fn format_overlay(
    backend: &str,
    agent_count: usize,
    tick_index: u64,
    paused: bool,
    stats: FrameStats,
) -> String {
    let pause = if paused { " paused" } else { "" };
    format!(
        "backend={backend} agents={agent_count} tick={tick_index}{pause}\n\
         frame total={:.3}ms sim={:.3}ms upload={:.3}ms",
        stats.total_ms, stats.sim_ms, stats.upload_ms
    )
}
