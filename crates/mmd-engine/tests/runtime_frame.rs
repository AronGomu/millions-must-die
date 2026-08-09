//! Runtime frame composition (T8): tick, pause, instance partition, input map.

use std::path::PathBuf;

use mmd_engine::render::{ATLAS_COUNT, IsoView, SpriteInstance};
use mmd_engine::runtime::{BoundKey, InputAction, Runtime, action_for_key};
use mmd_engine::sim::AgentsView;
use mmd_engine::testkit::{COLLISION_SPRITE_SCENE, scene_path};

/// How many alive agents have a quad of `size` the view can see at all, where
/// `anchor` places the quad's top-left relative to the agent's ground point.
///
/// The tracked scenes project to a map diamond larger than the 1920 × 1080
/// view, so the packer culls and "one instance per alive agent" is no longer
/// the claim — "one instance per *visible* agent" is. The projection and the
/// rect test are written out longhand rather than borrowed from the engine, so
/// the expectation does not flow through the code under test.
fn visible_quads(agents: AgentsView<'_>, iso: &IsoView, size: [f32; 2], anchor: [f32; 2]) -> usize {
    (0..agents.x.len())
        .filter(|&i| {
            let (cx, cy) = (agents.x[i], agents.y[i]);
            let sx = iso.origin[0] + (cx - cy) * iso.tile_w * 0.5;
            let sy = iso.origin[1] + (cx + cy) * iso.tile_h * 0.5;
            let (px, py) = (sx + anchor[0], sy + anchor[1]);
            px + size[0] > 0.0
                && py + size[1] > 0.0
                && px < iso.view_size[0]
                && py < iso.view_size[1]
        })
        .count()
}

/// [`visible_quads`] for a sprite: a square quad standing on its bottom edge.
fn visible_count(agents: AgentsView<'_>, iso: &IsoView, sprite: f32) -> usize {
    visible_quads(agents, iso, [sprite, sprite], [-sprite * 0.5, -sprite])
}

