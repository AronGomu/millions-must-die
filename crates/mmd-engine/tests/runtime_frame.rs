//! Runtime frame composition (T8): tick, pause, instance partition, input map.

use std::path::PathBuf;

use mmd_engine::render::{ATLAS_COUNT, SpriteInstance};
use mmd_engine::runtime::{BoundKey, InputAction, Runtime, action_for_key};
use mmd_engine::testkit::{COLLISION_SPRITE_SCENE, scene_path};

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
    // The scenario's hard count is the expectation, so retuning the scene
    // cannot leave this test asserting a population that no longer exists.
    let expected = rt.agent_count();
    // The drawn quad is the *scenario's* sprite size, not the atlas source
    // frame: `render::SPRITE_SIZE_PX` only sizes the static GPU-golden demo.
    let sprite_px = rt.scenario().sprite_size_px() as f32;
    let total: usize = {
        let out = rt.tick_and_render();
        for inst in out.groups.iter().flat_map(|group| &group.instances) {
            assert_eq!(inst.size, [sprite_px, sprite_px]);
        }
        out.groups.iter().map(|g| g.instances.len()).sum()
    };
    assert_eq!(total, expected);
}

#[test]
fn partitions_four_groups() {
    let mut rt = Runtime::load(gate_scenario(), None).expect("load");
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
    let sum: usize = counts.iter().map(|&c| c as usize).sum();
    assert_eq!(sum, rt.agent_count());
    // Exact even split for the hard count (5000 % 4 == 0).
    let per_group = (rt.agent_count() / ATLAS_COUNT) as u32;
    assert!(
        counts.iter().all(|&c| c == per_group),
        "counts={counts:?} per_group={per_group}"
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
    assert_eq!(
        rings_on.len(),
        agents,
        "one ring per agent while hitboxes are visible"
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
