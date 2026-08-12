//! GPU smoke tests. Unit layout/backend tests always run;
//! device tests are `#[ignore]` and need a real GPU + SDL3.
//!
//! GPU cases share process-global SDL; run serially (`--test-threads=1`).
//!
//! Renderer lifecycle (device creation, swapchain sizes, clean shutdown) and
//! instance/transform/golden correctness live in `render_correctness.rs` (T31),
//! whose GPU cases run by default and skip cleanly on a host without a device.
//! `tracked_atlas_hashes_are_enforced` below is the negative half of the atlas
//! manifest contract; its positive half is
//! `render_correctness.rs::atlas_manifest_matches_generated_pngs`.

#[cfg(any(target_os = "windows", target_os = "macos"))]
use mmd_engine::render::validate_device_props;
use mmd_engine::render::{
    ATLAS_COUNT, DrawGroup, REQUIRED_BACKEND, SLOT_RTS_BUILDINGS, SLOT_RTS_PROPS, SLOT_UI_FONT,
    SPRITE_SIZE_PX, ScenePass, SpriteInstance, SpriteRenderer, VIEW_HEIGHT, VIEW_WIDTH,
    frame_uv_rect, load_atlases, push_text, required_backend, validate_adapter_name,
    validate_backend_name, validate_macos_host_arch,
};
use mmd_engine::rts::{
    BOTTOM_PANEL_RECT, BuildingKind, RtsFrame, TOP_BAR_RECT, pack_frame, pack_hud,
};
use mmd_engine::testkit::RtsHarness;
use mmd_engine::workspace_root;
use std::sync::{Mutex, MutexGuard};

/// SDL GPU state is process-global; the two text GPU cases in this file share
/// this lock the same way `render_correctness.rs` shares its own, so they
/// never overlap a device with each other regardless of `cargo test`'s thread
/// fan-out.
static GPU_LOCK: Mutex<()> = Mutex::new(());

fn gpu_guard() -> MutexGuard<'static, ()> {
    GPU_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Env var that turns a device-unavailable skip into a hard failure — mirrors
/// `render_correctness.rs::REQUIRE_GPU_ENV`, duplicated here rather than
/// shared because the two files are separate test binaries.
const REQUIRE_GPU_ENV: &str = "MMD_REQUIRE_GPU";

fn gpu_is_required() -> bool {
    std::env::var_os(REQUIRE_GPU_ENV).is_some_and(|v| v != "0" && !v.is_empty())
}

fn skip(case: &str, reason: &str) {
    assert!(
        !gpu_is_required(),
        "{case}: skipped ({reason}) but {REQUIRE_GPU_ENV} is set — this host is \
         declared to have a GPU, so a skip is a missed verification"
    );
    eprintln!("SKIP {case}: {reason}");
}

/// A live renderer, or `None` when this host genuinely has no GPU device.
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

#[test]
fn instance_layout_is_stable() {
    // Re-export coverage via renderer constants + struct sizes.
    use mmd_engine::render::SpriteInstance;
    use std::mem::{align_of, offset_of, size_of};
    assert_eq!(size_of::<SpriteInstance>(), 48);
    assert_eq!(align_of::<SpriteInstance>(), 4);
    assert_eq!(offset_of!(SpriteInstance, pos), 0);
    assert_eq!(offset_of!(SpriteInstance, size), 8);
    assert_eq!(offset_of!(SpriteInstance, uv_rect), 16);
    assert_eq!(offset_of!(SpriteInstance, tint), 32);
}

#[test]
fn wrong_backend_rejected() {
    for bad in ["software", "opengles2", ""] {
        assert!(validate_backend_name(bad).is_err(), "{bad}");
    }
    assert!(validate_backend_name(required_backend()).is_ok());
    assert_eq!(required_backend(), REQUIRED_BACKEND);
}

