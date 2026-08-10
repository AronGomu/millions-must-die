//! Compact sprite instance record shared with `shaders/sprite.hlsl`.

use std::mem::size_of;

use crate::scenario::Cell;

/// GPU instance stride for one sprite (slot 1, per-instance).
///
/// Layout must match VS TEXCOORD2..5 packing:
/// `pos.xy`, `size.xy`, `uv_rect.xyzw`, `tint.rgba`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteInstance {
    /// Top-left pixel position.
    pub pos: [f32; 2],
    /// Display-quad size in pixels (independent from atlas frame resolution).
    pub size: [f32; 2],
    /// Atlas UV rectangle `(u0, v0, u1, v1)`.
    pub uv_rect: [f32; 4],
    /// Premultiplied RGBA tint.
    pub tint: [f32; 4],
}

/// Value written to `uv_rect.x` to select the shader's ring branch.
///
/// A *negative* `u0` is the sentinel. It is safe because every rect
/// [`super::frame_uv_rect`] can emit is a ratio of non-negative integers, so a
/// sprite's `u0` is never below zero — pinned by
/// `ring_instances_keep_the_pinned_layout` across the whole animation grid.
///
/// The shader side of this contract is `MMD_RING_THRESHOLD` in
/// `shaders/sprite.hlsl`, which is `0.0` — the *threshold* this value must sit
/// below, not a copy of it. The invariant is
/// `RING_SENTINEL < MMD_RING_THRESHOLD <= 0.0`.
///
/// Overloading an existing field rather than adding one is what keeps
/// [`SpriteInstance`] at the 48 bytes `instance_layout_is_stable` locks, and
/// keeps the ring on the same pipeline and the same vertex format as a sprite.
pub const RING_SENTINEL: f32 = -1.0;

impl SpriteInstance {
    /// Byte size of one instance record.
    pub const STRIDE: u32 = size_of::<Self>() as u32;

    /// White opaque premul tint.
    pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    /// Build instance for a single atlas frame.
    pub fn new(pos: [f32; 2], size: [f32; 2], uv_rect: [f32; 4], tint: [f32; 4]) -> Self {
        Self {
            pos,
            size,
            uv_rect,
            tint,
        }
    }

    /// Build a procedural hitbox-ring instance — no atlas, no texture.
    ///
    /// `inner`/`outer` are radii in normalised quad units, where `0.5` is the
    /// quad edge. The fragment stage keeps the annulus between them and
    /// discards the rest, so `outer` is where the ring's outer edge lands.
    pub fn ring(pos: [f32; 2], size: [f32; 2], inner: f32, outer: f32, tint: [f32; 4]) -> Self {
        Self {
            pos,
            size,
            uv_rect: [RING_SENTINEL, inner, outer, 0.0],
            tint,
        }
    }

    /// Whether the shader will take the ring branch for this instance.
    ///
    /// One predicate shared by the renderer, the tests and the shader comment,
    /// so "what counts as a ring" cannot be stated two ways.
    pub fn is_ring(&self) -> bool {
        self.uv_rect[0] < 0.0
    }
}

/// Unit-quad vertex (slot 0, per-vertex). Matches VS TEXCOORD0..1.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadVertex {
    /// Corner in `[-0.5, 0.5]`.
    pub corner: [f32; 2],
    /// Unit UV in `[0, 1]`.
    pub uv: [f32; 2],
}

impl QuadVertex {
    pub const STRIDE: u32 = size_of::<Self>() as u32;
}

/// Frame view uniform (space1 b0).
///
/// The two depth scalars occupy what used to be `_pad`: the vertex stage
/// already computes the screen-space ground point it needs, so nothing about
/// them costs a byte of [`SpriteInstance`], and the struct stays 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameUniforms {
    pub view_size: [f32; 2],
    /// `1 / iso_map_height_px` — see [`IsoView::depth_scale`].
    pub depth_scale: f32,
    /// `-origin.y / iso_map_height_px` — see [`IsoView::depth_bias`].
    pub depth_bias: f32,
}

/// Isometric tile width, as a multiple of the scenario's `cell_size_px`.
///
/// With [`ISO_TILE_H_PER_CELL`] this is the 2:1 tile the whole projection is
/// named for: 8 × 4 px at the tracked scenes' `cell_size_px: 4`.
pub const ISO_TILE_W_PER_CELL: f32 = 2.0;

