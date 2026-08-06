//! T31 — render correctness on the **development host only**.
//!
//! Three layers, in order of how much they assume:
//!
//! 1. **Instance data** (headless): the CPU pack step turns simulation state
//!    into exactly one instance per alive agent, in the right atlas bucket,
//!    with the right animation UVs, centred on the agent's world position.
//! 2. **Projection** (headless mirror + GPU oracle): [`world_to_clip`] states
//!    where a world corner lands in clip space; the GPU raster probe proves the
//!    real shader agrees, so the mirror cannot silently drift from
//!    `shaders/sprite.hlsl`.
//! 3. **Whole frame** (GPU): the committed host golden. Drift fails with a
//!    written diff artifact naming the pixels that moved.
//!
//! # Scope of the golden claim
//!
//! The golden is **host-scoped**: it proves *this* development backend
//! (Linux/Vulkan on the recorded adapter) renders the expected frame. It is
//! not, and must never be read as, a cross-backend or cross-platform claim —
//! `compare_readback` refuses a foreign golden outright rather than diffing it.
//! The comparison is exact (`GOLDEN_MAX_CHANNEL_DELTA_POLICY == 0`); any
//! future tolerance would cover `f32`/driver variation on this one host only.
//!
//! Regenerating the golden is an explicit reviewed step, one command:
//!
//! ```sh
//! MMD_UPDATE_GOLDEN=1 cargo test -p mmd-engine --test gpu_golden -- --ignored update_host_golden
//! ```
//!
//! Inspect the resulting `lab/goldens/<family>/golden.png` diff before
//! committing it — a regenerated golden re-baselines the gate.
//!
//! # Headless hosts
//!
//! Every GPU case obtains its renderer through [`renderer_or_skip`], which
//! *skips* (prints and returns) when the host has no GPU device at all, and
//! *fails* for every other error. A drifted atlas or a rejected software
//! adapter is a defect, not a headless shell.
//!
//! Nothing here reads a clock or asserts on timing (`docs/05-testing.md`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use mmd_engine::render::{
    ATLAS_COUNT, DrawGroup, FRAME_SIZE_PX, GOLDEN_DIFF_ACTUAL_PNG, GOLDEN_DIFF_MASK_PNG,
    GOLDEN_DIFF_SUMMARY_JSON, GOLDEN_MAX_CHANNEL_DELTA_POLICY, GOLDEN_STATUS_CAPTURED,
    GoldenDiffSummary, GoldenError, GoldenManifest, HostBinding, Readback, RenderError,
    SpriteInstance, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH, clip_to_pixel,
    compare_readback_writing_diff, diff_readback, frame_uv_rect, golden_family_dir,
    host_binding_hashes, load_atlases, load_golden_image, load_golden_manifest, world_to_clip,
    write_golden_diff,
};
use mmd_engine::runtime::build_instance_groups;
use mmd_engine::testkit::{FIXTURE_CORRIDOR_V1, FIXTURE_SMALL_V1, Harness};
use mmd_engine::workspace_root;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// SDL GPU state is process-global and the renderer owns a device, so GPU
/// cases in this binary run one at a time regardless of `cargo test`'s default
/// thread fan-out. A poisoned lock (an earlier GPU case panicked) is recovered
/// rather than cascading: the remaining cases still deserve to report their
/// own verdict.
static GPU_LOCK: Mutex<()> = Mutex::new(());

fn gpu_guard() -> MutexGuard<'static, ()> {
    GPU_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Env var that turns every skip in this file into a hard failure.
///
/// `cargo test` swallows `eprintln!` for passing tests, so on a host with no
/// device the suite would print `13 passed` while the golden, the shader
/// oracle and the lifecycle smoke were never evaluated — a skip that reads
/// exactly like a verification. Setting `MMD_REQUIRE_GPU=1` (what the
/// development host's gate run uses, see `docs/05-testing.md`) removes the
/// ambiguity: on a machine that is supposed to have a GPU, a skip is a failure.
const REQUIRE_GPU_ENV: &str = "MMD_REQUIRE_GPU";

fn gpu_is_required() -> bool {
    std::env::var_os(REQUIRE_GPU_ENV).is_some_and(|v| v != "0" && !v.is_empty())
}

/// Record a skipped capability, or fail when the host promised to have it.
fn skip(case: &str, reason: &str) {
    assert!(
        !gpu_is_required(),
        "{case}: skipped ({reason}) but {REQUIRE_GPU_ENV} is set — this host is \
         declared to have a GPU, so a skip is a missed verification"
    );
    eprintln!("SKIP {case}: {reason}");
}

/// A live renderer, or `None` when this host genuinely has no GPU device.
///
/// Panics on every other renderer failure — see [`RenderError::is_device_unavailable`].
fn renderer_or_skip(case: &str) -> Option<SpriteRenderer> {
    match SpriteRenderer::new(&workspace_root(), true) {
        Ok(r) => Some(r),
        Err(e) if e.is_device_unavailable() => {
            skip(case, &format!("no GPU device on this host ({e})"));
            None
        }
        Err(e) => panic!("{case}: renderer failed for a non-device reason: {e}"),
    }
}

fn goldens_root() -> PathBuf {
    workspace_root().join("lab/goldens")
}

/// Untracked evidence directory for drift artifacts (under `/target`).
fn diff_dir(case: &str) -> PathBuf {
    workspace_root().join("target/golden-diffs").join(case)
}

fn live_host_binding(renderer: &SpriteRenderer) -> HostBinding {
    let (atlas_sha, shader_sha) = host_binding_hashes(&workspace_root()).expect("host hashes");
    HostBinding {
        backend: renderer.backend().to_string(),
        os: std::env::consts::OS.into(),
        adapter: renderer.ctx.adapter.clone(),
        atlas_manifest_sha256: atlas_sha,
        shader_canonical_sha256: shader_sha,
    }
}