#[test]
fn rejects_basic_renderer() {
    let err = validate_adapter_name("Microsoft Basic Render Driver").expect_err("basic");
    let msg = err.to_string();
    assert!(
        msg.to_ascii_lowercase().contains("basic render")
            || msg.to_ascii_lowercase().contains("rejected"),
        "{msg}"
    );
    // Combined props: correct host backend + Basic Render still fails adapter gate.
    // On non-Windows, driver check fails first when forcing d3d12 — call adapter alone above.
    #[cfg(target_os = "windows")]
    {
        assert!(validate_device_props(REQUIRED_BACKEND, "Microsoft Basic Render Driver").is_err());
        validate_device_props(REQUIRED_BACKEND, "AMD Radeon RX 6400").expect("rx6400");
    }
}

#[test]
fn metal_backend_required() {
    #[cfg(target_os = "macos")]
    {
        assert_eq!(required_backend(), "metal");
        validate_device_props(REQUIRED_BACKEND, "Apple M4").expect("m4 metal");
        assert!(validate_device_props("vulkan", "Apple M4").is_err());
        assert!(validate_adapter_name("MoltenVK").is_err());
    }
    #[cfg(not(target_os = "macos"))]
    {
        assert_ne!(required_backend(), "metal");
        assert!(validate_backend_name("metal").is_err());
        assert!(validate_adapter_name("MoltenVK").is_err());
    }
}

#[test]
fn rejects_non_arm64_manifest() {
    validate_macos_host_arch("aarch64").expect("ok");
    validate_macos_host_arch("arm64").expect("ok");
    let err = validate_macos_host_arch("x86_64").expect_err("x86");
    let msg = err.to_string();
    assert!(
        msg.contains("x86_64") && msg.to_ascii_lowercase().contains("arch"),
        "{msg}"
    );
}

fn make_renderer() -> SpriteRenderer {
    SpriteRenderer::new(&workspace_root(), true).expect("create host GPU renderer")
}

#[test]
fn tracked_atlas_hashes_are_enforced() {
    let source = workspace_root().join("assets/sprites/generated");
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::copy(
        source.join("manifest.json"),
        temp.path().join("manifest.json"),
    )
    .expect("copy manifest");
    for id in 0..ATLAS_COUNT {
        let file = format!("atlas_{id}.png");
        std::fs::copy(source.join(&file), temp.path().join(&file)).expect("copy atlas");
    }

    let atlas_0 = temp.path().join("atlas_0.png");
    let mut bytes = std::fs::read(&atlas_0).expect("read atlas");
    let last = bytes.last_mut().expect("png bytes");
    *last ^= 1;
    std::fs::write(&atlas_0, bytes).expect("mutate atlas");

    let err = load_atlases(temp.path()).expect_err("hash drift must fail");
    assert!(err.to_string().contains("hash mismatch"), "{err}");
}

#[test]
#[ignore = "requires host GPU + SDL3"]
fn readback_is_1920x1080() {
    let mut r = make_renderer();
    assert_eq!(r.backend(), required_backend());
    let rb = r
        .draw_offscreen_readback(&SpriteRenderer::static_demo_groups())
        .expect("readback");
    assert_eq!(rb.width, VIEW_WIDTH);
    assert_eq!(rb.height, VIEW_HEIGHT);
    assert_eq!(rb.rgba.len(), (VIEW_WIDTH * VIEW_HEIGHT * 4) as usize);
}

#[test]
#[ignore = "requires host GPU + SDL3"]
fn known_static_pixels_match() {
    let mut r = make_renderer();
    let groups = SpriteRenderer::static_demo_groups();
    let expected: Vec<[u8; 4]> = r
        .atlases()
        .iter()
        .map(|atlas| atlas.pixel(18, 18))
        .collect();
    let rb = r.draw_offscreen_readback(&groups).expect("readback");

    // Interior probes of four 30×30 sprites sample source-frame texel (18,18).
    let probes = [(117u32, 117u32), (217, 117), (317, 117), (417, 117)];
    for (atlas_id, (x, y)) in probes.into_iter().enumerate() {
        let got = rb.pixel(x, y);
        let expected = expected[atlas_id];
        // Allow ±1 channel for GPU filter/blend rounding.
        for c in 0..4 {
            let d = (got[c] as i16 - expected[c] as i16).unsigned_abs();
            assert!(
                d <= 1,
                "atlas {atlas_id} @({x},{y}) chan {c}: got {got:?} expected {expected:?}"
            );
        }
    }

    // Background far from sprites stays cleared.
    let bg = rb.pixel(50, 50);
    assert_eq!(bg, [0, 0, 0, 0], "clear color");
}