/// [`visible_quads`] for a hitbox ring: a 2:1 ellipse centred on the ground
/// point, `2r` across each projected axis.
fn visible_rings(agents: AgentsView<'_>, iso: &IsoView, radius_cells: f32) -> usize {
    let size = [
        2.0 * radius_cells * iso.tile_w,
        2.0 * radius_cells * iso.tile_h,
    ];
    visible_quads(agents, iso, size, [-size[0] * 0.5, -size[1] * 0.5])
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn gate_scenario() -> PathBuf {
    workspace_root().join("assets/scenarios/technical_prototype_v1.ron")
}

/// A scene with a real body, so there is something for a ring to trace.
///
/// Named through the testkit rather than hand-joined, so renaming the asset
/// breaks the build instead of this test at runtime.
fn bodied_scenario() -> PathBuf {
    scene_path(COLLISION_SPRITE_SCENE)
}

#[test]
fn frame_ticks_once() {
    let mut rt = Runtime::load(gate_scenario(), None).expect("load");
    assert_eq!(rt.tick_index(), 0);
    let before = rt.tick_index();
    let _ = rt.tick_and_render();
    assert_eq!(rt.tick_index(), before + 1);
}

#[test]
fn pause_keeps_checksum() {
    let mut rt = Runtime::load(gate_scenario(), Some(256)).expect("load");
    // Warm one tick so state is past spawn-only.
    let _ = rt.tick_and_render();
    let hash = rt.state_hash();
    let tick = rt.tick_index();
    rt.apply_action(InputAction::TogglePause);
    assert!(rt.paused());
    let out_hash = rt.tick_and_render().state_hash;
    assert_eq!(rt.tick_index(), tick);
    assert_eq!(rt.state_hash(), hash);
    assert_eq!(out_hash, hash);
}

#[test]
fn builds_one_instance_per_agent() {
    let mut rt = Runtime::load(gate_scenario(), None).expect("load");
    let population = rt.agent_count();
    // The drawn quad is the *scenario's* sprite size, not the atlas source
    // frame: `render::SPRITE_SIZE_PX` only sizes the static GPU-golden demo.
    let sprite_px = rt.scenario().sprite_size_px() as f32;
    let iso = rt.iso_view();
    let total: usize = {
        let out = rt.tick_and_render();
        for inst in out.groups.iter().flat_map(|group| &group.instances) {
            assert_eq!(inst.size, [sprite_px, sprite_px]);
        }
        out.groups.iter().map(|g| g.instances.len()).sum()
    };
    // One instance per agent the view can see. Recomputed from sim state rather
    // than hard-coded, so retuning the scene cannot leave this asserting a
    // population that no longer exists.
    let expected = visible_count(rt.agents(), &iso, sprite_px);
    assert_eq!(total, expected);
    // …and the gate scene really does exercise both sides of the cull: its
    // 480 × 270 grid projects to a 3000 × 1500 px diamond against a 1920 × 1080
    // view. Without this the case would silently degrade to a tautology if the
    // cull ever started rejecting everything.
    assert!(
        total > 0 && total < population,
        "packed {total} of {population} agents; the gate scene must have some agents \
         on screen and some off it"
    );
}

#[test]
fn partitions_four_groups() {
    let mut rt = Runtime::load(gate_scenario(), None).expect("load");
    let sprite_px = rt.scenario().sprite_size_px() as f32;
    let iso = rt.iso_view();
    let counts = {
        let out = rt.tick_and_render();
        assert_eq!(out.groups.len(), ATLAS_COUNT);
        let mut counts = [0u32; ATLAS_COUNT];
        for (i, g) in out.groups.iter().enumerate() {
            assert_eq!(g.atlas_id as usize, i);
            counts[i] = g.instances.len() as u32;
        }
        counts
    };
    // Exact per-bucket expectation, recomputed from sim state. The cull removes
    // a screen-space region rather than a multiple of four, so the split is no
    // longer exactly even — but "which bucket each surviving agent lands in" is
    // still exactly determined, and a band would let a packer misroute a few
    // percent of instances into group 0 unnoticed.
    let want = {
        let v = rt.agents();
        let mut want = [0u32; ATLAS_COUNT];
        for i in 0..v.x.len() {
            let (cx, cy) = (v.x[i], v.y[i]);
            let sx = iso.origin[0] + (cx - cy) * iso.tile_w * 0.5;
            let sy = iso.origin[1] + (cx + cy) * iso.tile_h * 0.5;
            let (px, py) = (sx - sprite_px * 0.5, sy - sprite_px);
            if px + sprite_px > 0.0
                && py + sprite_px > 0.0
                && px < iso.view_size[0]
                && py < iso.view_size[1]
            {
                want[v.atlas[i] as usize % ATLAS_COUNT] += 1;
            }
        }
        want
    };
    assert_eq!(counts, want, "per-bucket instance counts");

    let sum: usize = counts.iter().map(|&c| c as usize).sum();
    assert_eq!(sum, visible_count(rt.agents(), &iso, sprite_px));
    assert!(
        sum > 0 && sum < rt.agent_count(),
        "packed {sum} of {} agents; the gate scene must exercise both sides of the cull",
        rt.agent_count()
    );
}

#[test]
fn input_actions_are_stable() {
    assert_eq!(action_for_key(BoundKey::Escape), InputAction::Quit);
    assert_eq!(action_for_key(BoundKey::F1), InputAction::ToggleOverlay);
    assert_eq!(action_for_key(BoundKey::Space), InputAction::TogglePause);
    assert_eq!(action_for_key(BoundKey::H), InputAction::ToggleHitboxes);
    // Discriminants stay fixed for overlay/log contracts.
    assert_eq!(InputAction::Quit as u8, 0);
    assert_eq!(InputAction::ToggleOverlay as u8, 1);
    assert_eq!(InputAction::TogglePause as u8, 2);
    // Appended, never inserted: renumbering an existing action would silently
    // repoint any recorded log or script that names one by value.
    assert_eq!(InputAction::ToggleHitboxes as u8, 3);
}

/// The hitbox overlay is exactly that — an overlay.
///
/// Toggling it may move the ring count between `n` and `0` and must move
/// *nothing else*: not the simulation, and not one byte of the four atlas
/// groups. The sim is paused first so the only thing that can explain a
/// difference between the two packs is the toggle itself; without that, a tick
/// would legitimately move every sprite and the comparison would prove nothing.
#[test]
fn toggling_hitboxes_changes_only_the_rings() {
    let mut rt = Runtime::load(bodied_scenario(), Some(256)).expect("load");
    assert!(
        rt.scenario().collision_radius_q8() > 0,
        "this scene must have a body, or 'rings appear' is unfalsifiable"
    );
    // Warm one tick so the state is past spawn-only, then freeze it.
    let _ = rt.tick_and_render();
    rt.set_paused(true);

    let agents = rt.agent_count();
    let iso = rt.iso_view();
    let radius_cells = rt.sim().collision().radius_cells;
    assert!(
        rt.hitboxes_visible(),
        "hitboxes are on by default — that is the point of the ticket"
    );

    let snapshot = |rt: &mut Runtime| {
        let out = rt.tick_and_render();
        let groups: Vec<Vec<SpriteInstance>> =
            out.groups.iter().map(|g| g.instances.clone()).collect();
        (out.state_hash, groups, out.rings.to_vec())
    };

    let (hash_on, groups_on, rings_on) = snapshot(&mut rt);
    // Not `agents`: this scene's map diamond is larger than the view, so the
    // packer culls. Still an exact count, though — recomputed from sim state
    // through this file's own longhand rect test, so a packer that emitted a
    // ring for every *other* agent would still fail.
    let want_rings = visible_rings(rt.agents(), &iso, radius_cells);
    assert_eq!(
        rings_on.len(),
        want_rings,
        "packed {} rings for {want_rings} on-screen agents (of {agents} alive)",
        rings_on.len()
    );
    assert!(
        want_rings > 0,
        "no ring is on screen; the toggle proves nothing"
    );

    rt.apply_action(InputAction::ToggleHitboxes);
    assert!(!rt.hitboxes_visible());
    let (hash_off, groups_off, rings_off) = snapshot(&mut rt);
    assert!(rings_off.is_empty(), "hiding hitboxes must pack no rings");
    assert_eq!(
        hash_off, hash_on,
        "the overlay reached the simulation — a render toggle must never move the state hash"
    );
    assert_eq!(
        groups_off, groups_on,
        "hiding the rings changed an atlas group; the overlay must be additive only"
    );

    // And back: the toggle is symmetric, not a one-way switch.
    rt.apply_action(InputAction::ToggleHitboxes);
    assert!(rt.hitboxes_visible());
    let (hash_back, groups_back, rings_back) = snapshot(&mut rt);
    assert_eq!(rings_back, rings_on, "re-showing rebuilt different rings");
    assert_eq!(groups_back, groups_on);
    assert_eq!(hash_back, hash_on);
}