/// The tracked golden for the running backend, plus its manifest.
fn host_golden(renderer: &SpriteRenderer) -> (GoldenManifest, Vec<u8>, PathBuf) {
    let family = golden_family_dir(renderer.backend()).expect("host backend has a golden family");
    let dir = goldens_root().join(family);
    let manifest = load_golden_manifest(&dir.join("manifest.json")).expect("golden manifest");
    assert_eq!(
        manifest.status, GOLDEN_STATUS_CAPTURED,
        "the development host's own golden must be captured, not a placeholder"
    );
    let pixels = load_golden_image(&dir, &manifest).expect("golden image");
    (manifest, pixels, dir)
}

/// Bounding box of every pixel that is not the cleared background, as
/// `(x0, y0, x1_exclusive, y1_exclusive)`.
fn nonclear_bbox(rb: &Readback) -> Option<(u32, u32, u32, u32)> {
    let mut bbox: Option<(u32, u32, u32, u32)> = None;
    for y in 0..rb.height {
        for x in 0..rb.width {
            if rb.pixel(x, y) == [0, 0, 0, 0] {
                continue;
            }
            bbox = Some(match bbox {
                None => (x, y, x + 1, y + 1),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
            });
        }
    }
    bbox
}

/// Four groups with a single sprite in atlas 0 at `pos`/`size`; the renderer
/// requires exactly [`ATLAS_COUNT`] groups, so the rest stay empty.
fn single_sprite_groups(pos: [f32; 2], size: [f32; 2]) -> [DrawGroup; ATLAS_COUNT] {
    std::array::from_fn(|i| DrawGroup {
        atlas_id: i as u32,
        instances: if i == 0 {
            vec![SpriteInstance::new(
                pos,
                size,
                frame_uv_rect(0, 0),
                SpriteInstance::WHITE,
            )]
        } else {
            Vec::new()
        },
    })
}

// ---------------------------------------------------------------------------
// 1. Instance data — headless
// ---------------------------------------------------------------------------

/// Every alive agent contributes exactly one instance, in the bucket its atlas
/// channel names.
///
/// The counts are compared per bucket rather than in aggregate: a pack step
/// that routed every agent into group 0 would still produce the right total.
#[test]
fn instance_per_alive_agent() {
    for fixture in [FIXTURE_SMALL_V1, FIXTURE_CORRIDOR_V1] {
        let mut h = Harness::fixture(fixture).seed(7).build().expect(fixture);
        // Mid-run, not at spawn: animation channels have advanced and agents
        // have spread across the map.
        h.step_exact(37);

        let expected_per_bucket = {
            let v = h.agents();
            let mut counts = vec![0usize; ATLAS_COUNT];
            for &atlas in v.atlas {
                counts[atlas as usize % ATLAS_COUNT] += 1;
            }
            counts
        };
        let alive = h.alive_count();
        assert_eq!(
            expected_per_bucket.iter().sum::<usize>(),
            alive,
            "{fixture}: fixture sanity — every alive agent has an atlas channel"
        );

        let groups = h.render_frame_groups();
        let packed: usize = groups.iter().map(|g| g.instances.len()).sum();
        assert_eq!(
            packed, alive,
            "{fixture}: packed {packed} instances for {alive} alive agents"
        );
        for (i, g) in groups.iter().enumerate() {
            assert_eq!(g.atlas_id as usize, i, "{fixture}: group {i} atlas_id");
            assert_eq!(
                g.instances.len(),
                expected_per_bucket[i],
                "{fixture}: group {i} holds {} instances, sim state names {}",
                g.instances.len(),
                expected_per_bucket[i]
            );
        }
    }
}

/// Each instance is centred on its agent's world position and carries the UV
/// rect for that agent's `(dir, frame)`.
///
/// Position is asserted as *the sprite's centre equals the agent's world pixel*
/// — the contract a reader cares about — rather than by restating the pack
/// step's top-left arithmetic. UVs are matched as a multiset per bucket, since
/// nothing promises intra-bucket ordering.
///
/// Centres are keyed at 1/256 px. Recovering the centre costs one f32 add that
/// the pack step's subtraction does not, so the two can differ by an ULP;
/// 1/256 px absorbs that while still resolving a misplacement 256× finer than
/// the smallest error that could matter (one pixel).
#[test]
fn instances_carry_agent_position_and_animation_uvs() {
    let mut h = Harness::fixture(FIXTURE_SMALL_V1)
        .seed(11)
        .build()
        .expect("fixture");
    h.step_exact(23);

    let cell = h.scenario().cell_size_px() as f32;
    let sprite = h.scenario().sprite_size_px() as f32;
    let dirs = h.scenario().direction_count();
    let frames = h.scenario().frame_count();

    // Expected per-bucket multisets, gathered from sim state alone.
    //
    // Position and UV are keyed *together*. As two independent multisets they
    // would be satisfied by any permutation that swapped one agent's UVs onto
    // another's position — exactly the shape of an SoA index desync between
    // `x/y` and `dir/frame`.
    type PlacedInstance = ((i64, i64), [u32; 4]);
    let mut want: Vec<BTreeMap<PlacedInstance, usize>> = vec![BTreeMap::new(); ATLAS_COUNT];
    {
        let v = h.agents();
        for i in 0..v.x.len() {
            let bucket = v.atlas[i] as usize % ATLAS_COUNT;
            let centre = (q256(v.x[i] * cell), q256(v.y[i] * cell));
            let uv = frame_uv_rect(u32::from(v.dir[i]), u32::from(v.frame[i])).map(f32::to_bits);
            *want[bucket].entry((centre, uv)).or_default() += 1;
            assert!(
                u32::from(v.dir[i]) < dirs && u32::from(v.frame[i]) < frames,
                "agent {i} animation channels ({}, {}) outside the scenario contract {dirs}x{frames}",
                v.dir[i],
                v.frame[i]
            );
        }
    }

    let groups = h.render_frame_groups();
    for (bucket, g) in groups.iter().enumerate() {
        let mut got: BTreeMap<PlacedInstance, usize> = BTreeMap::new();
        for inst in &g.instances {
            assert_eq!(inst.size, [sprite, sprite], "bucket {bucket}: sprite size");
            assert_eq!(inst.tint, SpriteInstance::WHITE, "bucket {bucket}: tint");
            let centre = (
                q256(inst.pos[0] + sprite * 0.5),
                q256(inst.pos[1] + sprite * 0.5),
            );
            got.entry((centre, inst.uv_rect.map(f32::to_bits)))
                .and_modify(|n| *n += 1)
                .or_insert(1);

            // UV rects address a real atlas frame: inside the atlas, ordered,
            // and exactly one frame wide/tall.
            let [u0, v0, u1, v1] = inst.uv_rect;
            assert!(u1 > u0 && v1 > v0, "bucket {bucket}: degenerate uv rect");
            assert!(
                u1 <= 1.0 + 1e-6 && v1 <= 1.0 + 1e-6,
                "bucket {bucket}: uv rect {:?} runs past the atlas edge",
                inst.uv_rect
            );
            assert!(
                ((u1 - u0) - 1.0 / frames as f32).abs() < 1e-6
                    && ((v1 - v0) - 1.0 / dirs as f32).abs() < 1e-6,
                "bucket {bucket}: uv rect {:?} is not one {frames}x{dirs} atlas frame",
                inst.uv_rect
            );
        }
        assert_eq!(
            got, want[bucket],
            "bucket {bucket}: (position, UV) pairs do not match agent state"
        );
    }
}

