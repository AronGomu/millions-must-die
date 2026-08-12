//! T8: aspect-fit logical canvas — pure viewport math.
//!
//! `DisplayViewport`/`aspect_fit_16_9` never touch SDL or a GPU device, so
//! this is a plain unit-test file, not a GPU case: every test here runs on
//! every host, every time.

use mmd_engine::render::{DisplayViewport, RectU32, VIEW_HEIGHT, VIEW_WIDTH, aspect_fit_16_9};

#[test]
fn exact_canvas_uses_full_drawable() {
    let rect = aspect_fit_16_9([1920, 1080]).expect("1920x1080 fits");
    assert_eq!(
        rect,
        RectU32 {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080
        }
    );
}

#[test]
fn ultrawide_pillarboxes() {
    // k = min(3440/16, 1440/9) = min(215, 160) = 160 -> 2560x1440, centred.
    let rect = aspect_fit_16_9([3440, 1440]).expect("3440x1440 fits");
    assert_eq!(rect.w, 2560);
    assert_eq!(rect.h, 1440);
    assert_eq!(rect.y, 0, "full height used: letterbox bars are on x only");
    assert_eq!(rect.x, (3440 - 2560) / 2);
}

#[test]
fn four_three_letterboxes() {
    // k = min(1280/16, 1024/9) = min(80, 113) = 80 -> 1280x720, centred.
    let rect = aspect_fit_16_9([1280, 1024]).expect("1280x1024 fits");
    assert_eq!(rect.w, 1280);
    assert_eq!(rect.h, 720);
    assert_eq!(rect.x, 0, "full width used: pillarbox bars are on y only");
    assert_eq!(rect.y, (1024 - 720) / 2);
}

#[test]
fn odd_drawable_keeps_exact_ratio() {
    // k = min(1366/16, 768/9) = min(85, 85) = 85 -> 1360x765.
    let rect = aspect_fit_16_9([1366, 768]).expect("1366x768 fits");
    assert_eq!(rect.w, 1360);
    assert_eq!(rect.h, 765);
    assert_eq!(rect.x, (1366 - 1360) / 2);
    assert_eq!(rect.y, (768 - 765) / 2);
}

#[test]
fn too_small_drawable_has_no_fit() {
    assert_eq!(aspect_fit_16_9([15, 9]), None);
    assert_eq!(aspect_fit_16_9([16, 8]), None);
}

// ---------------------------------------------------------------------------
// Pointer inverse: window units -> drawable px -> logical canvas.
// ---------------------------------------------------------------------------

#[test]
fn hidpi_pointer_inverse_round_trips() {
    // 1280x720 window units, 2560x1440 drawable px (2x HiDPI). Exact fit:
    // content is the full 2560x1440 drawable (16:9 already).
    let vp = DisplayViewport::new([1280, 720], [2560, 1440]).expect("fits");
    assert_eq!(
        vp.content_px,
        RectU32 {
            x: 0,
            y: 0,
            w: 2560,
            h: 1440
        }
    );

    let center = vp.map_pointer([640.0, 360.0]);
    assert!(center.inside_content);
    assert!((center.logical[0] - VIEW_WIDTH as f32 / 2.0).abs() < 1e-3);
    assert!((center.logical[1] - VIEW_HEIGHT as f32 / 2.0).abs() < 1e-3);

    let top_left = vp.map_pointer([0.0, 0.0]);
    assert!(top_left.inside_content);
    assert_eq!(top_left.logical, [0.0, 0.0]);

    // Bottom-right window-unit corner maps to drawable (2560, 1440), which is
    // just past the half-open content bounds (content spans [0,2560)x[0,1440)).
    let bottom_right = vp.map_pointer([1280.0, 720.0]);
    assert!(!bottom_right.inside_content);
    assert!((bottom_right.logical[0] - VIEW_WIDTH as f32).abs() < 1e-3);
    assert!((bottom_right.logical[1] - VIEW_HEIGHT as f32).abs() < 1e-3);
}

