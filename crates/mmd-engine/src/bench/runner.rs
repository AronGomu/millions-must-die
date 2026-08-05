//! Offscreen bench loop: 1 sim tick / frame, 2-deep fence queue, scale curve.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::alloc_guard::MeasureGuard;
use crate::render::{DrawGroup, FRAMES_IN_FLIGHT, SpriteRenderer};
use crate::runtime::{Runtime, RuntimeError};
use crate::version;
use crate::workspace_root;

use super::fence_queue::{CompletedFrame, FenceQueue, InflightFrame};
use super::policy::BenchPolicy;
use super::report::{
    BenchmarkReport, ReportManifests, ScaleResult, WorkloadIdentity, build_report,
    build_scale_result, trial_report,
};
use super::stats::{SampleBuffer, TrialAggregate, TrialPercentiles, median};

/// Bench runner failures.
#[derive(Debug, Error)]
pub enum BenchError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Render(#[from] crate::render::RenderError),
    #[error("io: {0}")]
    Io(String),
    #[error("fence queue: {0}")]
    FenceQueue(String),
    #[error("scenario hash file missing: {0}")]
    MissingHash(String),
}

/// Options for a bench invocation.
#[derive(Debug, Clone)]
pub struct BenchOptions {
    pub policy: BenchPolicy,
    pub scenario: PathBuf,
    pub output: Option<PathBuf>,
    /// Skip real GPU; synthesize frame timings (unit / dry). Not production gate.
    pub dry_cpu_only: bool,
    /// Force a Rust heap alloc inside each measured frame (gate proof).
    pub inject_frame_alloc: bool,
}

impl Default for BenchOptions {
    fn default() -> Self {
        Self {
            policy: BenchPolicy::production(),
            scenario: default_scenario_path(),
            output: None,
            dry_cpu_only: false,
            inject_frame_alloc: false,
        }
    }
}

pub fn default_scenario_path() -> PathBuf {
    workspace_root().join("assets/scenarios/technical_prototype_v1.ron")
}

/// Run full scale curve; return report.
pub fn run_bench(opts: BenchOptions) -> Result<BenchmarkReport, BenchError> {
    let policy = opts.policy;
    let scenario_path = opts.scenario;
    let scenario_sha = read_sidecar_sha256(&scenario_path)?;
    let scenario_version = {
        let rt = Runtime::load(&scenario_path, Some(1_000))?;
        rt.scenario_version().to_string()
    };
    let root = workspace_root_or_cwd();
    let atlas_manifest_sha = sha256_file(&root.join("assets/sprites/generated/manifest.json"))?;

    let mut gpu: Option<SpriteRenderer> = None;
    let (backend, adapter) = if opts.dry_cpu_only {
        ("dry".into(), "none".into())
    } else {
        let renderer = SpriteRenderer::new(&root, false)?;
        let backend = renderer.backend().to_string();
        let adapter = renderer.ctx.adapter.clone();
        gpu = Some(renderer);
        (backend, adapter)
    };

    let manifests = ReportManifests::new(
        WorkloadIdentity::new(scenario_version, scenario_sha, atlas_manifest_sha),
        backend,
        adapter,
        1,
        version(),
        &policy,
    );

    let mut scale_results = Vec::with_capacity(policy.scale_counts.len());
    for &count in &policy.scale_counts {
        let result = match gpu.as_mut() {
            Some(r) => run_scale_point(
                &policy,
                &scenario_path,
                count,
                Some(r),
                false,
                opts.inject_frame_alloc,
            )?,
            None => run_scale_point(
                &policy,
                &scenario_path,
                count,
                None,
                true,
                opts.inject_frame_alloc,
            )?,
        };
        scale_results.push(result);
    }

    let report = build_report(&policy, manifests, scale_results);

    if let Some(path) = opts.output.as_ref() {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| BenchError::Io(e.to_string()))?;
        }
        let json = report
            .to_json_pretty()
            .map_err(|e| BenchError::Io(e.to_string()))?;
        fs::write(path, json).map_err(|e| BenchError::Io(e.to_string()))?;
    }

    Ok(report)
}