#[test]
#[ignore = "requires host GPU + SDL3"]
fn four_groups_drawn() {
    let mut r = make_renderer();
    let groups = SpriteRenderer::static_demo_groups();
    assert_eq!(groups.len(), ATLAS_COUNT);
    for (i, g) in groups.iter().enumerate() {
        assert_eq!(g.atlas_id as usize, i);
        assert!(!g.instances.is_empty());
    }
    let rb = r.draw_offscreen_readback(&groups).expect("draw");

    // Each group leaves a non-empty (non-clear) footprint at its sprite center.
    let mut nonempty = 0usize;
    for atlas_id in 0..ATLAS_COUNT {
        let x = 100 + atlas_id as u32 * 100 + 17;
        let y = 117;
        let px = rb.pixel(x, y);
        if px != [0, 0, 0, 0] {
            nonempty += 1;
        }
    }
    assert_eq!(nonempty, ATLAS_COUNT, "expected 4 nonempty atlas groups");

    // Empty group vector rejected.
    let empty: [DrawGroup; 0] = [];
    assert!(r.draw_offscreen_readback(&empty).is_err());
}

#[test]
fn static_instance_covers_sprite_size() {
    for group in SpriteRenderer::static_demo_groups() {
        for inst in group.instances {
            assert_eq!(inst.size, [SPRITE_SIZE_PX as f32, SPRITE_SIZE_PX as f32]);
        }
    }
}

/// `push_text` output actually rasterises: packed into a UI group on
/// [`SLOT_UI_FONT`] and drawn, `"A"` at scale 4.0 (an 8px cell scaled to a
/// 32px quad, landing exactly on the probed 64..96 x 64..96 rect) must leave
/// real opaque pixels behind, not merely a well-formed instance.
#[test]
fn text_renders_visible_pixels() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("text_renders_visible_pixels") else {
        return;
    };
    let mut instances = Vec::new();
    push_text(
        &mut instances,
        "A",
        [64.0, 64.0],
        4.0,
        SpriteInstance::WHITE,
    );
    let ui = [DrawGroup {
        atlas_id: SLOT_UI_FONT,
        instances,
    }];
    let rb = r
        .draw_offscreen_readback_scene(ScenePass {
            world: &[],
            overlay: &[],
            ui: &ui,
            frame_uniforms: None,
        })
        .expect("readback");

    let mut opaque = 0u32;
    for y in 64..96 {
        for x in 64..96 {
            if rb.pixel(x, y)[3] == 255 {
                opaque += 1;
            }
        }
    }
    assert!(
        opaque >= 8,
        "expected at least 8 opaque glyph pixels in 64..96 x 64..96, got {opaque}"
    );
}

/// The UI layer draws after the world and with depth off, so a glyph over an
/// opaque world sprite still lands as pure white — the same claim
/// `render_correctness.rs::ui_layer_draws_over_the_world` makes for a panel,
/// here for text.
#[test]
fn text_is_drawn_over_the_world() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("text_is_drawn_over_the_world") else {
        return;
    };
    let world = [DrawGroup {
        atlas_id: 0,
        instances: vec![SpriteInstance::new(
            [64.0, 64.0],
            [32.0, 32.0],
            frame_uv_rect(0, 0),
            SpriteInstance::WHITE,
        )],
    }];
    let mut instances = Vec::new();
    push_text(
        &mut instances,
        "A",
        [64.0, 64.0],
        4.0,
        SpriteInstance::WHITE,
    );
    let ui = [DrawGroup {
        atlas_id: SLOT_UI_FONT,
        instances,
    }];
    let rb = r
        .draw_offscreen_readback_scene(ScenePass {
            world: &world,
            overlay: &[],
            ui: &ui,
            frame_uniforms: None,
        })
        .expect("readback");

    let mut white = 0u32;
    for y in 64..96 {
        for x in 64..96 {
            if rb.pixel(x, y) == [255, 255, 255, 255] {
                white += 1;
            }
        }
    }
    assert!(
        white >= 1,
        "expected at least one pixel white [255,255,255,255] over the world sprite"
    );
}

