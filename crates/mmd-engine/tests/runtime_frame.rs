//! Runtime frame composition (T8): tick, pause, instance partition, input map.

use std::path::PathBuf;

use mmd_engine::render::{ATLAS_COUNT, SPRITE_SIZE_PX};
use mmd_engine::runtime::{BoundKey, InputAction, Runtime, action_for_key};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn gate_scenario() -> PathBuf {
    workspace_root().join("assets/scenarios/technical_prototype_v1.ron")
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
fn builds_50000_instances() {
    let mut rt = Runtime::load(gate_scenario(), None).expect("load");
    assert_eq!(rt.agent_count(), 50_000);
    let total: usize = {
        let out = rt.tick_and_render();
        for inst in out.groups.iter().flat_map(|group| &group.instances) {
            assert_eq!(inst.size, [SPRITE_SIZE_PX as f32, SPRITE_SIZE_PX as f32]);
        }
        out.groups.iter().map(|g| g.instances.len()).sum()
    };
    assert_eq!(total, 50_000);
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
    let sum: u32 = counts.iter().sum();
    assert_eq!(sum, 50_000);
    // Exact even split for hard count (50000 % 4 == 0).
    assert!(counts.iter().all(|&c| c == 12_500), "counts={counts:?}");
}

#[test]
fn input_actions_are_stable() {
    assert_eq!(action_for_key(BoundKey::Escape), InputAction::Quit);
    assert_eq!(action_for_key(BoundKey::F1), InputAction::ToggleOverlay);
    assert_eq!(action_for_key(BoundKey::Space), InputAction::TogglePause);
    // Discriminants stay fixed for overlay/log contracts.
    assert_eq!(InputAction::Quit as u8, 0);
    assert_eq!(InputAction::ToggleOverlay as u8, 1);
    assert_eq!(InputAction::TogglePause as u8, 2);
}