fn run_scale_point(
    policy: &BenchPolicy,
    scenario_path: &Path,
    agent_count: u32,
    mut renderer: Option<&mut SpriteRenderer>,
    dry: bool,
    inject_frame_alloc: bool,
) -> Result<ScaleResult, BenchError> {
    let mut runtime = Runtime::load(scenario_path, Some(agent_count))?;
    assert!(!runtime.paused());

    let mut queue: FenceQueue<BenchFence> = FenceQueue::new(policy.frames_in_flight);
    assert_eq!(policy.frames_in_flight, FRAMES_IN_FLIGHT);

    // Warmup: allocations allowed (buffer growth, first GPU paths, etc.).
    let warmup_cap = estimate_frame_cap(policy.warmup, policy.min_frames_warmup);
    queue.reserve_latency_samples(warmup_cap.saturating_mul(2));
    run_phase(
        &mut runtime,
        renderer.as_deref_mut(),
        &mut queue,
        policy.warmup,
        policy.min_frames_warmup,
        dry,
        false,
        inject_frame_alloc,
        &mut SampleBuffer::with_capacity(warmup_cap),
        &mut Vec::with_capacity(policy.frames_in_flight),
    )?;

    let mut trial_rows = Vec::with_capacity(policy.trial_count as usize);
    let mut trial_pcts = Vec::with_capacity(policy.trial_count as usize);
    let mut project_rust_alloc_count = 0u64;
    let trial_cap = estimate_frame_cap(policy.trial_duration, policy.min_frames_trial);
    // Latency samples across all trials + backpressure completes.
    queue.reserve_latency_samples(
        trial_cap
            .saturating_mul(policy.trial_count as usize)
            .saturating_mul(2)
            .saturating_add(64),
    );
    let mut poll_scratch = Vec::with_capacity(policy.frames_in_flight);

    for i in 0..policy.trial_count {
        let mut samples = SampleBuffer::with_capacity(trial_cap);
        // Measured trial frames: project Rust allocs must stay 0.
        let guard = MeasureGuard::enter();
        run_phase(
            &mut runtime,
            renderer.as_deref_mut(),
            &mut queue,
            policy.trial_duration,
            policy.min_frames_trial,
            dry,
            true,
            inject_frame_alloc,
            &mut samples,
            &mut poll_scratch,
        )?;
        project_rust_alloc_count = project_rust_alloc_count.saturating_add(guard.finish());

        let tp = samples.trial_frame_service();
        trial_pcts.push(tp);
        trial_rows.push(trial_report(
            i,
            &tp,
            median(&samples.sim_ms),
            median(&samples.upload_ms),
            median(&samples.gpu_queue_latency_ms),
        ));
    }

    let drain_t0 = Instant::now();
    drain_queue(&mut queue, renderer, dry)?;
    let final_drain_ms = drain_t0.elapsed().as_secs_f64() * 1000.0;

    let agg = TrialAggregate::from_trials(&trial_pcts);
    Ok(build_scale_result(
        policy,
        agent_count,
        trial_rows,
        &agg,
        final_drain_ms,
        queue.submitted(),
        queue.completed(),
        queue.max_observed_in_flight(),
        project_rust_alloc_count,
    ))
}

/// Headroom for sample/latency reserves (duration × 120 fps + min + pad).
fn estimate_frame_cap(duration: Duration, min_frames: u32) -> usize {
    let from_dur = (duration.as_secs_f64() * 120.0).ceil() as usize;
    from_dur.max(min_frames as usize).saturating_add(32).max(16)
}

enum BenchFence {
    Real(sdl3::gpu::Fence),
    Dry { ready_at: Instant },
}

fn run_phase(
    runtime: &mut Runtime,
    mut renderer: Option<&mut SpriteRenderer>,
    queue: &mut FenceQueue<BenchFence>,
    duration: Duration,
    min_frames: u32,
    dry: bool,
    record: bool,
    inject_frame_alloc: bool,
    samples: &mut SampleBuffer,
    poll_scratch: &mut Vec<CompletedFrame>,
) -> Result<(), BenchError> {
    let deadline = Instant::now() + duration;
    let mut frames = 0u32;
    // Duration floor + optional min frame count (test policy).
    while Instant::now() < deadline || frames < min_frames {
        // Frame service starts before oldest-fence backpressure / sim.
        let frame_t0 = Instant::now();

        let bp_ms = apply_backpressure(queue, renderer.as_deref_mut(), dry);
        let _begin = queue.begin_after_backpressure(bp_ms);

        if inject_frame_alloc && record {
            // Deliberate heap touch so the zero-alloc gate can hard-fail.
            let forced = vec![frames as u8; 64];
            std::hint::black_box(forced);
        }

        let out = runtime.tick_and_render();
        let sim_ms = out.stats.sim_ms;
        let upload_ms = out.stats.upload_ms;
        let agent_count = out.agent_count;

        let submit_at = Instant::now();
        let fence = submit_frame(renderer.as_deref_mut(), dry, out.groups, agent_count)?;
        queue
            .submit(fence, submit_at)
            .map_err(|e| BenchError::FenceQueue(e.to_string()))?;

        let frame_service_ms = frame_t0.elapsed().as_secs_f64() * 1000.0;
        poll_ready_into(queue, renderer.as_deref(), poll_scratch);
        if record {
            samples.push_frame(frame_service_ms, sim_ms, upload_ms);
            for c in poll_scratch.iter() {
                samples.push_queue_latency(c.gpu_queue_latency_ms);
            }
        }
        frames += 1;
    }
    Ok(())
}