/// The tracked RTS scene, packed by `rts::pack_frame`, really rasterises.
///
/// Every headless case in `rts_pack.rs` asserts about instances; none of them
/// can tell a well-formed instance from a visible one. This is the binding
/// back to the device: pack the base scene as the app will and require the
/// frame to leave real pixels behind.
#[test]
fn the_frame_renders() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("the_frame_renders") else {
        return;
    };
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), [960.0, 540.0], None, &mut frame);
    assert_eq!(
        frame.instance_count(),
        17,
        "the base scene must actually be packed, or this probes an empty frame"
    );

    let rb = r
        .draw_offscreen_readback_scene(frame.scene())
        .expect("readback");

    let mut lit = 0u32;
    for y in 0..VIEW_HEIGHT {
        for x in 0..VIEW_WIDTH {
            if rb.pixel(x, y)[3] > 0 {
                lit += 1;
            }
        }
    }
    assert!(
        lit > 1_000,
        "expected more than 1000 non-transparent pixels from the packed RTS \
         scene, got {lit}"
    );
}

/// The placement ghost is UI, not overlay: it draws *over* the world with the
/// prop sheet bound, so a Depot ghost dropped on the HQ tints the HQ's own
/// pixels with the placement-BAD red.
///
/// Compared against the same frame packed without the ghost rather than
/// against a literal colour: the tile is premultiplied and blends over
/// whatever the world put there, so "it is red" is only meaningful next to
/// what it had to beat.
#[test]
fn the_ghost_draws_over_the_world() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("the_ghost_draws_over_the_world") else {
        return;
    };
    let mut h = RtsHarness::scene().build().expect("rts scene harness");
    let cursor = [960.0, 540.0];
    let mut frame = RtsFrame::new();

    // The HQ's screen rect, read off the pack rather than hardcoded.
    pack_frame(h.world(), cursor, None, &mut frame);
    let hq = frame
        .world
        .iter()
        .find(|g| g.atlas_id == SLOT_RTS_BUILDINGS)
        .expect("building group")
        .instances[0];
    let x0 = hq.pos[0].max(0.0) as u32;
    let y0 = hq.pos[1].max(0.0) as u32;
    let x1 = (hq.pos[0] + hq.size[0]).min(VIEW_WIDTH as f32) as u32;
    let y1 = (hq.pos[1] + hq.size[1]).min(VIEW_HEIGHT as f32) as u32;
    assert!(x1 > x0 && y1 > y0, "the HQ must be on screen");
    let under = r
        .draw_offscreen_readback_scene(frame.scene())
        .expect("world-only readback");

    // …and now with a Depot ghost centred on that same HQ, which is an
    // invalid footprint and therefore tiles the placement-BAD cell.
    assert!(h.world_mut().begin_placement(BuildingKind::Depot));
    pack_frame(h.world(), cursor, None, &mut frame);
    let props = &frame
        .ui
        .iter()
        .find(|g| g.atlas_id == SLOT_RTS_PROPS)
        .expect("prop group")
        .instances;
    assert_eq!(props.len(), 65, "64 ghost tiles plus the silhouette");
    assert_eq!(
        props[0].uv_rect,
        frame_uv_rect(0, 2),
        "a footprint over the HQ must be tiled placement-BAD"
    );
    let over = r
        .draw_offscreen_readback_scene(frame.scene())
        .expect("world + ghost readback");

    // Red dominance, not absolute colour: the BAD tile is the only red thing
    // in this frame, so any pixel the ghost pushed toward red inside the HQ's
    // rect can only have come from it.
    let redness = |p: [u8; 4]| i32::from(p[0]) - i32::from(p[1]).max(i32::from(p[2]));
    let mut hits = 0u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let a = under.pixel(x, y);
            let b = over.pixel(x, y);
            if redness(b) > redness(a) + 20 && b[0] > b[1] && b[0] > b[2] {
                hits += 1;
            }
        }
    }
    assert!(
        hits > 0,
        "no pixel inside the HQ's screen rect ({x0}..{x1} x {y0}..{y1}) carries \
         the placement-BAD colour"
    );
}