/// Isometric tile height, as a multiple of the scenario's `cell_size_px`.
pub const ISO_TILE_H_PER_CELL: f32 = 1.0;

/// Project a point in **cell space** onto the 2:1 isometric screen.
///
/// `sx = origin.x + (cx - cy) * tile_w / 2`,
/// `sy = origin.y + (cx + cy) * tile_h / 2`.
///
/// The simulation never sees this function. It stays in Cartesian cell space —
/// an isometric game is a Cartesian game with a different camera — and that is
/// exactly what keeps every pinned state digest alive across this change.
pub fn iso_project(cx: f32, cy: f32, tile_w: f32, tile_h: f32, origin: [f32; 2]) -> [f32; 2] {
    [
        origin[0] + (cx - cy) * tile_w * 0.5,
        origin[1] + (cx + cy) * tile_h * 0.5,
    ]
}

/// Exact inverse of [`iso_project`]: a screen pixel back to cell space.
///
/// From `sx = ox + (cx - cy) * tw/2` and `sy = oy + (cx + cy) * th/2`:
///   `u = (sx - ox) / (tw/2) = cx - cy`
///   `v = (sy - oy) / (th/2) = cx + cy`
///   `cx = (u + v) / 2`,  `cy = (v - u) / 2`
///
/// A zero `tile_w` or `tile_h` has no inverse; it yields `[f32::NAN, f32::NAN]`
/// rather than an infinity, so a caller's bounds check rejects it instead of
/// indexing a cell at the far edge of the grid.
pub fn iso_unproject(sx: f32, sy: f32, tile_w: f32, tile_h: f32, origin: [f32; 2]) -> [f32; 2] {
    if tile_w == 0.0 || tile_h == 0.0 {
        return [f32::NAN, f32::NAN];
    }
    let u = (sx - origin[0]) / (tile_w * 0.5);
    let v = (sy - origin[1]) / (tile_h * 0.5);
    [(u + v) * 0.5, (v - u) * 0.5]
}

/// The fixed camera offset that puts `dest`'s **centre** at the view centre.
///
/// The camera does not move: this is evaluated once per scene. Scrolling,
/// edge-pan, zoom and selection are Phase 1.
///
/// `width`/`height` bound the grid the destination is read against. Scenario
/// validation already rejects an out-of-bounds destination, so the clamp never
/// fires on a tracked scene; it is here so that a grid built in memory cannot
/// aim the camera at a cell that does not exist and leave the whole map off
/// screen.
pub fn iso_origin(
    width: u32,
    height: u32,
    dest: Cell,
    tile_w: f32,
    tile_h: f32,
    view: [f32; 2],
) -> [f32; 2] {
    let cx = dest.x.min(width.saturating_sub(1)) as f32 + 0.5;
    let cy = dest.y.min(height.saturating_sub(1)) as f32 + 0.5;
    let centred = iso_project(cx, cy, tile_w, tile_h, [0.0, 0.0]);
    [view[0] * 0.5 - centred[0], view[1] * 0.5 - centred[1]]
}

/// Smallest depth a sprite may be given — one quantum of the `D16_UNORM` depth
/// attachment.
///
/// Mirrors `MMD_DEPTH_EPSILON` in `shaders/sprite.hlsl`. The attachment is
/// cleared to `0` and tested with `GREATER`, so a key of exactly `0` fails
/// `0 > 0` and is discarded outright — drawn *nowhere*, not drawn last. An
/// agent on the far corner of the map diamond normalises to `0`, and a
/// degenerate [`IsoView`] zeroes both scalars and would blank the whole frame.
/// Flooring costs one part in 65 536 of sort precision and removes the class.
///
/// Rings are exempt by construction: they bypass [`iso_depth`] and emit an
/// exact `0`, which is what makes "the ring pass is not depth-tested"
/// falsifiable.
pub const ISO_DEPTH_EPSILON: f32 = 1.0 / 65_536.0;