/// Hard constraint: the input inverse must exactly invert the render
/// transform. `present_blit` scales the fixed `VIEW_WIDTH x VIEW_HEIGHT`
/// offscreen linearly into `content_px`; this proves the round trip
/// `logical -> drawable -> logical` is exact (to float rounding) across a
/// pillarbox case, a letterbox case, and points inside the bars on both.
#[test]
fn round_trip_inverts_render_transform_pillarbox_and_letterbox() {
    let cases: [([u32; 2], [u32; 2]); 2] = [
        // Pillarboxed: content 2560x1440 centred in a 3440x1440 drawable.
        ([3440, 1440], [3440, 1440]),
        // Letterboxed: content 1280x720 centred in a 1280x1024 drawable.
        ([1280, 1024], [1280, 1024]),
    ];

    for (drawable, window_units) in cases {
        let vp = DisplayViewport::new(window_units, drawable).expect("fits");
        let content = vp.content_px;

        // Forward: a logical canvas point -> its drawable pixel under the
        // same linear map `present_blit` uses (content origin + logical
        // fraction * content extent).
        let forward = |logical: [f32; 2]| -> [f32; 2] {
            [
                content.x as f32 + logical[0] / VIEW_WIDTH as f32 * content.w as f32,
                content.y as f32 + logical[1] / VIEW_HEIGHT as f32 * content.h as f32,
            ]
        };

        for logical in [
            [0.0, 0.0],
            [VIEW_WIDTH as f32, 0.0],
            [0.0, VIEW_HEIGHT as f32],
            [VIEW_WIDTH as f32, VIEW_HEIGHT as f32],
            [VIEW_WIDTH as f32 / 2.0, VIEW_HEIGHT as f32 / 2.0],
            [VIEW_WIDTH as f32 * 0.25, VIEW_HEIGHT as f32 * 0.75],
        ] {
            let drawable_pt = forward(logical);
            // window_units == drawable_px in these cases, ratio is 1.0.
            let mapped = vp.map_pointer(drawable_pt);
            assert!(
                (mapped.logical[0] - logical[0]).abs() < 1e-2
                    && (mapped.logical[1] - logical[1]).abs() < 1e-2,
                "round trip drifted: {logical:?} -> {drawable_pt:?} -> {:?}",
                mapped.logical
            );
        }

        // Points inside the bars: clamp to the nearest content edge, in
        // logical space, rather than reporting an out-of-canvas coordinate.
        if content.x > 0 {
            // Pillarbox: a point in the left bar.
            let bar_pt = [content.x as f32 / 2.0, content.y as f32 + 5.0];
            let mapped = vp.map_pointer(bar_pt);
            assert!(!mapped.inside_content);
            assert!((mapped.logical[0] - 0.0).abs() < 1e-2);
            assert!(mapped.logical[1] >= 0.0 && mapped.logical[1] <= VIEW_HEIGHT as f32);
        }
        if content.y > 0 {
            // Letterbox: a point in the top bar.
            let bar_pt = [content.x as f32 + 5.0, content.y as f32 / 2.0];
            let mapped = vp.map_pointer(bar_pt);
            assert!(!mapped.inside_content);
            assert!((mapped.logical[1] - 0.0).abs() < 1e-2);
            assert!(mapped.logical[0] >= 0.0 && mapped.logical[0] <= VIEW_WIDTH as f32);
        }
    }
}

#[test]
fn bar_click_is_outside() {
    let vp = DisplayViewport::new([1280, 1024], [1280, 1024]).expect("fits");
    // content is 1280x720 letterboxed vertically; y=10 is in the top bar.
    let mapped = vp.map_pointer([640.0, 10.0]);
    assert!(!mapped.inside_content);
}

#[test]
fn bar_motion_clamps_to_edge() {
    let vp = DisplayViewport::new([3440, 1440], [3440, 1440]).expect("fits");
    // content is 2560x1440 pillarboxed; x=5 is deep in the left bar.
    let left_bar = vp.map_pointer([5.0, 720.0]);
    assert_eq!(left_bar.logical[0], 0.0);

    // x near the right edge of the drawable is deep in the right bar.
    let right_bar = vp.map_pointer([3435.0, 720.0]);
    assert_eq!(right_bar.logical[0], VIEW_WIDTH as f32);
}