/// Packing is a pure projection of sim state: repeating it without stepping
/// the simulation reproduces the same instances, and stepping changes them.
#[test]
fn packing_is_a_pure_projection_of_sim_state() {
    let mut h = Harness::fixture(FIXTURE_SMALL_V1)
        .seed(3)
        .build()
        .expect("fixture");
    h.step_exact(15);

    let cell = h.scenario().cell_size_px() as f32;
    let sprite = h.scenario().sprite_size_px() as f32;

    let first: Vec<Vec<SpriteInstance>> = h
        .render_frame_groups()
        .iter()
        .map(|g| g.instances.clone())
        .collect();
    let again: Vec<Vec<SpriteInstance>> = h
        .render_frame_groups()
        .iter()
        .map(|g| g.instances.clone())
        .collect();
    assert_eq!(
        first, again,
        "re-packing an unchanged sim changed the frame"
    );

    // The allocating builder is the same projection as the reusing packer —
    // a divergence would mean the interactive path and this test disagree.
    let built = build_instance_groups(h.agents(), cell, sprite);
    let built: Vec<Vec<SpriteInstance>> = built.iter().map(|g| g.instances.clone()).collect();
    assert_eq!(first, built, "build_instance_groups diverged from pack");

    h.step_exact(1);
    let after: Vec<Vec<SpriteInstance>> = h
        .render_frame_groups()
        .iter()
        .map(|g| g.instances.clone())
        .collect();
    assert_ne!(
        first, after,
        "a simulation tick left every instance untouched — the frame is not tracking the sim"
    );
}

// ---------------------------------------------------------------------------
// 2. Projection — headless mirror
// ---------------------------------------------------------------------------