/// Normalised sort key for a ground point at screen `ground_y`.
///
/// CPU mirror of `max(saturate(ground_y * depth_scale + depth_bias),
/// MMD_DEPTH_EPSILON)` in `shaders/sprite.hlsl`. `f32::max` returns the non-NaN
/// operand, so a NaN input collapses to [`ISO_DEPTH_EPSILON`] the way HLSL's
/// `saturate` collapses it to `0` — and, unlike `0`, that still draws.
// Deliberately not `clamp`: `f32::clamp` *propagates* NaN, which is precisely
// the behaviour this function must not have if it is to mirror `saturate`.
#[allow(clippy::manual_clamp)]
pub fn iso_depth(ground_y: f32, scale: f32, bias: f32) -> f32 {
    (ground_y * scale + bias).max(ISO_DEPTH_EPSILON).min(1.0)
}

/// Whether a quad's AABB overlaps the view rect at all.
///
/// Pure in the two rects — no camera, no scenario, no instance — so the packers
/// and the tests can only ever be asking the same question. Touching an edge
/// exactly (`pos.x + size.x == 0`) covers no pixel and is rejected.
pub fn quad_is_visible(pos: [f32; 2], size: [f32; 2], view: [f32; 2]) -> bool {
    pos[0] + size[0] > 0.0 && pos[1] + size[1] > 0.0 && pos[0] < view[0] && pos[1] < view[1]
}

/// Everything the world→screen projection needs, derived once per scene.
///
/// One value computes the tile, the fixed origin and the two depth scalars, and
/// both packers plus the uniform upload read it — so the CPU mirror and the GPU
/// cannot disagree about where an agent is or how deep it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IsoView {
    /// Isometric tile width in pixels (`2 * cell_size_px`).
    pub tile_w: f32,
    /// Isometric tile height in pixels (`cell_size_px`).
    pub tile_h: f32,
    /// Fixed camera offset — [`iso_origin`].
    pub origin: [f32; 2],
    /// Height of the whole map diamond in pixels — the span the depth key is
    /// normalised over. Stored rather than recovered from `depth_scale`: the
    /// reciprocal of an already-rounded `f32` is not the value it came from.
    pub map_height_px: f32,
    /// `1 / iso_map_height_px`.
    pub depth_scale: f32,
    /// `-origin.y / iso_map_height_px`, so `ground_y * scale + bias` is the
    /// agent's position down the *map* diamond in `[0, 1]` and does not move
    /// when the camera does.
    pub depth_bias: f32,
    /// Render-target size in pixels; the cull's view rect.
    pub view_size: [f32; 2],
}

impl IsoView {
    /// Derive the projection for a grid of `width` × `height` cells whose
    /// destination is `dest`, rendered into a `view_size` target.
    ///
    /// A degenerate grid (zero cells, or a zero cell size) has no map diamond
    /// to normalise against; its depth scalars are zeroed so every quad lands
    /// at depth `0` instead of at a non-finite one.
    pub fn new(
        width: u32,
        height: u32,
        dest: Cell,
        cell_size_px: f32,
        view_size: [f32; 2],
    ) -> Self {
        let tile_w = cell_size_px * ISO_TILE_W_PER_CELL;
        let tile_h = cell_size_px * ISO_TILE_H_PER_CELL;
        let origin = iso_origin(width, height, dest, tile_w, tile_h, view_size);
        let map_h = (width as f32 + height as f32) * tile_h * 0.5;
        let (depth_scale, depth_bias) = if map_h > 0.0 && map_h.is_finite() {
            (1.0 / map_h, -origin[1] / map_h)
        } else {
            (0.0, 0.0)
        };
        Self {
            tile_w,
            tile_h,
            origin,
            map_height_px: if map_h.is_finite() { map_h } else { 0.0 },
            depth_scale,
            depth_bias,
            view_size,
        }
    }

    /// [`iso_project`] through this view's tile and origin.
    pub fn project(&self, cx: f32, cy: f32) -> [f32; 2] {
        iso_project(cx, cy, self.tile_w, self.tile_h, self.origin)
    }

    /// [`iso_depth`] through this view's scalars.
    pub fn depth(&self, ground_y: f32) -> f32 {
        iso_depth(ground_y, self.depth_scale, self.depth_bias)
    }

    /// The uniform block the vertex stage reads for this view.
    pub fn frame_uniforms(&self) -> FrameUniforms {
        FrameUniforms {
            view_size: self.view_size,
            depth_scale: self.depth_scale,
            depth_bias: self.depth_bias,
        }
    }