/// The HUD's top bar really rasterises text, not just well-formed instances.
///
/// Every headless case in `rts_hud.rs` asserts about instances; none of them
/// can tell a well-formed glyph instance from a visible one. Compared against
/// a same-frame reference pixel (the panel's own colour, sampled where no
/// glyph reaches) rather than a literal colour, since the panel art's exact
/// RGB is not this ticket's concern.
#[test]
fn the_hud_renders_legible_pixels() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("the_hud_renders_legible_pixels") else {
        return;
    };
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let mut frame = RtsFrame::new();
    pack_frame(h.world(), [960.0, 540.0], None, &mut frame);
    pack_hud(h.world(), &mut frame);

    let rb = r
        .draw_offscreen_readback_scene(frame.scene())
        .expect("readback");

    // Far right of the top bar: past every icon and every digit the tracked
    // scene's stock ever draws, so this is bare panel.
    let panel_ref = rb.pixel(1900, 20);
    let mut differing = 0u32;
    for y in (TOP_BAR_RECT[1] as u32)..(TOP_BAR_RECT[1] + TOP_BAR_RECT[3]) as u32 {
        for x in (TOP_BAR_RECT[0] as u32)..(TOP_BAR_RECT[0] + TOP_BAR_RECT[2]) as u32 {
            if rb.pixel(x, y) != panel_ref {
                differing += 1;
            }
        }
    }
    assert!(
        differing > 200,
        "expected more than 200 pixels inside the top bar to differ from the \
         panel colour (glyphs visible), got {differing}"
    );
}

/// The bottom panel draws over the world: a world sprite placed under it must
/// have its pixels overwritten by the panel — the same claim
/// `the_ghost_draws_over_the_world` makes for the placement ghost, here for
/// the HUD's command panel.
///
/// Compared before/after at the *same* screen pixel, not across two panel
/// positions: the panel art is a textured, semi-transparent cell stretched
/// over a large quad, so two arbitrary points inside it are neither the same
/// colour nor fully opaque — only the same pixel is guaranteed to change once
/// the world sprite under it is drawn over.
#[test]
fn the_hud_draws_over_the_world() {
    let _g = gpu_guard();
    let Some(mut r) = renderer_or_skip("the_hud_draws_over_the_world") else {
        return;
    };
    let h = RtsHarness::scene().build().expect("rts scene harness");
    let mut frame = RtsFrame::new();
    pack_hud(h.world(), &mut frame); // frame.world stays empty

    // A patch inside the bottom panel with no glyph on it: nothing is
    // selected, so the production block is empty, and this x sits past the
    // build menu's own text.
    let probe = [700u32, 950];
    let world = [DrawGroup {
        atlas_id: 0,
        instances: vec![SpriteInstance::new(
            [probe[0] as f32, probe[1] as f32],
            [32.0, 32.0],
            frame_uv_rect(0, 0),
            SpriteInstance::WHITE,
        )],
    }];

    let under = r
        .draw_offscreen_readback_scene(ScenePass {
            world: &world,
            overlay: &[],
            ui: &[],
            frame_uniforms: None,
        })
        .expect("world-only readback");
    let over = r
        .draw_offscreen_readback_scene(ScenePass {
            world: &world,
            overlay: &[],
            ui: &frame.ui,
            frame_uniforms: None,
        })
        .expect("world + HUD readback");

    assert!(
        (BOTTOM_PANEL_RECT[1] as u32..(BOTTOM_PANEL_RECT[1] + BOTTOM_PANEL_RECT[3]) as u32)
            .contains(&probe[1]),
        "the probe must itself be inside the bottom panel"
    );
    for y in probe[1]..probe[1] + 32 {
        for x in probe[0]..probe[0] + 32 {
            assert_ne!(
                over.pixel(x, y),
                under.pixel(x, y),
                "pixel ({x}, {y}) must change once the HUD panel covers it"
            );
        }
    }
}