/// Known world corners map to the clip coordinates the shader contract
/// specifies: pixel space is y-down from the top-left, clip space is y-up over
/// `[-1, 1]`.
#[test]
fn world_to_clip_transform() {
    let view = [VIEW_WIDTH as f32, VIEW_HEIGHT as f32];
    // A quad covering the whole view: its four unit-quad corners must land on
    // the four corners of clip space, with y flipped.
    let full = ([0.0, 0.0], view);
    let cases = [
        ([-0.5, -0.5], [-1.0, 1.0]), // view top-left  → clip (-1, +1)
        ([0.5, -0.5], [1.0, 1.0]),   // view top-right → clip (+1, +1)
        ([-0.5, 0.5], [-1.0, -1.0]), // bottom-left    → clip (-1, -1)
        ([0.5, 0.5], [1.0, -1.0]),   // bottom-right   → clip (+1, -1)
        ([0.0, 0.0], [0.0, 0.0]),    // centre         → clip origin
    ];
    for (corner, want) in cases {
        let clip = world_to_clip(full.0, full.1, corner, view);
        assert!(
            (clip[0] - want[0]).abs() < 1e-6 && (clip[1] - want[1]).abs() < 1e-6,
            "corner {corner:?} → clip {:?}, expected {want:?}",
            [clip[0], clip[1]]
        );
        assert_eq!([clip[2], clip[3]], [0.0, 1.0], "clip z/w are fixed");
    }

    // Y is flipped, not merely scaled: a sprite in the upper half of the view
    // must sit in the positive half of clip space.
    let upper = world_to_clip([0.0, 0.0], [10.0, 10.0], [0.0, 0.0], view);
    assert!(upper[1] > 0.0, "a sprite near the top must have clip y > 0");
    let lower = world_to_clip(
        [0.0, VIEW_HEIGHT as f32 - 10.0],
        [10.0, 10.0],
        [0.0, 0.0],
        view,
    );
    assert!(
        lower[1] < 0.0,
        "a sprite near the bottom must have clip y < 0"
    );

    // `clip_to_pixel` is pinned absolutely, not only as an inverse: a sign or
    // scale error shared by both functions would round-trip cleanly and go
    // unnoticed if the only headless constraint were the round trip below.
    let viewport_cases = [
        ([-1.0, 1.0], [0.0, 0.0]),                              // clip top-left
        ([1.0, 1.0], [VIEW_WIDTH as f32, 0.0]),                 // clip top-right
        ([-1.0, -1.0], [0.0, VIEW_HEIGHT as f32]),              // bottom-left
        ([1.0, -1.0], [VIEW_WIDTH as f32, VIEW_HEIGHT as f32]), // bottom-right
        (
            [0.0, 0.0],
            [VIEW_WIDTH as f32 / 2.0, VIEW_HEIGHT as f32 / 2.0],
        ), // centre
    ];
    for (clip_xy, want) in viewport_cases {
        let px = clip_to_pixel([clip_xy[0], clip_xy[1], 0.0, 1.0], view);
        assert!(
            (px[0] - want[0]).abs() < 1e-3 && (px[1] - want[1]).abs() < 1e-3,
            "clip {clip_xy:?} → pixel {px:?}, expected {want:?}"
        );
    }

    // The viewport map is the inverse: clip → pixel returns the world pixel.
    for pos in [[0.0, 0.0], [640.0, 360.0], [1889.5, 1049.5]] {
        let size = [30.0, 30.0];
        for corner in [[-0.5, -0.5], [0.5, 0.5], [0.0, 0.0]] {
            let clip = world_to_clip(pos, size, corner, view);
            let px = clip_to_pixel(clip, view);
            let want = [
                pos[0] + (corner[0] + 0.5) * size[0],
                pos[1] + (corner[1] + 0.5) * size[1],
            ];
            assert!(
                (px[0] - want[0]).abs() < 1e-3 && (px[1] - want[1]).abs() < 1e-3,
                "clip→pixel round trip for {pos:?}/{corner:?} gave {px:?}, expected {want:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Atlas manifest ↔ generated PNGs
// ---------------------------------------------------------------------------

/// The tracked manifest and the tracked PNGs agree, and every atlas decodes to
/// the frame grid the UV contract assumes.
///
/// The negative half of this contract (flipping a PNG byte must fail the hash)
/// lives in `gpu_smoke.rs::tracked_atlas_hashes_are_enforced`.
#[test]
fn atlas_manifest_matches_generated_pngs() {
    let dir = workspace_root().join("assets/sprites/generated");
    let atlases = load_atlases(&dir).expect("tracked atlases load and hash-verify");
    for (i, atlas) in atlases.iter().enumerate() {
        assert_eq!(atlas.id as usize, i, "atlas {i} id");
        assert_eq!(
            atlas.rgba.len(),
            (atlas.width * atlas.height * 4) as usize,
            "atlas {i} rgba length"
        );
        assert_eq!(
            (atlas.width % FRAME_SIZE_PX, atlas.height % FRAME_SIZE_PX),
            (0, 0),
            "atlas {i} is {}x{}, not a whole number of {FRAME_SIZE_PX}px frames",
            atlas.width,
            atlas.height
        );
        // The UV contract tiles exactly this atlas: the last frame's rect must
        // start one cell in from the far edge and end *on* it.
        let (cols, rows) = (atlas.width / FRAME_SIZE_PX, atlas.height / FRAME_SIZE_PX);
        let [u0, v0, u1, v1] = frame_uv_rect(rows - 1, cols - 1);
        let want = [
            (cols - 1) as f32 / cols as f32,
            (rows - 1) as f32 / rows as f32,
            1.0,
            1.0,
        ];
        for (got, want) in [u0, v0, u1, v1].into_iter().zip(want) {
            assert!(
                (got - want).abs() < 1e-6,
                "atlas {i}: last frame UV {:?} does not tile the {cols}x{rows} grid \
                 (expected {want:?})",
                [u0, v0, u1, v1]
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 4. Golden drift — headless comparator
// ---------------------------------------------------------------------------

/// A shifted sprite fails the comparison *and* leaves a reviewable artifact.
///
/// Runs on the CPU comparator so drift detection is still covered on a
/// headless host; `golden_drift_fails_on_gpu` exercises the same path against
/// a real shifted render.
#[test]
fn golden_drift_fails() {
    let (w, h) = (64u32, 32u32);
    let mut golden = vec![0u8; (w * h * 4) as usize];
    // A 6x6 opaque block at (10, 8).
    let put = |buf: &mut Vec<u8>, ox: u32, oy: u32| {
        for y in oy..oy + 6 {
            for x in ox..ox + 6 {
                let i = ((y * w + x) * 4) as usize;
                buf[i..i + 4].copy_from_slice(&[200, 40, 90, 255]);
            }
        }
    };
    put(&mut golden, 10, 8);

    // Same block, shifted one pixel right: 12 pixels change (6 leave, 6 arrive).
    let mut shifted = vec![0u8; (w * h * 4) as usize];
    put(&mut shifted, 11, 8);
    let candidate = Readback {
        width: w,
        height: h,
        rgba: shifted,
    };

    let manifest = host_shaped_manifest(w, h);
    let host = HostBinding {
        backend: manifest.backend.clone(),
        os: manifest.os.clone(),
        adapter: manifest.adapter.clone(),
        atlas_manifest_sha256: manifest.atlas_manifest_sha256.clone(),
        shader_canonical_sha256: manifest.shader_canonical_sha256.clone(),
    };

    let dir = diff_dir("golden_drift_fails");
    clear_dir(&dir);
    let err = compare_readback_writing_diff(&manifest, &host, &golden, &candidate, &dir)
        .expect_err("a shifted sprite must fail the golden");

    let GoldenError::GoldenDrift {
        differing_pixels,
        total_pixels,
        max_channel_delta,
        tolerance,
        ref artifact_dir,
    } = err
    else {
        panic!("expected GoldenDrift, got {err}");
    };
    assert_eq!(
        differing_pixels, 12,
        "one-pixel shift of a 6x6 block moves 12 pixels: {err}"
    );
    assert_eq!(
        total_pixels,
        u64::from(w) * u64::from(h),
        "the drift verdict must scale its count against the whole frame: {err}"
    );
    assert_eq!(
        tolerance, GOLDEN_MAX_CHANNEL_DELTA_POLICY,
        "the verdict must record the tolerance it ran at: {err}"
    );
    assert_eq!(
        max_channel_delta, 255,
        "the block is opaque against a clear background"
    );
    assert!(
        artifact_dir.contains("golden_drift_fails"),
        "the error must name where the evidence went: {err}"
    );
    assert_diff_artifact_is_reviewable(&dir, 12, w, h);

    // An identical frame passes and writes nothing — the artifact is failure
    // evidence, not a side effect of every comparison.
    let clean_dir = diff_dir("golden_drift_fails_clean");
    clear_dir(&clean_dir);
    let identical = Readback {
        width: w,
        height: h,
        rgba: golden.clone(),
    };
    compare_readback_writing_diff(&manifest, &host, &golden, &identical, &clean_dir)
        .expect("an identical frame matches the golden");
    assert!(
        !clean_dir.exists(),
        "a passing comparison must not write a diff artifact"
    );

    // Same directly through the writer: no drift, no directory, and the
    // summary says so.
    let summary = write_golden_diff(&clean_dir, &golden, &identical, 0).expect("clean diff");
    assert!(
        !summary.is_drift(),
        "an identical frame reported drift: {summary:?}"
    );
    assert_eq!(summary.differing_pixels, 0);
    assert_eq!(summary.first_difference, None);
    assert!(
        !clean_dir.exists(),
        "write_golden_diff created an artifact for a frame with no drift"
    );
}

/// A size mismatch has no per-pixel diff — it is reported as one, on either side.
#[test]
fn diff_rejects_mismatched_buffers() {
    let (w, h) = (8u32, 4u32);
    let full = vec![7u8; (w * h * 4) as usize];

    let short_golden = vec![7u8; (w * h * 4 - 4) as usize];
    let candidate = Readback {
        width: w,
        height: h,
        rgba: full.clone(),
    };
    let err = diff_readback(&short_golden, &candidate, 0).expect_err("short golden");
    assert!(
        matches!(
            err,
            GoldenError::ImageSizeMismatch {
                expected,
                got
            } if expected == (w * h * 4) as usize && got == short_golden.len()
        ),
        "{err}"
    );

    let short_candidate = Readback {
        width: w,
        height: h,
        rgba: vec![7u8; (w * h * 4 - 4) as usize],
    };
    let err = diff_readback(&full, &short_candidate, 0).expect_err("short candidate");
    assert!(
        matches!(
            err,
            GoldenError::ImageSizeMismatch {
                expected,
                got
            } if expected == (w * h * 4) as usize && got == short_candidate.rgba.len()
        ),
        "{err}"
    );
}

/// A single-channel, single-pixel drift is still caught, and the summary points
/// straight at it. The exact-compare policy has no slack to hide in.
#[test]
fn golden_drift_reports_the_first_differing_pixel() {
    let (w, h) = (8u32, 4u32);
    let golden: Vec<u8> = (0..(w * h * 4)).map(|i| (i % 251) as u8).collect();
    let mut drifted = golden.clone();
    // Pixel (5, 2), channel 1.
    let idx = ((2 * w + 5) * 4 + 1) as usize;
    drifted[idx] = drifted[idx].wrapping_add(1);
    let candidate = Readback {
        width: w,
        height: h,
        rgba: drifted,
    };

    let (summary, mask) =
        diff_readback(&golden, &candidate, GOLDEN_MAX_CHANNEL_DELTA_POLICY).expect("diff");
    assert_eq!(summary.differing_pixels, 1, "exactly one pixel moved");
    assert_eq!(summary.max_channel_delta, 1);
    assert_eq!(
        summary.first_difference,
        Some([5, 2]),
        "the summary must locate the drift"
    );
    assert!(summary.is_drift());

    // The mask marks that pixel and only that pixel.
    let marked: Vec<u32> = (0..w * h)
        .filter(|p| mask[(p * 4) as usize..(p * 4 + 4) as usize] != [0, 0, 0, 255])
        .collect();
    assert_eq!(
        marked,
        vec![2 * w + 5],
        "mask marks exactly the drifted pixel"
    );
}

/// A manifest that does not apply to this host must not produce a diff image:
/// nothing was compared, so there is nothing to review.
#[test]
fn unbound_golden_writes_no_diff() {
    let (w, h) = (8u32, 4u32);
    let golden = vec![0u8; (w * h * 4) as usize];
    let candidate = Readback {
        width: w,
        height: h,
        rgba: vec![255u8; (w * h * 4) as usize],
    };
    let manifest = host_shaped_manifest(w, h);
    let mut host = HostBinding {
        backend: manifest.backend.clone(),
        os: manifest.os.clone(),
        adapter: "Some Other GPU".into(),
        atlas_manifest_sha256: manifest.atlas_manifest_sha256.clone(),
        shader_canonical_sha256: manifest.shader_canonical_sha256.clone(),
    };
    let dir = diff_dir("unbound_golden_writes_no_diff");
    clear_dir(&dir);

    let err = compare_readback_writing_diff(&manifest, &host, &golden, &candidate, &dir)
        .expect_err("adapter drift blocks the comparison");
    assert!(
        matches!(err, GoldenError::ManifestDrift { .. }),
        "expected recalibration error, got {err}"
    );
    assert!(
        !dir.exists(),
        "an unbound golden must not write a pixel diff"
    );

    // Same for a golden belonging to another backend.
    host.adapter = manifest.adapter.clone();
    host.backend = if manifest.backend == "vulkan" {
        "metal".into()
    } else {
        "vulkan".into()
    };
    let err = compare_readback_writing_diff(&manifest, &host, &golden, &candidate, &dir)
        .expect_err("a foreign backend cannot use this golden");
    assert!(
        matches!(err, GoldenError::BackendMismatch { .. }),
        "expected backend mismatch, got {err}"
    );
    assert!(
        !dir.exists(),
        "a foreign golden must not write a pixel diff"
    );
}

// ---------------------------------------------------------------------------
// 5. Headless skip policy
// ---------------------------------------------------------------------------

/// The skip path is exactly as wide as "this host has no GPU device" — and no
/// wider. Every other renderer failure is a defect that must fail the suite.
///
/// Without the negative half, a drifted atlas or a rejected software adapter
/// would be silently swallowed as "headless" and the render gate would pass
/// vacuously on a broken tree.
#[test]
fn no_gpu_skips_cleanly() {
    // Device absence → skip.
    let absent = RenderError::DeviceUnavailable(
        "vulkan device failed (no ICD). Check LD_LIBRARY_PATH".into(),
    );
    assert!(
        absent.is_device_unavailable(),
        "a failed device creation must classify as skip: {absent}"
    );

    // Everything else → fail.
    let must_fail: Vec<RenderError> = vec![
        RenderError::Atlas("atlas_0.png hash mismatch: expected aa, got bb".into()),
        RenderError::Io("read assets/sprites/generated/manifest.json: missing".into()),
        RenderError::Shader("sprite.vert.spv rejected".into()),
        RenderError::Sdl("too many instances 200000".into()),
        RenderError::GroupCount {
            got: 3,
            expected: ATLAS_COUNT,
        },
        RenderError::WrongBackend {
            got: "software".into(),
            required: "vulkan",
        },
        RenderError::RejectedAdapter {
            name: "llvmpipe".into(),
        },
        RenderError::RejectedHostArch {
            got: "x86_64".into(),
            required: "aarch64",
        },
    ];
    for e in &must_fail {
        assert!(
            !e.is_device_unavailable(),
            "`{e}` is a defect, not a headless host — skipping it would hole the gate"
        );
    }

    // And the harness helper agrees: it returns a renderer or a skip, and only
    // panics on the defect class asserted above. On a host with a GPU this
    // yields Some; on a headless host, None. Both are a pass.
    let _g = gpu_guard();
    match renderer_or_skip("no_gpu_skips_cleanly") {
        Some(r) => assert_eq!(
            r.backend(),
            mmd_engine::render::required_backend(),
            "a renderer that was handed back must be on the required backend"
        ),
        None => eprintln!("no_gpu_skips_cleanly: skip path exercised for real"),
    }
}

/// The *production* device path really produces the variant the skip policy
/// keys on.
///
/// `no_gpu_skips_cleanly` only proves the predicate is not inverted — it
/// constructs the variant by hand. This drives the real
/// `GpuContext::new` → `create_host_device` path with a poisoned Vulkan
/// loader, so reverting any of those sites to `RenderError::Sdl` turns every
/// headless run from a clean skip into a hard panic and is caught here.
///
/// Runs as a child process because SDL and the Vulkan loader read their
/// environment once per process; the child re-enters this same test binary.
#[test]
#[cfg(target_os = "linux")]
fn device_creation_failure_classifies_as_unavailable() {
    const CHILD: &str = "MMD_TEST_NO_VULKAN_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let err = SpriteRenderer::new(&workspace_root(), true)
            .err()
            .expect("child: a poisoned Vulkan loader must not yield a device");
        assert!(
            err.is_device_unavailable(),
            "child: device creation failed with {err:?}, which does not classify as \
             'no GPU on this host' — headless runs would panic instead of skipping"
        );
        return;
    }

    let status = std::process::Command::new(std::env::current_exe().expect("test binary path"))
        .args([
            "--exact",
            "device_creation_failure_classifies_as_unavailable",
            "--nocapture",
        ])
        .env(CHILD, "1")
        // Never let the child inherit the parent's strict-mode flag: it is
        // *expected* to find no device.
        .env_remove(REQUIRE_GPU_ENV)
        .env("VK_ICD_FILENAMES", "/nonexistent-mmd-t31.json")
        .env("VK_DRIVER_FILES", "/nonexistent-mmd-t31.json")
        .env("SDL_VIDEODRIVER", "offscreen")
        .env_remove("SDL_VULKAN_LIBRARY")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .status()
        .expect("spawn child test process");
    assert!(
        status.success(),
        "child process asserting the device-unavailable classification failed ({status})"
    );
}

// ---------------------------------------------------------------------------
// 6. GPU cases — skip cleanly without a device
// ---------------------------------------------------------------------------

/// The committed host golden still describes what this backend renders.
#[test]
fn golden_frame_matches() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("golden_frame_matches") else {
        return;
    };
    let host = live_host_binding(&r);
    let (manifest, golden, _) = host_golden(&r);
    assert_eq!(
        (manifest.width, manifest.height),
        (VIEW_WIDTH, VIEW_HEIGHT),
        "the golden must be the gate resolution"
    );
    assert_eq!(
        manifest.max_channel_delta, GOLDEN_MAX_CHANNEL_DELTA_POLICY,
        "the host golden compares exactly; any tolerance would cover f32/driver \
         variation on this host only and never a second backend"
    );

    let rb = r
        .draw_offscreen_readback(&SpriteRenderer::static_demo_groups())
        .expect("offscreen readback");
    let dir = diff_dir("golden_frame_matches");
    clear_dir(&dir);
    if let Err(e) = compare_readback_writing_diff(&manifest, &host, &golden, &rb, &dir) {
        panic!(
            "the development host no longer renders its own golden frame: {e}\n\
             Review the artifact, then regenerate deliberately with:\n  \
             MMD_UPDATE_GOLDEN=1 cargo test -p mmd-engine --test gpu_golden -- --ignored update_host_golden"
        );
    }
    assert!(
        !dir.exists(),
        "a matching frame must not leave a drift artifact behind"
    );
}

/// Shifting the scene by one pixel fails the golden and writes the diff.
///
/// This is the live half of `golden_drift_fails`: the comparison runs against
/// a real render, so it proves the gate would actually catch a renderer that
/// started drawing in the wrong place.
#[test]
fn golden_drift_fails_on_gpu() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("golden_drift_fails_on_gpu") else {
        return;
    };
    let host = live_host_binding(&r);
    let (manifest, golden, _) = host_golden(&r);

    let mut shifted = SpriteRenderer::static_demo_groups();
    for g in shifted.iter_mut() {
        for inst in g.instances.iter_mut() {
            inst.pos[0] += 1.0;
        }
    }
    let rb = r.draw_offscreen_readback(&shifted).expect("readback");

    let dir = diff_dir("golden_drift_fails_on_gpu");
    clear_dir(&dir);
    let err = compare_readback_writing_diff(&manifest, &host, &golden, &rb, &dir)
        .expect_err("a one-pixel scene shift must fail the golden");
    let GoldenError::GoldenDrift {
        differing_pixels, ..
    } = err
    else {
        panic!("expected GoldenDrift, got {err}");
    };
    assert!(
        differing_pixels > 0,
        "drift with no differing pixels: {err}"
    );
    assert_diff_artifact_is_reviewable(&dir, differing_pixels, VIEW_WIDTH, VIEW_HEIGHT);
}

/// The real shader agrees with [`world_to_clip`]: a sprite at a known world
/// position rasterizes exactly inside the pixel rect the mirror predicts.
///
/// This is what stops the CPU mirror from being a tautology — the GPU is the
/// oracle, and the assertion is one-sided (every lit pixel must be inside the
/// predicted rect), so a transform that shifted, scaled, or un-flipped the
/// projection fails no matter which way it moved.
#[test]
fn world_to_clip_matches_gpu_raster() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("world_to_clip_matches_gpu_raster") else {
        return;
    };
    let view = [VIEW_WIDTH as f32, VIEW_HEIGHT as f32];
    // Deliberately off-centre and asymmetric in x/y so an axis swap or a lost
    // y-flip cannot coincidentally still pass.
    let pos = [700.0, 300.0];
    let size = [96.0, 64.0];

    let top_left = clip_to_pixel(world_to_clip(pos, size, [-0.5, -0.5], view), view);
    let bottom_right = clip_to_pixel(world_to_clip(pos, size, [0.5, 0.5], view), view);
    let (px0, py0) = (top_left[0].round() as u32, top_left[1].round() as u32);
    let (px1, py1) = (
        bottom_right[0].round() as u32,
        bottom_right[1].round() as u32,
    );

    // Where the *opaque* part of atlas frame (0,0) sits inside its 32x32
    // source frame, scaled into the predicted quad. Deriving the expected
    // footprint from the atlas itself — instead of "at least half the rect" —
    // is what makes this probe reject a wrong scale: a shader computing
    // `(corner * 0.5 + 0.5) * size` would draw a correctly-sized-looking blob
    // in the wrong half of the quad and pass a "covers half" check.
    let (fx0, fy0, fx1, fy1) =
        opaque_frame_bounds(&r.atlases()[0]).expect("atlas frame (0,0) has opaque texels");
    let sx = size[0] / FRAME_SIZE_PX as f32;
    let sy = size[1] / FRAME_SIZE_PX as f32;
    let want = (
        px0 + (fx0 as f32 * sx).round() as u32,
        py0 + (fy0 as f32 * sy).round() as u32,
        px0 + (fx1 as f32 * sx).round() as u32,
        py0 + (fy1 as f32 * sy).round() as u32,
    );

    let rb = r
        .draw_offscreen_readback(&single_sprite_groups(pos, size))
        .expect("readback");
    let bbox = nonclear_bbox(&rb).expect("the sprite must draw something");
    let (bx0, by0, bx1, by1) = bbox;

    // Nearest sampling can round a destination pixel either way at the edges.
    const EDGE_SLACK: i64 = 2;
    let edges = [
        ("left", bx0 as i64, want.0 as i64),
        ("top", by0 as i64, want.1 as i64),
        ("right", bx1 as i64, want.2 as i64),
        ("bottom", by1 as i64, want.3 as i64),
    ];
    for (name, got, expected) in edges {
        assert!(
            (got - expected).abs() <= EDGE_SLACK,
            "rendered footprint {bbox:?} has its {name} edge at {got}, but \
             world_to_clip + the atlas's own opaque bounds predict {expected} \
             (predicted quad ({px0},{py0})..({px1},{py1})) — the shader and \
             world_to_clip disagree"
        );
    }

    // The predicted quad is where we asked for it, not merely self-consistent.
    assert_eq!(
        (px0, py0, px1, py1),
        (700, 300, 796, 364),
        "world_to_clip round trip moved the sprite off its world position"
    );
}

/// Bounds of the non-transparent texels of atlas frame `(dir 0, frame 0)`,
/// as `(x0, y0, x1_exclusive, y1_exclusive)` within that 32x32 frame.
fn opaque_frame_bounds(atlas: &mmd_engine::render::AtlasRgba) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..FRAME_SIZE_PX {
        for x in 0..FRAME_SIZE_PX {
            if atlas.pixel(x, y)[3] == 0 {
                continue;
            }
            bounds = Some(match bounds {
                None => (x, y, x + 1, y + 1),
                Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
            });
        }
    }
    bounds
}

/// Renderer lifecycle smoke: device creation, a window resize across
/// swapchain presents, and a clean shutdown that releases the device so a
/// fresh renderer can be built in the same process.
#[test]
fn renderer_smoke_device_resize_shutdown() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("renderer_smoke_device_resize_shutdown") else {
        return;
    };
    assert_eq!(r.backend(), mmd_engine::render::required_backend());
    let groups = SpriteRenderer::static_demo_groups();

    // Offscreen path is authoritative and always available.
    let rb = r.draw_offscreen_readback(&groups).expect("first frame");
    assert_eq!((rb.width, rb.height), (VIEW_WIDTH, VIEW_HEIGHT));

    // Present path across changing target sizes. A window is a separate
    // capability from a device (a session may have one and not the other), so
    // an unavailable window skips this half only — never the whole case.
    //
    // Windows are built hidden and each size gets its own window: a hidden
    // surface's `set_size` is only a request the compositor may ignore, so
    // asserting on a resized `window.size()` would assert on the window
    // manager, not on the renderer. A fresh window per size exercises the same
    // thing that matters here — swapchain acquisition and blit at a target
    // resolution that is not the fixed 1920x1080 offscreen one, including a
    // target both smaller and larger in each axis.
    //
    // Each claim is released before its window is dropped. That pairing is
    // load-bearing, not tidiness: a window destroyed while still claimed
    // leaves the device holding a dangling swapchain, and the next device call
    // faults the process (SIGSEGV, no unwind, no test verdict).
    // Creating a window is the only part of this that may legitimately be
    // unavailable. Once a window exists, a failing claim or present is a
    // defect, not a missing capability — swallowing it would let this case
    // pass on a GPU host while proving nothing.
    let mut presented = 0usize;
    for (w, h) in [(640u32, 360u32), (1280, 720), (800, 600)] {
        let Ok(mut window) = r
            .ctx
            .video
            .window("mmd-render-correctness", w, h)
            .hidden()
            .resizable()
            .build()
            .inspect_err(|e| {
                skip(
                    "renderer_smoke/present",
                    &format!("no window ({w}x{h}): {e}"),
                )
            })
        else {
            continue;
        };
        r.ctx
            .claim_window(&window)
            .unwrap_or_else(|e| panic!("claim a {w}x{h} window the video driver just built: {e}"));

        // Every failure between claim and release goes through `release_first`:
        // unwinding past a claimed window would crash the process instead of
        // reporting an assertion.
        let release_first = |r: &mut SpriteRenderer, window: &_, res: Result<(), _>| {
            if let Err(e) = res {
                r.ctx.release_window(window);
                panic!("present at {w}x{h}: {e}");
            }
        };
        let res = r.draw_to_swapchain(&window, &groups);
        release_first(&mut r, &window, res);
        // A resize request the compositor *does* honour must not break the
        // next present either.
        if window.set_size(w / 2, h / 2).is_ok() {
            let res = r.draw_to_swapchain(&window, &groups);
            release_first(&mut r, &window, res);
        }
        r.ctx.release_window(&window);
        presented += 1;
    }
    assert!(
        presented == 0 || presented == 3,
        "the present path worked for {presented} of 3 window sizes — a swapchain \
         that only handles some resolutions is a defect, not a missing capability"
    );

    // The device survived every claim/release cycle: it can still build a
    // window and still render offscreen. (Before `release_window` existed this
    // is the call that faulted.)
    if presented > 0 {
        let probe = r
            .ctx
            .video
            .window("mmd-render-correctness-probe", 320, 240)
            .hidden()
            .build()
            .expect("the device outlived its released windows");
        drop(probe);
        r.draw_offscreen_readback(&groups)
            .expect("offscreen path still works after the swapchain cycles");
    }

    // Clean shutdown: dropping releases GPU objects before the device, and the
    // process can build a second renderer afterwards.
    drop(r);
    let Some(mut second) = renderer_or_skip("renderer_smoke_device_resize_shutdown/restart") else {
        panic!("the device vanished after a clean shutdown");
    };
    let rb2 = second
        .draw_offscreen_readback(&groups)
        .expect("renderer rebuilt after shutdown");
    assert_eq!(rb.rgba, rb2.rgba, "the same scene must survive a restart");
    drop(second);
}

// ---------------------------------------------------------------------------
// Local helpers
// ---------------------------------------------------------------------------

/// A manifest bound to the running host's backend, sized for a synthetic frame.
fn host_shaped_manifest(width: u32, height: u32) -> GoldenManifest {
    GoldenManifest {
        schema_version: mmd_engine::render::GOLDEN_SCHEMA_VERSION.into(),
        backend: mmd_engine::render::required_backend().into(),
        os: std::env::consts::OS.into(),
        status: GOLDEN_STATUS_CAPTURED.into(),
        scene: mmd_engine::render::GOLDEN_SCENE_STATIC_DEMO.into(),
        width,
        height,
        image_file: "golden.png".into(),
        image_sha256: String::new(),
        adapter: "Synthetic Adapter".into(),
        driver_info: "synthetic".into(),
        atlas_manifest_sha256: "aa".repeat(32),
        shader_canonical_sha256: "bb".repeat(32),
        max_channel_delta: GOLDEN_MAX_CHANNEL_DELTA_POLICY,
    }
}

/// A drift artifact is only useful if a human can open it: the candidate
/// frame, a mask locating the drift, and a summary that agrees with the error.
fn assert_diff_artifact_is_reviewable(
    dir: &Path,
    expected_differing: u64,
    width: u32,
    height: u32,
) {
    for name in [
        GOLDEN_DIFF_ACTUAL_PNG,
        GOLDEN_DIFF_MASK_PNG,
        GOLDEN_DIFF_SUMMARY_JSON,
    ] {
        let path = dir.join(name);
        let meta = std::fs::metadata(&path)
            .unwrap_or_else(|e| panic!("drift artifact {} missing: {e}", path.display()));
        assert!(meta.len() > 0, "drift artifact {} is empty", path.display());
    }

    let json = std::fs::read(dir.join(GOLDEN_DIFF_SUMMARY_JSON)).expect("read summary");
    let summary: GoldenDiffSummary = serde_json::from_slice(&json).expect("parse summary");
    assert_eq!((summary.width, summary.height), (width, height));
    assert_eq!(
        summary.differing_pixels, expected_differing,
        "summary disagrees with the reported drift"
    );
    assert!(
        summary.first_difference.is_some(),
        "summary must locate the first drifted pixel"
    );
    assert_eq!(
        summary.tolerance, GOLDEN_MAX_CHANNEL_DELTA_POLICY,
        "the artifact must record the tolerance the comparison ran at"
    );

    // The mask decodes at the compared resolution — it is a real reviewable
    // image, not a placeholder byte blob.
    let mask_bytes = std::fs::read(dir.join(GOLDEN_DIFF_MASK_PNG)).expect("read mask");
    let mask =
        mmd_engine::render::decode_readback_png(GOLDEN_DIFF_MASK_PNG, &mask_bytes, width, height)
            .expect("mask decodes as RGBA8 at the compared resolution");
    let marked = (0..width * height)
        .filter(|p| {
            let i = (p * 4) as usize;
            mask.rgba[i..i + 4] != [0, 0, 0, 255]
        })
        .count() as u64;
    assert_eq!(
        marked, expected_differing,
        "the mask must mark exactly the drifted pixels"
    );
}

/// Remove a diff directory, failing loudly rather than silently validating a
/// previous run's artifact.
fn clear_dir(dir: &Path) {
    if dir.exists() {
        std::fs::remove_dir_all(dir)
            .unwrap_or_else(|e| panic!("clear stale artifact dir {}: {e}", dir.display()));
    }
}

/// Quantize a pixel coordinate to 1/256 px for exact multiset comparison.
fn q256(v: f32) -> i64 {
    (v as f64 * 256.0).round() as i64
}