    /// [`iso_unproject`] through this view's tile and origin.
    pub fn unproject(&self, sx: f32, sy: f32) -> [f32; 2] {
        iso_unproject(sx, sy, self.tile_w, self.tile_h, self.origin)
    }

    /// The cell containing a screen pixel, or `None` when it falls outside the
    /// `width` x `height` grid this view was built for.
    ///
    /// `width`/`height` are parameters rather than stored state: `IsoView` is a
    /// projection, not a map, and giving it a second copy of the grid size is
    /// how the two would drift.
    pub fn cell_at(&self, sx: f32, sy: f32, width: u32, height: u32) -> Option<Cell> {
        let c = self.unproject(sx, sy);
        if !c[0].is_finite() || !c[1].is_finite() || c[0] < 0.0 || c[1] < 0.0 {
            return None;
        }
        let (x, y) = (c[0] as u32, c[1] as u32);
        if x >= width || y >= height {
            None
        } else {
            Some(Cell { x, y })
        }
    }

    /// This view re-derived around a new cell-space centre.
    ///
    /// `map_height_px`, `tile_w`, `tile_h` and `view_size` are unchanged;
    /// `origin` and `depth_bias` move together so the depth key stays the
    /// agent's position down the map diamond and does not shift under the
    /// camera.
    pub fn with_center_cell(&self, center: [f32; 2]) -> Self {
        let centred = iso_project(center[0], center[1], self.tile_w, self.tile_h, [0.0, 0.0]);
        let origin = [
            self.view_size[0] * 0.5 - centred[0],
            self.view_size[1] * 0.5 - centred[1],
        ];
        let depth_bias = if self.map_height_px > 0.0 {
            -origin[1] / self.map_height_px
        } else {
            0.0
        };
        Self {
            origin,
            depth_bias,
            ..*self
        }
    }
}

/// Canonical unit quad (two triangles via index buffer).
pub const QUAD_VERTICES: [QuadVertex; 4] = [
    QuadVertex {
        corner: [-0.5, -0.5],
        uv: [0.0, 0.0],
    },
    QuadVertex {
        corner: [0.5, -0.5],
        uv: [1.0, 0.0],
    },
    QuadVertex {
        corner: [0.5, 0.5],
        uv: [1.0, 1.0],
    },
    QuadVertex {
        corner: [-0.5, 0.5],
        uv: [0.0, 1.0],
    },
];

/// Triangle list indices for [`QUAD_VERTICES`].
pub const QUAD_INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

/// CPU mirror of the vertex stage's world→clip transform in
/// `shaders/sprite.hlsl` (`VSMain`).
///
/// Pixel space is y-down with the origin at the view's top-left; clip space is
/// y-up over `[-1, 1]`. Given a unit-quad `corner` in `[-0.5, 0.5]` and one
/// instance's `pos`/`size`, this returns the `SV_Position` the shader emits.
///
/// This mirror exists so a headless test can state the expected clip
/// coordinate for a known world corner. It is *not* self-validating: the
/// binding back to the real shader is the GPU raster probe in
/// `tests/render_correctness.rs`, which renders one sprite and requires its
/// footprint to land exactly where this function predicts.
///
/// A zero component in `view_size` has no meaningful projection and yields a
/// non-finite coordinate; callers pass the live [`FrameUniforms::view_size`],
/// which the renderer fixes at the offscreen resolution.
///
/// `depth` is `SV_Position.z`. It is a parameter rather than something this
/// function derives, because the shader derives it from the quad's **bottom
/// edge** — one value for all four corners — and a mirror that recomputed it
/// per corner would state a depth gradient the GPU never emits. Callers pass
/// [`IsoView::depth`] of the instance's ground point (`0.0` for a ring, which
/// draws with the depth test off).
pub fn world_to_clip(
    pos: [f32; 2],
    size: [f32; 2],
    corner: [f32; 2],
    view_size: [f32; 2],
    depth: f32,
) -> [f32; 4] {
    let world = [
        pos[0] + (corner[0] + 0.5) * size[0],
        pos[1] + (corner[1] + 0.5) * size[1],
    ];
    let ndc_x = (world[0] / view_size[0]) * 2.0 - 1.0;
    let ndc_y = (world[1] / view_size[1]) * 2.0 - 1.0;
    [ndc_x, -ndc_y, depth, 1.0]
}

