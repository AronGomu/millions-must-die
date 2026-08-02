//! Scenario contract: versioned scene load + hash/validation gates.

use std::fs;
use std::path::{Path, PathBuf};

use mmd_engine::scenario::{Scenario, ScenarioError};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn v1_paths() -> (PathBuf, PathBuf) {
    let dir = workspace_root().join("assets/scenarios");
    (
        dir.join("technical_prototype_v1.ron"),
        dir.join("technical_prototype_v1.sha256"),
    )
}

fn write_temp_pair(dir: &Path, stem: &str, ron: &str) -> (PathBuf, PathBuf) {
    let ron_path = dir.join(format!("{stem}.ron"));
    let sha_path = dir.join(format!("{stem}.sha256"));
    let digest = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(ron.as_bytes());
        hex::encode(h.finalize())
    };
    fs::write(&ron_path, ron).expect("write ron");
    fs::write(&sha_path, format!("{digest}\n")).expect("write sha");
    (ron_path, sha_path)
}

#[test]
fn loads_v1_scene() {
    let (ron_path, _) = v1_paths();
    let scene = Scenario::load_verified(&ron_path).expect("v1 scene must load");

    assert_eq!(scene.version(), "technical_prototype_v1");
    assert_eq!(scene.width(), 480);
    assert_eq!(scene.height(), 270);
    assert_eq!(scene.cell_size_px(), 4);
    assert_eq!(scene.sprite_size_px(), 3);
    assert_eq!(scene.hard_agent_count(), 50_000);
    assert_eq!(scene.stretch_agent_count(), 100_000);
    assert_eq!(scene.atlas_count(), 4);
    assert_eq!(scene.direction_count(), 8);
    assert_eq!(scene.frame_count(), 4);
    assert_eq!(scene.obstacle_count(), 25_920);
    assert!(scene.seed() != 0, "seed must be fixed nonzero");
    assert_eq!(scene.destination().x, 240);
    assert_eq!(scene.destination().y, 135);
    assert!(!scene.spawn_cells().is_empty());
    assert_eq!(
        scene.obstacle_count() * 5,
        scene.width() * scene.height(),
        "exact 20% obstacles"
    );
}

#[test]
fn rejects_wrong_hash() {
    let (ron_path, sha_path) = v1_paths();
    let mut bytes = fs::read(&ron_path).expect("read v1 ron");
    let expected = fs::read_to_string(&sha_path).expect("read sha");
    // Flip one payload byte after hash computed over original.
    let idx = bytes.iter().position(|&b| b == b'0').expect("byte to flip");
    bytes[idx] = if bytes[idx] == b'1' { b'2' } else { b'1' };

    let err = Scenario::from_verified_bytes(&bytes, expected.trim())
        .expect_err("mutated bytes must fail hash");
    assert!(
        matches!(err, ScenarioError::HashMismatch { .. }),
        "expected HashMismatch, got {err:?}"
    );
}

#[test]
fn rejects_bad_obstacle_ratio() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Full v1 dims; 21% obstacles (27_216 of 129_600).
    let w: u32 = 480;
    let target = (w * 270) * 21 / 100; // 27216
    let dest_idx = 240 + 135 * w;
    let mut obstacles = Vec::with_capacity(target as usize);
    let mut i = 0u32;
    while (obstacles.len() as u32) < target {
        if i != dest_idx && i != 0 {
            obstacles.push(i);
        }
        i += 1;
    }
    let obs_ron = obstacles
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let ron = format!(
        r#"(
  version: "technical_prototype_v1",
  width: 480,
  height: 270,
  cell_size_px: 4,
  sprite_size_px: 3,
  hard_agent_count: 50000,
  stretch_agent_count: 100000,
  seed: 1,
  destination: (x: 240, y: 135),
  spawn_cells: [(x: 0, y: 0)],
  atlas_count: 4,
  direction_count: 8,
  frame_count: 4,
  obstacle_cells: [{obs_ron}],
)
"#
    );
    let (ron_path, _) = write_temp_pair(dir.path(), "bad_ratio", &ron);
    let err = Scenario::load_verified(&ron_path).expect_err("21% must fail");
    assert!(
        matches!(err, ScenarioError::InvalidObstacleRatio { .. }),
        "expected InvalidObstacleRatio, got {err:?}"
    );
}

#[test]
fn rejects_unreachable_spawn() {
    let dir = tempfile::tempdir().expect("tempdir");
    // Full v1 dimensions; seal spawn (0,0) with a ring of obstacles.
    // Destination center free. Remaining obstacles fill to exact 20%.
    let w: u32 = 480;
    let h: u32 = 270;
    let total = w * h;
    let target_obs = total / 5; // 20%
    let dest_idx = 240 + 135 * w;

    let mut blocked = vec![false; total as usize];
    // Seal (0,0): block (1,0),(0,1),(1,1) — corner spawn isolated.
    // Keep (2,0) free + corridor so only sealed spawn fails reachability.
    let seal = [1u32, w, w + 1];
    for &s in &seal {
        blocked[s as usize] = true;
    }
    let spawn_open = 2u32; // (2,0)

    let mut obstacles = seal.to_vec();
    let mut i = 0u32;
    while (obstacles.len() as u32) < target_obs {
        let x = i % w;
        let y = i / w;
        let on_corridor = y == 135 || (y == 0 && x >= 2);
        let reserved = i == dest_idx || i == 0 || i == spawn_open || blocked[i as usize];
        if !reserved && !on_corridor {
            blocked[i as usize] = true;
            obstacles.push(i);
        }
        i += 1;
        if i >= total {
            break;
        }
    }
    assert_eq!(obstacles.len() as u32, target_obs, "fixture must stay 20%");
    assert!(!blocked[0] && !blocked[spawn_open as usize]);

    let obs_ron = obstacles
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let ron = format!(
        r#"(
  version: "technical_prototype_v1",
  width: 480,
  height: 270,
  cell_size_px: 4,
  sprite_size_px: 3,
  hard_agent_count: 50000,
  stretch_agent_count: 100000,
  seed: 99,
  destination: (x: 240, y: 135),
  spawn_cells: [(x: 0, y: 0), (x: 2, y: 0)],
  atlas_count: 4,
  direction_count: 8,
  frame_count: 4,
  obstacle_cells: [{obs_ron}],
)
"#
    );
    let (ron_path, _) = write_temp_pair(dir.path(), "sealed_spawn", &ron);
    let err = Scenario::load_verified(&ron_path).expect_err("sealed spawn must fail");
    assert!(
        matches!(err, ScenarioError::UnreachableSpawn { .. }),
        "expected UnreachableSpawn, got {err:?}"
    );
}
