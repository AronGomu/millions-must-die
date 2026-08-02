//! GPU smoke tests. Unit layout/backend tests always run;
//! device tests are `#[ignore]` and need a real GPU + SDL3.
//!
//! GPU cases share process-global SDL; run serially (`--test-threads=1`).

use mmd_engine::render::{
    ATLAS_COUNT, DrawGroup, REQUIRED_BACKEND, SPRITE_SIZE_PX, SpriteInstance, SpriteRenderer,
    VIEW_HEIGHT, VIEW_WIDTH, expected_sprite_center_pixel, required_backend, validate_adapter_name,
    validate_backend_name,
};
#[cfg(target_os = "windows")]
use mmd_engine::render::validate_device_props;
use mmd_engine::workspace_root;

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

fn make_renderer() -> SpriteRenderer {
    SpriteRenderer::new(&workspace_root(), true).expect("create host GPU renderer")
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
    let rb = r.draw_offscreen_readback(&groups).expect("readback");

    // Centers of the four static sprites.
    let probes = [(101u32, 100 + 1), (201, 101), (301, 101), (401, 101)];
    for (atlas_id, (x, y)) in probes.into_iter().enumerate() {
        let got = rb.pixel(x, y);
        let expected = expected_sprite_center_pixel(atlas_id as u32);
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
        let x = 100 + atlas_id as u32 * 100 + 1;
        let y = 101;
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
#[ignore = "requires host GPU + SDL3"]
fn static_instance_covers_sprite_size() {
    let g = &SpriteRenderer::static_demo_groups()[0];
    let inst: &SpriteInstance = &g.instances[0];
    assert_eq!(inst.size, [SPRITE_SIZE_PX as f32, SPRITE_SIZE_PX as f32]);
}