fn apply_backpressure(
    queue: &mut FenceQueue<BenchFence>,
    renderer: Option<&mut SpriteRenderer>,
    dry: bool,
) -> f64 {
    let Some(oldest) = queue.take_oldest_if_full() else {
        return 0.0;
    };
    let t0 = Instant::now();
    let InflightFrame {
        fence,
        submit_at,
        frame_index,
    } = oldest;
    wait_fence(fence, renderer, dry);
    let done_at = Instant::now();
    let _ = queue.complete_waited(
        InflightFrame {
            fence: BenchFence::Dry { ready_at: done_at },
            submit_at,
            frame_index,
        },
        done_at,
    );
    done_at.duration_since(t0).as_secs_f64() * 1000.0
}

fn submit_frame(
    renderer: Option<&mut SpriteRenderer>,
    dry: bool,
    groups: &[DrawGroup],
    agent_count: usize,
) -> Result<BenchFence, BenchError> {
    if dry {
        // Tiny synthetic cost scaled lightly by count (structure smoke only).
        let spin_us = 20 + (agent_count as u64 / 5_000);
        std::thread::sleep(Duration::from_micros(spin_us.min(200)));
        return Ok(BenchFence::Dry {
            ready_at: Instant::now() + Duration::from_micros(50),
        });
    }
    let r = renderer.expect("renderer required when not dry");
    let f = r.draw_offscreen_acquire_fence(groups)?;
    Ok(BenchFence::Real(f))
}

fn poll_ready_into(
    queue: &mut FenceQueue<BenchFence>,
    renderer: Option<&SpriteRenderer>,
    out: &mut Vec<CompletedFrame>,
) {
    let now = Instant::now();
    let device = renderer.map(|r| &r.ctx.device);
    queue.poll_ready_into(
        |f| match f {
            BenchFence::Real(fence) => device.map(|d| fence.query(d)).unwrap_or(false),
            BenchFence::Dry { ready_at } => now >= *ready_at,
        },
        now,
        out,
    );
}

fn drain_queue(
    queue: &mut FenceQueue<BenchFence>,
    mut renderer: Option<&mut SpriteRenderer>,
    dry: bool,
) -> Result<(), BenchError> {
    let pending = queue.take_all_pending();
    for frame in pending {
        let InflightFrame {
            fence,
            submit_at,
            frame_index,
        } = frame;
        wait_fence(fence, renderer.as_deref_mut(), dry);
        let done_at = Instant::now();
        let _ = queue.complete_waited(
            InflightFrame {
                fence: BenchFence::Dry { ready_at: done_at },
                submit_at,
                frame_index,
            },
            done_at,
        );
    }
    queue
        .finish_drain()
        .map_err(|e| BenchError::FenceQueue(e.to_string()))
}

fn wait_fence(fence: BenchFence, renderer: Option<&mut SpriteRenderer>, dry: bool) {
    match fence {
        BenchFence::Dry { ready_at } => {
            let now = Instant::now();
            if now < ready_at {
                std::thread::sleep(ready_at - now);
            }
        }
        BenchFence::Real(f) => {
            if dry {
                return;
            }
            if let Some(r) = renderer {
                let _ = r.ctx.device.wait_fences(true, &[f]);
            }
        }
    }
}

fn read_sidecar_sha256(scenario_path: &Path) -> Result<String, BenchError> {
    let side = scenario_path.with_extension("sha256");
    if side.is_file() {
        let s = fs::read_to_string(&side).map_err(|e| BenchError::Io(e.to_string()))?;
        return Ok(s.trim().split_whitespace().next().unwrap_or("").to_string());
    }
    sha256_file(scenario_path)
}

fn sha256_file(path: &Path) -> Result<String, BenchError> {
    let bytes = fs::read(path).map_err(|e| BenchError::Io(e.to_string()))?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

fn workspace_root_or_cwd() -> PathBuf {
    let root = workspace_root();
    if root.join("assets/sprites/generated/atlas_0.png").is_file() {
        root
    } else {
        std::env::current_dir().unwrap_or(root)
    }
}

/// Synthetic scale result helper for policy unit tests (no GPU).
pub fn synthetic_scale_from_trial_p99s(
    policy: &BenchPolicy,
    agent_count: u32,
    trial_p95: [f64; 7],
    trial_p99: [f64; 7],
) -> ScaleResult {
    let trials: Vec<TrialPercentiles> = (0..7)
        .map(|i| TrialPercentiles {
            p95_ms: trial_p95[i],
            p99_ms: trial_p99[i],
            sample_count: 100,
        })
        .collect();
    let agg = TrialAggregate::from_trials(&trials);
    let rows: Vec<_> = trials
        .iter()
        .enumerate()
        .map(|(i, t)| trial_report(i as u32, t, 1.0, 1.0, 1.0))
        .collect();
    build_scale_result(policy, agent_count, rows, &agg, 0.5, 100, 100, 2, 0)
}