/// Fixed-function viewport map the GPU applies after the vertex stage:
/// clip `xy` → pixel `xy` in the y-down render target.
///
/// Paired with [`world_to_clip`] this round-trips to the original world pixel,
/// which is exactly the claim the GPU raster probe checks.
pub fn clip_to_pixel(clip: [f32; 4], view_size: [f32; 2]) -> [f32; 2] {
    [
        (clip[0] * 0.5 + 0.5) * view_size[0],
        (0.5 - clip[1] * 0.5) * view_size[1],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of};

    /// Every scene the repo tracks, relative to the workspace root.
    ///
    /// Listed rather than globbed so the expectation is readable; a scene added
    /// or removed under `assets/scenarios/` trips
    /// [`the_tracked_scene_list_is_complete`] rather than silently narrowing
    /// [`the_destination_is_centred`].
    const TRACKED_SCENES: [&str; 4] = [
        "assets/scenarios/technical_prototype_v1.ron",
        "assets/scenarios/collision_mid_v1.ron",
        "assets/scenarios/collision_sprite_v1.ron",
        "assets/scenarios/rts_prototype_v1.ron",
    ];

    #[test]
    fn the_tracked_scene_list_is_complete() {
        let root = crate::workspace_root();
        let dir = root.join("assets/scenarios");
        // Compared as full workspace-relative paths, not basenames: a wrong
        // directory prefix in the constant would otherwise pass here and only
        // surface later, as a panic inside `the_destination_is_centred`.
        let mut found: Vec<String> = std::fs::read_dir(&dir)
            .expect("scenario dir")
            .filter_map(|e| {
                let p = e.expect("dir entry").path();
                (p.extension().is_some_and(|x| x == "ron")).then(|| {
                    p.strip_prefix(&root)
                        .expect("scenarios live under the workspace root")
                        .to_string_lossy()
                        .into_owned()
                })
            })
            .collect();
        found.sort();
        let mut want: Vec<String> = TRACKED_SCENES.iter().map(|p| (*p).to_owned()).collect();
        want.sort();
        assert_eq!(
            found,
            want,
            "TRACKED_SCENES no longer names every scene under {}",
            dir.display()
        );
    }

    /// The uniform block's *field offsets*, not only its size.
    ///
    /// Deliberately separate from [`instance_layout_is_stable`], which an
    /// earlier ticket pinned and which must keep passing unmodified. Size alone
    /// is not the contract: swapping `depth_scale` and `depth_bias` keeps the
    /// struct at 16 bytes and silently turns every depth key into
    /// `ground_y * bias + scale` — a frame that renders and is merely wrong,
    /// whose only other oracle is a GPU test behind an opt-in env var.
    ///
    /// These offsets are the Rust half of the contract with `cbuffer
    /// FrameUniforms` in `shaders/sprite.hlsl` and the `std140` block in
    /// `shaders/glsl/sprite.vert.glsl`, both of which lay out
    /// `view_size@0, depth_scale@8, depth_bias@12`.
    #[test]
    fn frame_uniforms_layout_is_stable() {
        assert_eq!(size_of::<FrameUniforms>(), 16);
        assert_eq!(align_of::<FrameUniforms>(), 4);
        assert_eq!(offset_of!(FrameUniforms, view_size), 0);
        assert_eq!(offset_of!(FrameUniforms, depth_scale), 8);
        assert_eq!(offset_of!(FrameUniforms, depth_bias), 12);
    }

    /// The ring rides the *same* 48-byte record as a sprite — no new field, no
    /// second instance format — and its `inner`/`outer` survive the trip
    /// through the fields the shader reads them from.
    ///
    /// The second half is the load-bearing one: the branch is selected by a
    /// negative `uv_rect.x`, so the sentinel is only safe if no atlas rect the
    /// packer can emit is ever negative. That is asserted over the whole
    /// animation grid rather than argued.
    #[test]
    fn ring_instances_keep_the_pinned_layout() {
        // Deliberately *not* `RING_INNER`/`RING_OUTER`/`RING_TINT`: this test
        // owns the record's plumbing, not the overlay's tuning. Arbitrary
        // values keep it from reading as a second definition of the band that
        // would go stale the moment the real one is retuned.
        let ring = SpriteInstance::ring([10.0, 20.0], [48.0, 48.0], 0.3, 0.4, [0.1, 0.2, 0.3, 0.4]);

        assert_eq!(size_of::<SpriteInstance>(), 48);
        assert_eq!(SpriteInstance::STRIDE, 48);

        assert_eq!(ring.pos, [10.0, 20.0]);
        assert_eq!(ring.size, [48.0, 48.0]);
        assert_eq!(ring.tint, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(ring.uv_rect[0], RING_SENTINEL);
        assert_eq!(ring.uv_rect[1], 0.3, "inner radius round-trip");
        assert_eq!(ring.uv_rect[2], 0.4, "outer radius round-trip");
        assert_eq!(ring.uv_rect[3], 0.0);
        assert!(ring.is_ring());

        // No sprite may trip the ring branch. `frame_uv_rect` divides
        // non-negative integers, so every rect it produces has `u0 >= 0` —
        // asserted across the whole animation grid, sized from the atlas
        // constants so growing the grid cannot leave new frames uncovered
        // while this still reads as exhaustive.
        for dir in 0..super::super::atlas::FRAMES_Y {
            for frame in 0..super::super::atlas::FRAMES_X {
                let uv = crate::render::frame_uv_rect(dir, frame);
                let sprite = SpriteInstance::new([0.0, 0.0], [1.0, 1.0], uv, SpriteInstance::WHITE);
                assert!(
                    !sprite.is_ring(),
                    "atlas frame ({dir}, {frame}) has uv_rect {uv:?}, which the shader \
                     would read as a ring"
                );
            }
        }
    }

    /// The four grid corners land on the four vertices of the screen diamond,
    /// in the order the isometric read demands: `(0,0)` is the top, `(w,0)` the
    /// right, `(0,h)` the left and `(w,h)` the bottom.
    ///
    /// Stated as an ordering *and* as absolute coordinates: a projection that
    /// swapped the two axes would still produce a diamond, and only the named
    /// vertices catch it.
    #[test]
    fn iso_projects_a_diamond() {
        let (w, h) = (480.0f32, 270.0f32);
        let (tw, th) = (8.0f32, 4.0f32);
        let origin = [1000.0f32, 20.0f32];

        let top = iso_project(0.0, 0.0, tw, th, origin);
        let right = iso_project(w, 0.0, tw, th, origin);
        let left = iso_project(0.0, h, tw, th, origin);
        let bottom = iso_project(w, h, tw, th, origin);

        assert_eq!(top, [1000.0, 20.0], "cell (0,0) is the diamond's top");
        assert_eq!(
            right,
            [1000.0 + w * tw * 0.5, 20.0 + w * th * 0.5],
            "cell (w,0) is the diamond's right vertex"
        );
        assert_eq!(
            left,
            [1000.0 - h * tw * 0.5, 20.0 + h * th * 0.5],
            "cell (0,h) is the diamond's left vertex"
        );
        assert_eq!(
            bottom,
            [1000.0 + (w - h) * tw * 0.5, 20.0 + (w + h) * th * 0.5],
            "cell (w,h) is the diamond's bottom vertex"
        );

        // Ordering, not just coordinates: top is highest, bottom lowest, left
        // leftmost, right rightmost.
        assert!(top[1] < left[1] && top[1] < right[1]);
        assert!(bottom[1] > left[1] && bottom[1] > right[1]);
        assert!(left[0] < top[0] && left[0] < bottom[0]);
        assert!(right[0] > top[0] && right[0] > bottom[0]);
    }

    /// One cell of `+x` moves half a tile right and half a tile down; one cell
    /// of `+y` moves the same distance *left* and down. That mirrored pair is
    /// what makes the projection 2:1 isometric rather than a shear.
    #[test]
    fn iso_is_two_to_one() {
        let (tw, th) = (8.0f32, 4.0f32);
        let origin = [100.0f32, 50.0f32];
        let base = iso_project(10.0, 6.0, tw, th, origin);

        let step_x = iso_project(11.0, 6.0, tw, th, origin);
        assert_eq!(
            [step_x[0] - base[0], step_x[1] - base[1]],
            [tw * 0.5, th * 0.5],
            "+1 cell in x moves half a tile right and half a tile down"
        );

        let step_y = iso_project(10.0, 7.0, tw, th, origin);
        assert_eq!(
            [step_y[0] - base[0], step_y[1] - base[1]],
            [-tw * 0.5, th * 0.5],
            "+1 cell in y moves half a tile left and half a tile down"
        );

        // The two steps are mirror images — the defining property of the
        // projection, and not implied by either assertion above on its own.
        assert_eq!(step_x[0] - base[0], -(step_y[0] - base[0]));
        assert_eq!(step_x[1] - base[1], step_y[1] - base[1]);

        // …and the tile the whole projection is named for really is 2:1. Read
        // off the production constants, not off this test's own literals, which
        // would be a tautology.
        assert_eq!(ISO_TILE_W_PER_CELL / ISO_TILE_H_PER_CELL, 2.0);
        let iso = IsoView::new(64, 64, Cell { x: 1, y: 1 }, 4.0, [1920.0, 1080.0]);
        assert_eq!(iso.tile_w / iso.tile_h, 2.0);
    }

    /// The fixed camera puts the destination cell dead centre of the view, for
    /// every scene the repo tracks.
    #[test]
    fn the_destination_is_centred() {
        let view = [1920.0f32, 1080.0f32];
        for rel in TRACKED_SCENES {
            let path = crate::workspace_root().join(rel);
            let scenario = crate::scenario::Scenario::load_verified(&path)
                .unwrap_or_else(|e| panic!("{rel}: {e}"));
            let iso = IsoView::new(
                scenario.width(),
                scenario.height(),
                scenario.destination(),
                scenario.cell_size_px() as f32,
                view,
            );
            let dest = scenario.destination();
            let centre = iso.project(dest.x as f32 + 0.5, dest.y as f32 + 0.5);
            assert!(
                (centre[0] - view[0] * 0.5).abs() < 1e-3
                    && (centre[1] - view[1] * 0.5).abs() < 1e-3,
                "{rel}: destination {dest:?} projects to {centre:?}, not the view centre \
                 {:?}",
                [view[0] * 0.5, view[1] * 0.5]
            );
            // The tile really is 2:1 over the scene's own cell size.
            assert_eq!(iso.tile_w, scenario.cell_size_px() as f32 * 2.0);
            assert_eq!(iso.tile_h, scenario.cell_size_px() as f32);
        }
    }

    /// The depth key says where an agent stands on the *map*, not where the
    /// camera happens to be: moving the origin must not reorder anything, and
    /// must not change a single key.
    #[test]
    fn depth_is_camera_independent() {
        let dest = Cell { x: 240, y: 135 };
        let here = IsoView::new(480, 270, dest, 4.0, [1920.0, 1080.0]);
        // A second view of the same map with the camera parked somewhere else.
        let mut elsewhere = here;
        elsewhere.origin = [here.origin[0] - 613.0, here.origin[1] + 271.0];
        elsewhere.depth_bias = -elsewhere.origin[1] / here.map_height_px;
        assert_ne!(here.origin, elsewhere.origin, "the cameras must differ");

        for (cx, cy) in [
            (0.5f32, 0.5f32),
            (240.5, 135.5),
            (479.5, 269.5),
            (12.25, 199.75),
        ] {
            let a = here.depth(here.project(cx, cy)[1]);
            let b = elsewhere.depth(elsewhere.project(cx, cy)[1]);
            assert!(
                (a - b).abs() < 1e-6,
                "cell ({cx},{cy}) has depth {a} under one camera and {b} under another"
            );
            // …and it is the agent's position down the map diamond.
            let want = (cx + cy) / (480.0 + 270.0);
            assert!(
                (a - want).abs() < 1e-6,
                "cell ({cx},{cy}) has depth {a}, expected {want}"
            );
        }

        // Nearer (larger `cx + cy`) gets the *larger* key, which is why the
        // pipeline tests GREATER against an attachment cleared to 0. Under LESS
        // the nearer agent would lose and the horde would draw back to front.
        let far = here.depth(here.project(10.0, 10.0)[1]);
        let near = here.depth(here.project(10.0, 11.0)[1]);
        assert!(
            near > far,
            "an agent one cell nearer must have a larger key"
        );

        // No sprite key is ever exactly 0. Zero is the far plane, and under the
        // strict GREATER test against a 0-clear a fragment there is *discarded*,
        // not sorted last — so the floor is what keeps the map's far corner, and
        // a degenerate view, visible at all.
        let degenerate = IsoView::new(0, 0, dest, 0.0, [1920.0, 1080.0]);
        for (view, cx, cy) in [
            (here, 0.0f32, 0.0f32),
            (here, -50.0, -50.0),
            (degenerate, 12.0, 34.0),
        ] {
            let d = view.depth(view.project(cx, cy)[1]);
            assert!(
                d >= ISO_DEPTH_EPSILON,
                "cell ({cx},{cy}) produced depth {d}, which the depth test would \
                 discard outright rather than draw behind everything"
            );
        }
    }

    /// `iso_origin` is exercised directly, including the branch that uses its
    /// `width`/`height`.
    ///
    /// `the_destination_is_centred` only ever reaches it through
    /// [`IsoView::new`] on in-bounds tracked scenes, which leaves the clamp
    /// untested — and untested behaviour added to satisfy a signature is worse
    /// than no behaviour.
    #[test]
    fn iso_origin_frames_the_destination() {
        let view = [1920.0f32, 1080.0f32];
        let (tw, th) = (8.0f32, 4.0f32);

        // In bounds: the destination's centre lands on the view centre.
        let o = iso_origin(480, 270, Cell { x: 240, y: 135 }, tw, th, view);
        let centre = iso_project(240.5, 135.5, tw, th, o);
        assert_eq!(centre, [view[0] * 0.5, view[1] * 0.5]);
        assert_eq!(o, [540.0, -212.0], "the gate scene's pinned camera offset");

        // Out of bounds: clamped to the last real cell, so the camera still
        // frames the map instead of sailing off it. A grid built in memory can
        // name a destination the grid does not contain; a scenario cannot.
        let clamped = iso_origin(480, 270, Cell { x: 9_999, y: 9_999 }, tw, th, view);
        let last = iso_origin(480, 270, Cell { x: 479, y: 269 }, tw, th, view);
        assert_eq!(clamped, last, "an out-of-grid destination must clamp");
        assert_ne!(clamped, o, "…and clamping must actually move the camera");

        // A zero-sized grid has no cell to centre on and must not divide or
        // underflow its way to a non-finite origin.
        let empty = iso_origin(0, 0, Cell { x: 0, y: 0 }, tw, th, view);
        assert!(empty[0].is_finite() && empty[1].is_finite());
    }

    /// The cull is a pure predicate over two rects, and it rejects at each of
    /// the four edges while keeping anything that still straddles one.
    #[test]
    fn quad_visibility_is_a_rect_test() {
        let view = [1920.0f32, 1080.0f32];
        let size = [48.0f32, 48.0f32];

        assert!(quad_is_visible([900.0, 500.0], size, view), "dead centre");

        // Fully outside each edge.
        assert!(!quad_is_visible([-48.0, 500.0], size, view), "past left");
        assert!(!quad_is_visible([1920.0, 500.0], size, view), "past right");
        assert!(!quad_is_visible([900.0, -48.0], size, view), "past top");
        assert!(!quad_is_visible([900.0, 1080.0], size, view), "past bottom");

        // Straddling each edge: one pixel of the quad is still on screen.
        assert!(
            quad_is_visible([-47.0, 500.0], size, view),
            "straddles left"
        );
        assert!(
            quad_is_visible([1919.0, 500.0], size, view),
            "straddles right"
        );
        assert!(quad_is_visible([900.0, -47.0], size, view), "straddles top");
        assert!(
            quad_is_visible([900.0, 1079.0], size, view),
            "straddles bottom"
        );
    }

    #[test]
    fn instance_layout_is_stable() {
        assert_eq!(size_of::<SpriteInstance>(), 48);
        assert_eq!(align_of::<SpriteInstance>(), 4);
        assert_eq!(offset_of!(SpriteInstance, pos), 0);
        assert_eq!(offset_of!(SpriteInstance, size), 8);
        assert_eq!(offset_of!(SpriteInstance, uv_rect), 16);
        assert_eq!(offset_of!(SpriteInstance, tint), 32);
        assert_eq!(SpriteInstance::STRIDE, 48);

        assert_eq!(size_of::<QuadVertex>(), 16);
        assert_eq!(offset_of!(QuadVertex, corner), 0);
        assert_eq!(offset_of!(QuadVertex, uv), 8);

        assert_eq!(size_of::<FrameUniforms>(), 16);
    }
}
