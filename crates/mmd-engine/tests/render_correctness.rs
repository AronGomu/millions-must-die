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
    GoldenDiffSummary, GoldenError, GoldenManifest, HostBinding, ISO_DEPTH_EPSILON, IsoView,
    Readback, RenderError, SpriteInstance, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH, clip_to_pixel,
    compare_readback_writing_diff, diff_readback, frame_uv_rect, golden_family_dir,
    host_binding_hashes, iso_depth, load_atlases, load_golden_image, load_golden_manifest,
    world_to_clip, write_golden_diff,
};
use mmd_engine::runtime::{
    RING_INNER, RING_OUTER, RING_TINT, build_instance_groups, ring_quad_size_px, ring_radius_px,
};
use mmd_engine::scenario::Cell;
use mmd_engine::testkit::{
    COLLISION_SPRITE_SCENE, FIXTURE_CORRIDOR_V1, FIXTURE_SMALL_V1, GridSpec, Harness,
    ScenarioSource, scene_path,
};
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

/// The [`ATLAS_COUNT`] groups the renderer demands, all empty. For frames that
/// draw only an overlay.
fn empty_groups() -> [DrawGroup; ATLAS_COUNT] {
    std::array::from_fn(|i| DrawGroup {
        atlas_id: i as u32,
        instances: Vec::new(),
    })
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

/// The projected ground point of a cell-space position, written out longhand.
///
/// Deliberately *not* [`IsoView::project`]: every case that states where an
/// instance should land needs an expectation that does not flow through the
/// function under test, or a projection that silently changed would move the
/// packer and the expectation together and pass.
fn ground_point(iso: &IsoView, cx: f32, cy: f32) -> [f32; 2] {
    [
        iso.origin[0] + (cx - cy) * iso.tile_w * 0.5,
        iso.origin[1] + (cx + cy) * iso.tile_h * 0.5,
    ]
}

/// Whether a quad of `size` at `pos` puts any pixel inside the view, written
/// out longhand for the same reason as [`ground_point`].
fn on_screen(pos: [f32; 2], size: [f32; 2], view: [f32; 2]) -> bool {
    pos[0] + size[0] > 0.0 && pos[1] + size[1] > 0.0 && pos[0] < view[0] && pos[1] < view[1]
}

/// How many alive agents have a sprite quad the view can see at all.
fn visible_sprite_count(
    agents: mmd_engine::sim::AgentsView<'_>,
    iso: &IsoView,
    sprite: f32,
) -> usize {
    (0..agents.x.len())
        .filter(|&i| {
            let g = ground_point(iso, agents.x[i], agents.y[i]);
            on_screen(
                [g[0] - sprite * 0.5, g[1] - sprite],
                [sprite, sprite],
                iso.view_size,
            )
        })
        .count()
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

        let iso = h.runtime().iso_view();
        let sprite = h.scenario().sprite_size_px() as f32;
        let expected_per_bucket = {
            let v = h.agents();
            let mut counts = vec![0usize; ATLAS_COUNT];
            for i in 0..v.x.len() {
                let g = ground_point(&iso, v.x[i], v.y[i]);
                if !on_screen(
                    [g[0] - sprite * 0.5, g[1] - sprite],
                    [sprite, sprite],
                    iso.view_size,
                ) {
                    continue;
                }
                counts[v.atlas[i] as usize % ATLAS_COUNT] += 1;
            }
            counts
        };
        let alive = h.alive_count();
        // These fixtures are small enough that the whole map diamond fits the
        // view, so the cull removes nothing and "one instance per alive agent"
        // is still the full claim. Stated rather than assumed: a fixture that
        // grew past the view would otherwise turn this into a weaker test
        // without anyone noticing.
        assert_eq!(
            expected_per_bucket.iter().sum::<usize>(),
            alive,
            "{fixture}: the whole fixture must project inside the view for this case \
             to mean 'one instance per alive agent'"
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

/// Each instance **stands on** its agent's projected ground point and carries
/// the UV rect for that agent's `(dir, frame)`.
///
/// Position is asserted as *the quad's bottom-centre equals the isometric
/// projection of the agent's cell* — the contract a reader cares about, and the
/// one the depth key depends on — rather than by restating the pack step's
/// top-left arithmetic. UVs are matched as a multiset per bucket, since
/// nothing promises intra-bucket ordering.
///
/// Foot points are keyed at 1/256 px. Recovering the foot costs f32 adds that
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

    let iso = h.runtime().iso_view();
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
            let g = ground_point(&iso, v.x[i], v.y[i]);
            let centre = (q256(g[0]), q256(g[1]));
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
            // Bottom-centre, not centre: the quad's *feet* sit on the tile.
            let centre = (q256(inst.pos[0] + sprite * 0.5), q256(inst.pos[1] + sprite));
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
            "bucket {bucket}: (ground point, UV) pairs do not match agent state"
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

    let iso = h.runtime().iso_view();
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
    let built = build_instance_groups(h.agents(), &iso, sprite);
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
// 1b. Hitbox rings — headless
// ---------------------------------------------------------------------------

/// A harness on the tracked sprite-scale collision scene: a real body, so a
/// ring has something true to trace.
fn bodied_harness(agents: u32, seed: u64) -> Harness {
    let h = Harness::builder(ScenarioSource::path(scene_path(COLLISION_SPRITE_SCENE)))
        .agents(agents)
        .seed(seed)
        .build()
        .expect("tracked collision scene loads");
    assert!(
        h.scenario().collision_radius_q8() > 0,
        "the collision scene lost its body; every ring claim below would be vacuous"
    );
    h
}

/// Rings are per-entity, not per-scene: one for every agent whose ring the view
/// can see.
///
/// The tracked scene's map diamond is larger than the view, so the count is the
/// *visible* population rather than the alive one — and the two rects differ,
/// because a 48 px sprite quad anchored on its feet and a `2r`-wide floor
/// ellipse do not leave the view at the same moment.
#[test]
fn a_ring_is_packed_for_every_agent() {
    let mut h = bodied_harness(256, 5);
    h.step_exact(19);

    h.runtime_mut().pack_groups();
    let iso = h.runtime().iso_view();
    let sprite = h.scenario().sprite_size_px() as f32;
    let radius_cells = h.sim().collision().radius_cells;
    let ring_size = [
        2.0 * radius_cells * iso.tile_w,
        2.0 * radius_cells * iso.tile_h,
    ];

    let want_rings = {
        let v = h.agents();
        (0..v.x.len())
            .filter(|&i| {
                let g = ground_point(&iso, v.x[i], v.y[i]);
                on_screen(
                    [g[0] - ring_size[0] * 0.5, g[1] - ring_size[1] * 0.5],
                    ring_size,
                    iso.view_size,
                )
            })
            .count()
    };
    let want_sprites = visible_sprite_count(h.agents(), &iso, sprite);
    assert!(
        want_rings > 0 && want_sprites > 0,
        "this case is only meaningful when the view holds something: {want_rings} rings, \
         {want_sprites} sprites"
    );

    let rings = h.runtime().ring_instances();
    assert_eq!(
        rings.len(),
        want_rings,
        "packed {} rings for {want_rings} on-screen agents",
        rings.len()
    );
    for (i, r) in rings.iter().enumerate() {
        assert!(
            r.is_ring(),
            "ring {i} does not carry the ring sentinel; the shader would sample it as a sprite"
        );
    }

    // The sprites are still there — "rings appear" must not mean "sprites left".
    let packed: usize = h
        .runtime()
        .draw_groups()
        .iter()
        .map(|g| g.instances.len())
        .sum();
    assert_eq!(
        packed, want_sprites,
        "the atlas groups lost agents to the rings"
    );

    // …and no sprite trips the ring branch. `ring_instances_keep_the_pinned_layout`
    // proves `frame_uv_rect` never emits a negative `u0`; this asks the real
    // packed groups — the instances that actually reach the GPU — the same
    // question, so a future packer that synthesised its own UVs could not
    // silently turn sprites into rings.
    for inst in h.runtime().draw_groups().iter().flat_map(|g| &g.instances) {
        assert!(
            !inst.is_ring(),
            "a packed sprite carries the ring sentinel: uv_rect {:?}",
            inst.uv_rect
        );
    }
}

/// No body, no ring. A bodyless scene has nothing to draw a contact circle
/// around, and a ring of radius zero would be a lie rather than a nicety.
#[test]
fn a_bodyless_scene_packs_no_rings() {
    let spec = GridSpec::new(24, 16, Cell { x: 23, y: 8 })
        .with_spawns(vec![Cell { x: 1, y: 8 }])
        .with_agents(32);
    assert_eq!(
        spec.collision_radius_q8, 0,
        "this case is only meaningful on a bodyless scene"
    );
    let mut h = Harness::grid(spec).build().expect("bodyless grid");
    h.step_exact(7);

    h.runtime_mut().pack_groups();
    assert!(
        h.runtime().ring_instances().is_empty(),
        "a bodyless scene packed {} rings",
        h.runtime().ring_instances().len()
    );

    // Still a rendered frame: the sprites are packed exactly as before.
    let packed: usize = h
        .runtime()
        .draw_groups()
        .iter()
        .map(|g| g.instances.len())
        .sum();
    assert_eq!(packed, h.alive_count());
}

/// The ring shows the radius the simulation actually separates on.
///
/// This is the whole point of the overlay: a ring that disagrees with the sim
/// is worse than no ring.
///
/// Two things make that claim hard to assert honestly, and both are handled
/// deliberately:
///
/// 1. **The expectation must not flow through the code under test.**
///    `want_diameter` is recomputed straight from the scenario's raw
///    `collision_radius_q8`, *not* via `runtime::ring_radius_px`. Routing both
///    sides through the production helper would make `ring_radius_px(c, r) {
///    r * c + 3.0 }` pass.
/// 2. **The tracked scene hides wrong derivations behind a coincidence.** T0
///    tuned every tracked scene so the body is exactly half a sprite, so
///    `2 * radius_px == sprite_size_px` there — which means "derive the body
///    from the sprite size" would also pass. `the_ring_traces_a_body_that_is_not_half_a_sprite`
///    below breaks that coincidence on purpose; this case keeps the tracked-scene
///    relationship because it is what makes the ring sit on the sprite's edge.
///
/// It also cross-checks the scenario's radius against `Simulation::collision()`
/// — the value the separation pass actually pushes on — so the two derivations
/// cannot drift apart unnoticed.
///
/// Under the isometric projection the drawn body is an **ellipse**: a circle
/// lying on a 2:1 floor is twice as wide on screen as it is tall. The height is
/// therefore still the body's true diameter, and the width is twice it — which
/// is what the size assertions below say.
///
/// The per-index pairing (ring `i` belongs to agent `i`) moved to
/// [`ring_and_sprite_stand_on_the_same_ground_point`]: on this scene the map
/// diamond is larger than the view, so the cull drops agents and packed index
/// `i` is no longer SoA index `i`.
#[test]
fn the_ring_traces_the_real_body() {
    let mut h = bodied_harness(64, 11);
    h.step_exact(11);

    let cell = h.scenario().cell_size_px() as f32;
    let sprite = h.scenario().sprite_size_px() as f32;
    let radius_cells = h.scenario().collision_radius_cells();

    // The ring is packed from the *sim's* radius; assert the scenario agrees
    // with it before using the scenario to state the expectation.
    assert_eq!(
        radius_cells,
        h.sim().collision().radius_cells,
        "the scenario and the simulation disagree on the body radius; the ring \
         would show one of two different numbers"
    );

    // Independent of `ring_radius_px`: raw Q8 field → cells → pixels.
    let want_diameter = 2.0 * (h.scenario().collision_radius_q8() as f32 / 256.0) * cell;
    assert_eq!(
        want_diameter,
        2.0 * ring_radius_px(cell, radius_cells),
        "the production radius expression no longer agrees with the raw scenario field"
    );

    // T0 tuned the body to exactly half a sprite, which is what makes "the ring
    // sits on the sprite's edge" true. Both sides come from the scenario, so
    // this pins the relationship, not a number.
    assert!(
        (want_diameter - sprite).abs() < 1e-3,
        "the body is {want_diameter} px across but the sprite is {sprite} px; the ring \
         can no longer sit on the sprite edge"
    );

    h.runtime_mut().pack_groups();
    let iso = h.runtime().iso_view();

    // Height is the body's diameter; width is twice it, because the body is a
    // circle seen on a 2:1 floor. Derived from the raw scenario field above,
    // then cross-checked against the production expression.
    let want_size = [2.0 * want_diameter, want_diameter];
    assert_eq!(
        want_size,
        ring_quad_size_px(iso.tile_w, iso.tile_h, radius_cells),
        "the production ring-quad expression no longer agrees with the raw scenario field"
    );

    // How many rings the view can hold, by this test's own rect arithmetic.
    let want_rings = {
        let v = h.agents();
        (0..v.x.len())
            .filter(|&i| {
                let g = ground_point(&iso, v.x[i], v.y[i]);
                on_screen(
                    [g[0] - want_size[0] * 0.5, g[1] - want_size[1] * 0.5],
                    want_size,
                    iso.view_size,
                )
            })
            .count()
    };

    // Every packed ring is that ellipse, carries the tuned band, and is centred
    // on an agent's ground point — as a **multiset**, consumed one agent per
    // ring. The cull means packed order is not SoA order on this scene, so the
    // pairing cannot be by index here; but counting rather than merely testing
    // membership is what stops a packer that emitted two rings for one agent,
    // or `agents.x[i & !1]`, from passing.
    let mut agent_grounds: BTreeMap<(i64, i64), usize> = {
        let mut m = BTreeMap::new();
        let v = h.agents();
        for i in 0..v.x.len() {
            let g = ground_point(&iso, v.x[i], v.y[i]);
            *m.entry((q256(g[0]), q256(g[1]))).or_default() += 1;
        }
        m
    };
    let rings = h.runtime().ring_instances().to_vec();
    assert_eq!(
        rings.len(),
        want_rings,
        "packed {} rings for {want_rings} on-screen agents",
        rings.len()
    );
    assert!(want_rings > 0, "no ring reached the view to check");
    for (i, r) in rings.iter().enumerate() {
        assert_eq!(
            r.size, want_size,
            "ring {i} is {:?} px across, not the {radius_cells}-cell body on a \
             {}x{} px tile",
            r.size, iso.tile_w, iso.tile_h
        );
        assert_eq!(r.uv_rect[1], RING_INNER, "ring {i} inner radius");
        assert_eq!(r.uv_rect[2], RING_OUTER, "ring {i} outer radius");
        let centre = (
            q256(r.pos[0] + r.size[0] * 0.5),
            q256(r.pos[1] + r.size[1] * 0.5),
        );
        let left = agent_grounds.get_mut(&centre).unwrap_or_else(|| {
            panic!("ring {i} is centred at {centre:?}, which is no agent's ground point")
        });
        assert!(
            *left > 0,
            "ring {i} is the second ring centred at {centre:?}, but only one agent \
             stands there"
        );
        *left -= 1;
    }

    // The ring's quad half-height is exactly the body radius in cell space, so
    // the helper the rest of the repo reads still means what it says.
    assert_eq!(want_size[1] * 0.5, ring_radius_px(cell, radius_cells));
}

/// A ring and the sprite it belongs to stand on the **same** projected ground
/// point — the ring on the floor, the sprite's feet in it.
///
/// Proven on a grid small enough that the whole map projects inside the view,
/// so nothing is culled and packed index `i` really is SoA index `i`. That is
/// what makes the per-agent pairing assertable at all; an SoA desync between
/// `x/y` and the ring buffer is invisible to any multiset comparison.
#[test]
fn ring_and_sprite_stand_on_the_same_ground_point() {
    // 1536 q8 = 6 cells of body against a 30 px sprite on a 4 px cell.
    let spec = GridSpec::new(24, 16, Cell { x: 22, y: 8 })
        .with_spawns(vec![Cell { x: 1, y: 8 }, Cell { x: 1, y: 4 }])
        .with_agents(48)
        .with_collision(1_536, 256);
    let mut h = Harness::grid(spec).build().expect("bodied grid");
    h.step_exact(9);
    h.runtime_mut().pack_groups();

    let iso = h.runtime().iso_view();
    let sprite = h.scenario().sprite_size_px() as f32;
    let alive = h.alive_count();
    assert_eq!(
        visible_sprite_count(h.agents(), &iso, sprite),
        alive,
        "this grid must project entirely inside the view, or index `i` is not agent `i`"
    );

    {
        let rings = h.runtime().ring_instances();
        let v = h.agents();
        assert_eq!(rings.len(), v.x.len(), "one ring per agent, in SoA order");
        for (i, r) in rings.iter().enumerate() {
            let want = ground_point(&iso, v.x[i], v.y[i]);
            let centre = [r.pos[0] + r.size[0] * 0.5, r.pos[1] + r.size[1] * 0.5];
            assert!(
                (centre[0] - want[0]).abs() < 1e-3 && (centre[1] - want[1]).abs() < 1e-3,
                "ring {i} is centred at {centre:?} but agent {i} stands at {want:?}"
            );
        }
    }

    // …and the ring centres are the sprite *feet*. Compared as multisets
    // because nothing promises intra-bucket ordering in the atlas groups.
    let ring_centres: BTreeMap<(i64, i64), usize> = {
        let mut m = BTreeMap::new();
        for r in h.runtime().ring_instances() {
            *m.entry((
                q256(r.pos[0] + r.size[0] * 0.5),
                q256(r.pos[1] + r.size[1] * 0.5),
            ))
            .or_default() += 1;
        }
        m
    };
    let sprite_feet: BTreeMap<(i64, i64), usize> = {
        let mut m = BTreeMap::new();
        for inst in h.runtime().draw_groups().iter().flat_map(|g| &g.instances) {
            *m.entry((
                q256(inst.pos[0] + inst.size[0] * 0.5),
                q256(inst.pos[1] + inst.size[1]),
            ))
            .or_default() += 1;
        }
        m
    };
    assert_eq!(
        ring_centres, sprite_feet,
        "the rings are not under the feet of the sprites they belong to"
    );
}

/// The ring follows the *body*, not the sprite — proven on a scene where the
/// two genuinely differ.
///
/// Every tracked scene has `2 * collision_radius == sprite_size_px`, so on
/// those a packer that derived the ring from the sprite size would be
/// indistinguishable from one that read `collision_radius_q8`. This grid sets
/// a body of 1.5 cells against a 30 px sprite, so the two answers are 12 px
/// and 30 px and only the correct derivation passes.
#[test]
fn the_ring_traces_a_body_that_is_not_half_a_sprite() {
    // 384 q8 = 1.5 cells; the grid's sprite is 30 px and its cell 4 px.
    let spec = GridSpec::new(24, 16, Cell { x: 23, y: 8 })
        .with_spawns(vec![Cell { x: 1, y: 8 }])
        .with_agents(16)
        .with_collision(384, 256);
    let mut h = Harness::grid(spec).build().expect("decoupled grid");
    h.step_exact(5);

    let cell = h.scenario().cell_size_px() as f32;
    let sprite = h.scenario().sprite_size_px() as f32;
    let radius_cells = h.sim().collision().radius_cells;
    assert_eq!(radius_cells, 1.5, "grid body radius");

    let want_diameter = 2.0 * radius_cells * cell; // 12 px
    assert!(
        (want_diameter - sprite).abs() > 1.0,
        "this case is only meaningful when the body ({want_diameter} px) and the \
         sprite ({sprite} px) disagree"
    );
    // …and the isometric ellipse must not accidentally match the sprite either,
    // on *either* axis, or a sprite-derived packer could still pass.
    let want_size = [2.0 * want_diameter, want_diameter]; // 24 x 12 px
    assert!(
        (want_size[0] - sprite).abs() > 1.0 && (want_size[1] - sprite).abs() > 1.0,
        "the projected body {want_size:?} must differ from the sprite ({sprite} px) on \
         both axes"
    );

    h.runtime_mut().pack_groups();
    let rings = h.runtime().ring_instances();
    assert_eq!(rings.len(), h.alive_count());
    for (i, r) in rings.iter().enumerate() {
        assert_eq!(
            r.size, want_size,
            "ring {i} is {:?} px across; the body projects to {want_size:?} and the \
             sprite is {sprite} px, so this ring is tracking the wrong one",
            r.size
        );
    }
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
        let clip = world_to_clip(full.0, full.1, corner, view, 0.0);
        assert!(
            (clip[0] - want[0]).abs() < 1e-6 && (clip[1] - want[1]).abs() < 1e-6,
            "corner {corner:?} → clip {:?}, expected {want:?}",
            [clip[0], clip[1]]
        );
        assert_eq!(
            [clip[2], clip[3]],
            [0.0, 1.0],
            "clip w is fixed; z is the key"
        );
    }

    // The depth key is carried through untouched rather than recomputed from
    // the corner. This is a *signature-shape* check and nothing more: `depth`
    // is an argument the function never derives, so it cannot fail for any
    // mutation of the depth logic — which lives in `iso_depth` and in the
    // shader. The claim that one quad carries one key is a property of the
    // vertex stage and is asserted where it can actually fail, on the GPU, in
    // `an_agent_in_front_occludes_one_behind`.
    for depth in [0.0f32, 0.25, 1.0] {
        for corner in [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]] {
            let clip = world_to_clip(full.0, full.1, corner, view, depth);
            assert_eq!(
                clip[2], depth,
                "corner {corner:?} reported depth {} instead of {depth}",
                clip[2]
            );
        }
    }

    // Y is flipped, not merely scaled: a sprite in the upper half of the view
    // must sit in the positive half of clip space.
    let upper = world_to_clip([0.0, 0.0], [10.0, 10.0], [0.0, 0.0], view, 0.0);
    assert!(upper[1] > 0.0, "a sprite near the top must have clip y > 0");
    let lower = world_to_clip(
        [0.0, VIEW_HEIGHT as f32 - 10.0],
        [10.0, 10.0],
        [0.0, 0.0],
        view,
        0.0,
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
            let clip = world_to_clip(pos, size, corner, view, 0.0);
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

/// Constants that exist **twice** — once in Rust, once as a `#define` in
/// `shaders/sprite.hlsl` — really do agree.
///
/// Nothing else can catch this. The depth buffer is created
/// `DEPTH_STENCIL_TARGET`-only with `StoreOp::DONT_CARE`, so no test can read a
/// depth value back; a shader whose epsilon or cutoff drifted from its Rust
/// mirror renders a frame that is merely wrong, and every GPU probe in this file
/// still passes. The canonical HLSL is a tracked file, so asserting on its text
/// costs one `include_str!`.
///
/// This pins the constants **this ticket introduced**. `RING_SENTINEL` /
/// `MMD_RING_THRESHOLD` carry the same duplication from T8 and are deliberately
/// left alone here rather than picked up as a drive-by.
#[test]
fn shader_defines_match_their_rust_mirrors() {
    const HLSL: &str = include_str!("../../../shaders/sprite.hlsl");

    // The define's value is parsed and compared as an `f32` rather than matched
    // as text: the shader spells it `0.0000152587890625` and Rust's own
    // formatter would render the same number `1.5258789e-5`, so a text match
    // would pin the *spelling* and break on a harmless reformat while still
    // missing a genuine numeric drift.
    let defined = |name: &str| -> f32 {
        let line = HLSL
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("#define {name} ")))
            .unwrap_or_else(|| panic!("shaders/sprite.hlsl no longer defines {name}"));
        line.trim()
            .rsplit(' ')
            .next()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or_else(|| panic!("cannot read a float out of `{line}`"))
    };

    assert_eq!(
        defined("MMD_DEPTH_EPSILON"),
        ISO_DEPTH_EPSILON,
        "the sprite depth floor exists once in Rust (ISO_DEPTH_EPSILON) and once in \
         the shader, and no test can read a depth value back to catch a disagreement"
    );
    // The floor must be exactly one quantum of the D16_UNORM attachment: any
    // smaller and it quantises back to the far plane the GREATER test discards.
    assert_eq!(ISO_DEPTH_EPSILON, 1.0 / 65_536.0);

    // The alpha cutout is the other constant this ticket added to the shader.
    // It has no Rust mirror by design — only the fragment stage uses it — so
    // what is pinned is the value every host golden was regenerated under.
    assert_eq!(
        defined("MMD_ALPHA_CUTOFF"),
        0.5,
        "every host golden was regenerated under a 0.5 alpha cutoff; moving it \
         silently re-baselines what the goldens mean"
    );
}

/// The depth key is a property of where the agent *stands*, not of the quad it
/// is drawn with.
///
/// An agent one cell nearer (`+y`, which walks down the screen) gets the
/// **larger** key, which is why the pipeline tests `GREATER` against an
/// attachment cleared to `0`. Under `LESS` the nearer agent would lose and the
/// horde would draw back to front.
///
/// The other half of the claim — that all four corners of one quad carry the
/// *same* key, so a sprite cannot slice into its neighbours — cannot be
/// asserted from here. It is a property of the vertex stage, and the CPU mirror
/// takes the key as an argument rather than deriving it. It is asserted where
/// it can actually fail, on the GPU, by the reversed draw order in
/// [`an_agent_in_front_occludes_one_behind`].
#[test]
fn depth_is_the_ground_point_not_the_quad() {
    let spec = GridSpec::new(24, 16, Cell { x: 22, y: 8 })
        .with_spawns(vec![Cell { x: 1, y: 8 }])
        .with_agents(2);
    let mut h = Harness::grid(spec).build().expect("grid");

    // Two agents one cell apart in +y, deliberately overlapping on screen.
    h.sim_mut().set_position(0, 8.5, 6.5);
    h.sim_mut().set_position(1, 8.5, 7.5);
    h.runtime_mut().pack_groups();

    let iso = h.runtime().iso_view();
    let sprite = h.scenario().sprite_size_px() as f32;

    let key = |cx: f32, cy: f32| -> f32 {
        let g = ground_point(&iso, cx, cy);
        let pos = [g[0] - sprite * 0.5, g[1] - sprite];
        // What the shader computes: the quad's bottom edge.
        iso_depth(pos[1] + sprite, iso.depth_scale, iso.depth_bias)
    };

    let farther = key(8.5, 6.5);
    let nearer = key(8.5, 7.5);
    assert!(
        nearer > farther,
        "the agent one cell nearer has depth {nearer}, the one behind it {farther}; \
         under a LESS test the nearer one would lose"
    );

    // Neither key is the far plane. Exactly 0 is discarded by the GREATER test
    // rather than sorted last, so the shader floors sprites at one depth
    // quantum — see `ISO_DEPTH_EPSILON`.
    for (name, d) in [("farther", farther), ("nearer", nearer)] {
        assert!(
            d >= ISO_DEPTH_EPSILON,
            "the {name} agent's key is {d}, which the depth test would discard"
        );
    }

    // The packed instances really are the two quads this reasoned about.
    let packed: Vec<SpriteInstance> = h
        .runtime()
        .draw_groups()
        .iter()
        .flat_map(|g| g.instances.iter().copied())
        .collect();
    assert_eq!(packed.len(), 2, "both agents must be on screen");
    let mut ground_ys: Vec<i64> = packed.iter().map(|i| q256(i.pos[1] + i.size[1])).collect();
    ground_ys.sort_unstable();
    let want_far = q256(ground_point(&iso, 8.5, 6.5)[1]);
    let want_near = q256(ground_point(&iso, 8.5, 7.5)[1]);
    assert_eq!(
        ground_ys,
        vec![want_far.min(want_near), want_far.max(want_near)],
        "the packed quads' bottom edges are not the two agents' ground points"
    );
}

/// A quad wholly outside the view is never pushed; one that still straddles an
/// edge is.
///
/// The map diamond is deliberately larger than the view, so this is not an
/// optimisation detail — without it the gate scene would upload and rasterize
/// several times the instances it can show.
#[test]
fn offscreen_quads_are_culled() {
    // The gate scene: 480 x 270 cells under an 8 x 4 tile is a 3000 x 1500 px
    // diamond against a 1920 x 1080 view, so cells past every edge exist.
    let mut h = Harness::builder(ScenarioSource::path(scene_path(COLLISION_SPRITE_SCENE)))
        .agents(1)
        .build()
        .expect("tracked collision scene loads");
    let iso = h.runtime().iso_view();
    let sprite = h.scenario().sprite_size_px() as f32;
    let view = iso.view_size;

    let pack_at = |h: &mut Harness, cx: f32, cy: f32| -> usize {
        h.sim_mut().set_position(0, cx, cy);
        h.runtime_mut().pack_groups();
        h.runtime()
            .draw_groups()
            .iter()
            .map(|g| g.instances.len())
            .sum()
    };

    // Screen position → cell position. `sx = ox + (cx - cy) * tw / 2` and
    // `sy = oy + (cx + cy) * th / 2`, inverted.
    let cell_at = |sx: f32, sy: f32| -> (f32, f32) {
        let a = (sx - iso.origin[0]) / (iso.tile_w * 0.5); // cx - cy
        let b = (sy - iso.origin[1]) / (iso.tile_h * 0.5); // cx + cy
        ((b + a) * 0.5, (b - a) * 0.5)
    };

    // The quad is anchored bottom-centre, so a ground point at these screen
    // coordinates puts the quad fully outside the named edge.
    let outside = [
        ("left", -sprite, view[1] * 0.5),
        ("right", view[0] + sprite, view[1] * 0.5),
        ("top", view[0] * 0.5, -1.0),
        ("bottom", view[0] * 0.5, view[1] + sprite),
    ];
    for (edge, sx, sy) in outside {
        let (cx, cy) = cell_at(sx, sy);
        // Sanity: this really is outside, by the test's own rect arithmetic.
        let pos = [sx - sprite * 0.5, sy - sprite];
        assert!(
            !on_screen(pos, [sprite, sprite], view),
            "{edge}: the case itself places a quad {pos:?} that is still on screen"
        );
        assert_eq!(
            pack_at(&mut h, cx, cy),
            0,
            "an agent past the {edge} edge (cell {cx},{cy} → quad {pos:?}) still packed \
             an instance"
        );
    }

    // Straddling: one pixel of the quad is inside, so the instance survives.
    let straddling = [
        ("left", -sprite * 0.5 + 1.0, view[1] * 0.5),
        ("right", view[0] + sprite * 0.5 - 1.0, view[1] * 0.5),
        ("top", view[0] * 0.5, sprite - 1.0),
        ("bottom", view[0] * 0.5, view[1] - 1.0),
    ];
    for (edge, sx, sy) in straddling {
        let (cx, cy) = cell_at(sx, sy);
        let pos = [sx - sprite * 0.5, sy - sprite];
        assert!(
            on_screen(pos, [sprite, sprite], view),
            "{edge}: the case itself places a quad {pos:?} that is already off screen"
        );
        assert_eq!(
            pack_at(&mut h, cx, cy),
            1,
            "an agent straddling the {edge} edge (quad {pos:?}) was culled"
        );
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

/// Every tracked manifest that pins the shader and atlas hashes still pins the
/// *live* ones — discovered by walking the tree, not by remembering a list.
///
/// This exists because editing `shaders/sprite.hlsl` moves
/// `shader_canonical_sha256` in **five** separate manifests, and only three of
/// them are obvious. The two under `lab/fixtures/*-candidate/golden/` are read
/// by the mmd-lab merge gate; the two placeholder families under `lab/goldens/`
/// are reachable only from hardware this project does not have. Before this
/// test, a missed pin in the placeholder families would ship silently and
/// surface on a reference host months later.
///
/// It is deliberately host-independent: it compares tracked bytes against
/// tracked bytes and needs no GPU, so it fails on every machine rather than
/// only on the one that owns the family. Note that `golden_frame_matches`
/// cannot cover this — it builds its `HostBinding` from the manifest under
/// test, so it is structurally incapable of noticing a stale pin.
#[test]
fn every_tracked_manifest_pins_the_live_shader_and_atlas() {
    let root = workspace_root();
    let (want_atlas, want_shader) = host_binding_hashes(&root).expect("host hashes");

    let mut manifests: Vec<PathBuf> = Vec::new();
    for dir in std::fs::read_dir(goldens_root()).expect("lab/goldens") {
        let path = dir.expect("entry").path().join("manifest.json");
        if path.is_file() {
            manifests.push(path);
        }
    }
    for dir in std::fs::read_dir(root.join("lab/fixtures")).expect("lab/fixtures") {
        let path = dir
            .expect("entry")
            .path()
            .join("golden")
            .join("manifest.json");
        if path.is_file() {
            manifests.push(path);
        }
    }
    manifests.sort();

    // A discovery walk that found nothing would pass vacuously.
    assert!(
        manifests.len() >= 5,
        "expected at least the three golden families plus the two candidate \
         fixtures, found {manifests:?}"
    );

    for path in &manifests {
        let manifest = load_golden_manifest(path).unwrap_or_else(|e| {
            panic!("{}: {e}", path.display());
        });
        let rel = path.strip_prefix(&root).unwrap_or(path).display();
        assert_eq!(
            manifest.shader_canonical_sha256, want_shader,
            "{rel} pins a stale shaders/sprite.hlsl. Editing the shader moves this \
             hash in every manifest that records it — re-pin them all, or the lane \
             that reads this one fails with ManifestDrift on a host you cannot test."
        );
        assert_eq!(
            manifest.atlas_manifest_sha256, want_atlas,
            "{rel} pins a stale atlas manifest"
        );
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
///
/// **Scope: `x` and `y` only.** `SV_Position.z` cannot be read back — the depth
/// attachment is created `DEPTH_STENCIL_TARGET`-only with `StoreOp::DONT_CARE`,
/// so it is neither sampleable nor downloadable. The mirror's `z` is bound to
/// the GPU indirectly, through the draw order that
/// [`an_agent_in_front_occludes_one_behind`] resolves. Asserting `z` here
/// against the same `iso_depth` the mirror was fed would check the mirror
/// against itself.
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

    // The depth the shader will emit for this instance: `saturate(ground_y *
    // depth_scale + depth_bias)` off the quad's bottom edge, through the
    // renderer's own live scalars.
    let (ds, db) = r.depth_params();
    let depth = iso_depth(pos[1] + size[1], ds, db);
    assert!(
        depth > 0.0 && depth < 1.0,
        "the probe must sit strictly between the near and far planes, not at {depth}"
    );

    let top_left = clip_to_pixel(world_to_clip(pos, size, [-0.5, -0.5], view, depth), view);
    let bottom_right = clip_to_pixel(world_to_clip(pos, size, [0.5, 0.5], view, depth), view);
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

/// The whole point of the ticket: a unit in front covers a unit behind, with
/// nobody sorting anything.
///
/// Both sprites go in **atlas group 0**, so the only thing separating them is
/// the depth attachment. Tints separate them visually: premultiplied opaque
/// green and red over a cleared target come back as exactly themselves.
///
/// **Both draw orders are exercised, and the second one is load-bearing.**
///
/// - Nearer first, farther second — the farther one must be rejected.
/// - Farther first, nearer second — the nearer one must overwrite it.
///
/// Only the second order can catch the defect this ticket calls the one that
/// must not be got wrong. If the vertex stage emitted its interpolated
/// `world.y` instead of the quad's bottom edge, both quads would emit an
/// *identical* `z` at every pixel they share. Under the first order that tie
/// still fails the `GREATER` test and the nearer sprite still survives — the
/// test passes with the bug in. Under the second order the tie means the nearer
/// sprite is the one rejected, and the assertion fires.
///
/// That asymmetry is also why the pipeline uses strict `GREATER` rather than
/// `GreaterOrEqual`: `GreaterOrEqual` would let the tie through and make this
/// case blind again.
#[test]
fn an_agent_in_front_occludes_one_behind() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("an_agent_in_front_occludes_one_behind") else {
        return;
    };

    let size = [240.0f32, 240.0f32];
    // `near` has the larger bottom edge, so the larger depth key.
    let near = [600.0f32, 400.0f32];
    let far = [700.0f32, 300.0f32];
    let near_tint = [0.0f32, 1.0, 0.0, 1.0];
    let far_tint = [1.0f32, 0.0, 0.0, 1.0];

    let (ds, db) = r.depth_params();
    let near_depth = iso_depth(near[1] + size[1], ds, db);
    let far_depth = iso_depth(far[1] + size[1], ds, db);
    assert!(
        near_depth > far_depth,
        "the case is only meaningful when the two quads have different depths"
    );

    let uv = frame_uv_rect(0, 0);
    let near_inst = SpriteInstance::new(near, size, uv, near_tint);
    let far_inst = SpriteInstance::new(far, size, uv, far_tint);

    let mut near_only = empty_groups();
    near_only[0].instances = vec![near_inst];
    let alone = r
        .draw_offscreen_readback(&near_only)
        .expect("near-only readback");

    // The two quads overlap over x in [700, 840), y in [400, 540).
    //
    // Restricted to pixels where the nearer sprite alone is **fully opaque**
    // (alpha 255), for two independent reasons. The art is a cutout with
    // transparent holes, and the alpha test discards those fragments without
    // writing depth, so the farther sprite legitimately shows *through* the
    // gaps. And blending is still enabled, so the nearer sprite's partial-alpha
    // fringe (the atlases carry a band at alpha 200–254) legitimately
    // composites over whatever is behind it, which differs between the two draw
    // orders. Neither is an ordering defect. Where the nearer sprite is opaque,
    // though, the result cannot depend on what is behind it or on submit order.
    let overlap_verdict = |frame: &Readback| -> (u32, u32) {
        let (mut kept, mut lost) = (0u32, 0u32);
        for y in 400..540u32 {
            for x in 700..840u32 {
                if alone.pixel(x, y)[3] != 255 {
                    continue;
                }
                if frame.pixel(x, y) == alone.pixel(x, y) {
                    kept += 1;
                } else {
                    lost += 1;
                }
            }
        }
        (kept, lost)
    };

    for (order, instances) in [
        ("nearer first, farther second", vec![near_inst, far_inst]),
        ("farther first, nearer second", vec![far_inst, near_inst]),
    ] {
        let mut groups = empty_groups();
        groups[0].instances = instances;
        let frame = r.draw_offscreen_readback(&groups).expect("readback");

        let (kept, lost) = overlap_verdict(&frame);
        assert!(
            kept + lost > 1_000,
            "{order}: the nearer sprite painted only {} pixels of the overlap — it \
             barely drew",
            kept + lost
        );
        assert_eq!(
            lost,
            0,
            "{order}: {lost} of {} pixels the nearer sprite painted do not belong to \
             it in the combined frame. If this fired only for 'farther first', the \
             vertex stage is deriving depth per-vertex instead of from the quad's \
             bottom edge, so the two quads tie and the nearer one loses the \
             GREATER test.",
            kept + lost
        );

        // Not vacuous: the farther sprite really did draw, in the band above the
        // near quad where it is alone.
        let mut red_solo = 0u32;
        for y in 300..400u32 {
            for x in 700..940u32 {
                let p = frame.pixel(x, y);
                if p != [0, 0, 0, 0] && p[0] > p[1] {
                    red_solo += 1;
                }
            }
        }
        assert!(
            red_solo > 1_000,
            "{order}: the farther sprite left only {red_solo} pixels of its own; \
             nothing was occluded because nothing was there"
        );
    }
}

/// A ring is an overlay, so a unit standing in front of it must not hide it.
///
/// This is falsifiable rather than tautological because of what the ring emits:
/// `z = 0`, the far end of the map diamond, which fails the sprite pipeline's
/// `GREATER` test against a depth buffer cleared to `0` *everywhere*. A ring
/// drawn on the sprite pipeline would therefore not appear at all, and one
/// drawn on a depth-*writing* pipeline would stamp the far plane over the
/// sprites it annotates. Only the second, depth-free pipeline renders this
/// frame.
#[test]
fn rings_are_never_occluded() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("rings_are_never_occluded") else {
        return;
    };

    let sprite_pos = [700.0f32, 300.0f32];
    let sprite_size = [320.0f32, 320.0f32];
    let ring_side = 256.0f32;
    // Centred inside the sprite's footprint, so the band crosses it.
    let ring_pos = [
        sprite_pos[0] + sprite_size[0] * 0.5 - ring_side * 0.5,
        sprite_pos[1] + sprite_size[1] * 0.5 - ring_side * 0.5,
    ];

    let groups = single_sprite_groups(sprite_pos, sprite_size);
    let without = r
        .draw_offscreen_readback(&groups)
        .expect("sprite-only readback");

    let rings = [SpriteInstance::ring(
        ring_pos,
        [ring_side, ring_side],
        RING_INNER,
        RING_OUTER,
        RING_TINT,
    )];
    let with = r
        .draw_offscreen_readback_with_rings(&groups, &rings)
        .expect("sprite + ring readback");

    // Every pixel on the ring's band that the sprite already covers. Counted
    // over the whole annulus rather than probed at one coordinate: the sprite
    // art has transparent holes, so a single pixel is a coin flip.
    let (x0, y0) = (ring_pos[0] as u32, ring_pos[1] as u32);
    let (x1, y1) = (
        (ring_pos[0] + ring_side) as u32,
        (ring_pos[1] + ring_side) as u32,
    );
    let (mut covered, mut changed) = (0u32, 0u32);
    for py in y0..y1 {
        for px in x0..x1 {
            let u = (px as f32 + 0.5 - ring_pos[0]) / ring_side - 0.5;
            let v = (py as f32 + 0.5 - ring_pos[1]) / ring_side - 0.5;
            let d = u.hypot(v);
            // A 1.5 px margin keeps the two rasterization boundary rows out.
            let margin = 1.5 / ring_side;
            if d < RING_INNER + margin || d > RING_OUTER - margin {
                continue;
            }
            if without.pixel(px, py) == [0, 0, 0, 0] {
                continue;
            }
            covered += 1;
            if with.pixel(px, py) != without.pixel(px, py) {
                changed += 1;
            }
        }
    }
    assert!(
        covered > 50,
        "only {covered} band pixels are behind the sprite; there is nothing here to be \
         occluded by"
    );
    assert_eq!(
        changed,
        covered,
        "{} of {covered} band pixels behind the sprite are unchanged by the ring — \
         the overlay was occluded",
        covered - changed
    );
}

/// The ring is a *ring*: hollow in the middle, lit on the circumference.
///
/// This is the case that distinguishes the feature from a filled disc, and it
/// can only be answered by the real fragment shader — the CPU packer emits the
/// same instance either way.
///
/// It draws **two** rings with different bands. One would prove only that
/// *some* annulus appears: a shader that ignored the interpolated
/// `(inner, outer)` payload and hardcoded the constants would pass. Two bands
/// checked against their own values pin the per-instance plumbing that
/// `ring_instances_keep_the_pinned_layout` can only prove in CPU memory.
#[test]
fn a_ring_is_hollow() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("a_ring_is_hollow") else {
        return;
    };

    // Deliberately large: the band is `outer - inner` in quad units, so a big
    // quad makes it several pixels wide and the verdict cannot hinge on one
    // rasterization edge case.
    let side = 256.0f32;
    let tint = RING_TINT;
    // (position, inner, outer). The second band is deliberately *not*
    // RING_INNER/RING_OUTER, and is both thicker and further in.
    let cases = [
        ([700.0f32, 300.0f32], RING_INNER, RING_OUTER),
        ([1200.0, 300.0], 0.25, 0.32),
    ];
    let groups = empty_groups();
    let rings: Vec<SpriteInstance> = cases
        .iter()
        .map(|&(pos, inner, outer)| SpriteInstance::ring(pos, [side, side], inner, outer, tint))
        .collect();

    let rb = r
        .draw_offscreen_readback_with_rings(&groups, &rings)
        .expect("offscreen readback");

    for &(pos, ring_inner, ring_outer) in &cases {
        assert_ring_band(&rb, pos, side, ring_inner, ring_outer, tint);
    }
}

/// Assert one rendered ring: hollow centre, lit circumference, a centre-row
/// profile matching the shader's own predicate, exactly two lit runs, the
/// right colour, and a footprint inside its own quad.
fn assert_ring_band(
    rb: &Readback,
    pos: [f32; 2],
    side: f32,
    ring_inner: f32,
    ring_outer: f32,
    tint: [f32; 4],
) {
    let band = format!("band {ring_inner}..{ring_outer} at {pos:?}");

    // Distance from the quad centre, in the same normalised units the shader
    // uses (`length(uv - 0.5)`).
    let d_at = |px: u32, py: u32| -> f32 {
        let u = (px as f32 + 0.5 - pos[0]) / side - 0.5;
        let v = (py as f32 + 0.5 - pos[1]) / side - 0.5;
        u.hypot(v)
    };
    let lit = |px: u32, py: u32| rb.pixel(px, py) != [0, 0, 0, 0];

    let cx = (pos[0] + side * 0.5).round() as u32;
    let cy = (pos[1] + side * 0.5).round() as u32;

    // 1. The centre is background. A disc would fail here and nowhere else.
    assert!(
        !lit(cx, cy),
        "the pixel at the ring's centre is lit ({:?}) — this is a disc, not a ring",
        rb.pixel(cx, cy)
    );

    // 2. A pixel on this instance's *own* circumference is not background, and
    //    carries this instance's tint. Colour matters: a shader that returned
    //    white, or sampled the bound atlas, would pass every geometric check
    //    here and still be wrong.
    let mid = (ring_inner + ring_outer) * 0.5;
    let edge_x = (pos[0] + side * (0.5 + mid)).round() as u32;
    assert!(
        lit(edge_x, cy),
        "{band}: the pixel at d≈{mid} on the circumference is background — nothing was drawn"
    );
    let want_rgba = tint.map(|c| (c * 255.0).round() as i32);
    let got_rgba = rb.pixel(edge_x, cy).map(i32::from);
    for (i, (got, want)) in got_rgba.iter().zip(want_rgba).enumerate() {
        assert!(
            (got - want).abs() <= 1,
            "{band}: circumference pixel is {got_rgba:?}, but the premultiplied \
             RING_TINT over a cleared target is {want_rgba:?} (channel {i}) — the ring \
             is not being painted with its own tint"
        );
    }

    // 3. The whole centre row agrees with the shader's own predicate, for *this
    //    instance's* band. A margin of ~1.5 px in `d` units skips the two
    //    boundary pixels, where rasterization may legitimately round either way.
    let margin = 1.5 / side;
    let (x0, x1) = (pos[0].floor() as u32, (pos[0] + side).ceil() as u32);
    let (mut lit_checked, mut dark_checked) = (0u32, 0u32);
    for px in x0..x1 {
        let d = d_at(px, cy);
        if d > ring_inner + margin && d < ring_outer - margin {
            assert!(
                lit(px, cy),
                "{band}: pixel ({px},{cy}) at d={d} is inside the band but dark"
            );
            lit_checked += 1;
        } else if d < ring_inner - margin || d > ring_outer + margin {
            assert!(
                !lit(px, cy),
                "{band}: pixel ({px},{cy}) at d={d} is outside the band but lit ({:?})",
                rb.pixel(px, cy)
            );
            dark_checked += 1;
        }
    }
    // Counted separately: one combined counter would still be satisfied by a
    // row that resolved only dark pixels and never entered the band at all.
    assert!(
        lit_checked > 4 && dark_checked > 4,
        "{band}: centre row resolved {lit_checked} in-band and {dark_checked} \
         out-of-band pixels — too few to constrain anything"
    );

    // 4. Exactly two lit runs on the centre row — left arc and right arc. One
    //    run means a filled disc; zero means nothing drew.
    let runs = {
        let mut runs = 0u32;
        let mut prev = false;
        for px in x0..x1 {
            let now = lit(px, cy);
            if now && !prev {
                runs += 1;
            }
            prev = now;
        }
        runs
    };
    assert_eq!(
        runs, 2,
        "{band}: the centre row has {runs} lit run(s); a hollow ring crosses it twice"
    );

    // 5. Nothing this instance drew escaped its own quad. Checked as "every lit
    //    pixel in a window around the quad is within `outer`" rather than via a
    //    whole-frame bbox, because the frame holds more than one ring.
    let (wx0, wy0) = (
        x0.saturating_sub(4),
        (pos[1].floor() as u32).saturating_sub(4),
    );
    let (wx1, wy1) = (
        (x1 + 4).min(rb.width),
        ((pos[1] + side).ceil() as u32 + 4).min(rb.height),
    );
    for py in wy0..wy1 {
        for px in wx0..wx1 {
            if lit(px, py) {
                let d = d_at(px, py);
                assert!(
                    d <= ring_outer + margin,
                    "{band}: pixel ({px},{py}) is lit at d={d}, outside the ring's \
                     own outer radius {ring_outer}"
                );
            }
        }
    }
}

/// Sprites and rings survive sharing one submit.
///
/// Every other case draws one or the other: `a_ring_is_hollow` passes four
/// empty groups, and the golden scene is ring-free. That leaves the shape the
/// app actually runs in — four atlas draws *then* the fifth ring range, with
/// the vertex buffer re-bound at the ring's byte offset and atlas 0 re-bound
/// after the group loop — exercised by nothing. An off-by-one in `ring_start`
/// or a rebind that corrupted the group draws would slip through.
#[test]
fn a_ring_and_a_sprite_share_a_pass() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("a_ring_and_a_sprite_share_a_pass") else {
        return;
    };

    let sprite_pos = [300.0f32, 200.0f32];
    let sprite_size = [96.0f32, 64.0f32];
    let ring_pos = [1200.0f32, 500.0f32];
    let ring_side = 256.0f32;

    // Baseline: the sprite alone.
    let sprite_only = r
        .draw_offscreen_readback(&single_sprite_groups(sprite_pos, sprite_size))
        .expect("sprite-only readback");
    let sprite_bbox = nonclear_bbox(&sprite_only).expect("the sprite must draw something");

    // Now the same sprite plus a ring, in one submit.
    let rings = [SpriteInstance::ring(
        ring_pos,
        [ring_side, ring_side],
        RING_INNER,
        RING_OUTER,
        RING_TINT,
    )];
    let both = r
        .draw_offscreen_readback_with_rings(&single_sprite_groups(sprite_pos, sprite_size), &rings)
        .expect("combined readback");

    // The ring really rendered, at its own band.
    assert_ring_band(
        &both, ring_pos, ring_side, RING_INNER, RING_OUTER, RING_TINT,
    );

    // …and the sprite is byte-for-byte what it was without the ring. The two
    // quads do not overlap, so anything but equality here means the extra
    // range disturbed the group draws.
    let (sx0, sy0, sx1, sy1) = sprite_bbox;
    assert!(
        sx1 as f32 <= ring_pos[0],
        "the two footprints must not overlap for this comparison to mean anything"
    );
    for py in sy0..sy1 {
        for px in sx0..sx1 {
            assert_eq!(
                both.pixel(px, py),
                sprite_only.pixel(px, py),
                "pixel ({px},{py}) of the sprite changed once a ring shared its submit"
            );
        }
    }
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
