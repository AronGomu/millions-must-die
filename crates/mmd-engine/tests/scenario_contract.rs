//! Scenario contract: versioned scene load + hash/validation gates.

use std::fs;
use std::path::{Path, PathBuf};

use mmd_engine::scenario::{
    COLLISION_SCENE_MAX_AGENTS, COLLISION_SCENE_V1, Cell, FIXTURE_MAX_AGENTS, FIXTURE_MAX_CELLS,
    MAX_COLLISION_RADIUS_Q8, Scenario, ScenarioError, ScenarioSpec,
};
use mmd_engine::testkit::{COLLISION_MID_SCENE, COLLISION_SPRITE_SCENE, scene_path};

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
    assert_eq!(scene.sprite_size_px(), 30);
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
fn gate_scene_locks_its_collision_tuning() {
    let (ron_path, _) = v1_paths();
    let scene = Scenario::load_verified(&ron_path).expect("v1 scene must load");
    assert_eq!(scene.collision_radius_q8(), 102);
    assert_eq!(scene.separation_strength_q8(), 256);
    assert!(
        (scene.collision_radius_cells() - 0.398_437_5).abs() < f32::EPSILON,
        "collision_radius_cells: {}",
        scene.collision_radius_cells()
    );
    assert!(
        (scene.separation_strength() - 1.0).abs() < f32::EPSILON,
        "separation_strength: {}",
        scene.separation_strength()
    );
}

#[test]
fn v1_rejects_a_retuned_collision_radius() {
    let (ron_path, _) = v1_paths();
    let text = fs::read_to_string(&ron_path).expect("read v1 ron");
    let mutated = text.replace("collision_radius_q8: 102,", "collision_radius_q8: 103,");
    assert_ne!(
        text, mutated,
        "collision_radius_q8 anchor not found in v1 ron"
    );
    let err =
        Scenario::parse_and_validate(mutated.as_bytes()).expect_err("retuned radius must fail");
    match err {
        ScenarioError::InvalidDimension(msg) => assert!(
            msg.contains("collision_radius_q8"),
            "expected msg to mention collision_radius_q8, got {msg:?}"
        ),
        other => panic!("expected InvalidDimension, got {other:?}"),
    }
}

#[test]
fn collision_radius_above_the_cap_is_refused() {
    let spec = ScenarioSpec {
        collision_radius_q8: MAX_COLLISION_RADIUS_Q8 + 1,
        ..fixture_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidCollision(msg)) => assert!(
            msg.contains("collision_radius_q8"),
            "expected msg to mention collision_radius_q8, got {msg:?}"
        ),
        other => panic!("expected InvalidCollision, got {other:?}"),
    }
}

#[test]
fn separation_strength_without_a_body_is_refused() {
    let spec = ScenarioSpec {
        collision_radius_q8: 0,
        separation_strength_q8: 256,
        ..fixture_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidCollision(msg)) => assert!(
            msg.contains("pushes nothing"),
            "expected msg to mention 'pushes nothing', got {msg:?}"
        ),
        other => panic!("expected InvalidCollision, got {other:?}"),
    }
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
  sprite_size_px: 30,
  hard_agent_count: 50000,
  stretch_agent_count: 100000,
  seed: 1,
  destination: (x: 240, y: 135),
  spawn_cells: [(x: 0, y: 0)],
  atlas_count: 4,
  direction_count: 8,
  frame_count: 4,
  collision_radius_q8: 102,
  separation_strength_q8: 256,
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
  sprite_size_px: 30,
  hard_agent_count: 50000,
  stretch_agent_count: 100000,
  seed: 99,
  destination: (x: 240, y: 135),
  spawn_cells: [(x: 0, y: 0), (x: 2, y: 0)],
  atlas_count: 4,
  direction_count: 8,
  frame_count: 4,
  collision_radius_q8: 102,
  separation_strength_q8: 256,
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

// --- fixture family (T29) ---------------------------------------------------
//
// The `fixture_*` family relaxes the frozen v1 geometry so small harness
// scenarios are expressible. These tests pin what it does *not* relax: without
// them the caps below would be unenforced claims, and deleting a check would
// leave every test green.

/// Minimal valid fixture: 8×8 open grid, destination reachable from one spawn.
fn fixture_spec() -> ScenarioSpec {
    ScenarioSpec {
        version: "fixture_negative_v1".to_string(),
        width: 8,
        height: 8,
        cell_size_px: 4,
        sprite_size_px: 30,
        hard_agent_count: 4,
        stretch_agent_count: 64,
        seed: 1,
        destination: Cell { x: 7, y: 7 },
        spawn_cells: vec![Cell { x: 0, y: 0 }],
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 64,
        separation_strength_q8: 256,
        obstacle_cells: vec![],
    }
}

fn expect_invalid_dimension(spec: ScenarioSpec, needle: &str) {
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidDimension(msg)) => assert!(
            msg.contains(needle),
            "expected InvalidDimension containing {needle:?}, got {msg:?}"
        ),
        other => panic!("expected InvalidDimension containing {needle:?}, got {other:?}"),
    }
}

#[test]
fn fixture_baseline_spec_is_valid() {
    // Guards the negative tests below: each mutates exactly one field, so the
    // baseline must pass or they would prove nothing.
    let scene = Scenario::from_spec(fixture_spec()).expect("baseline fixture must validate");
    assert_eq!(scene.width(), 8);
    assert_eq!(scene.hard_agent_count(), 4);
}

#[test]
fn fixture_rejects_oversized_grid() {
    // 257 × 256 = 65_792 > 65_536.
    let spec = ScenarioSpec {
        width: 257,
        height: 256,
        destination: Cell { x: 0, y: 0 },
        ..fixture_spec()
    };
    const { assert!(257 * 256 > FIXTURE_MAX_CELLS) };
    expect_invalid_dimension(spec, "exceeds cap");
}

#[test]
fn fixture_rejects_oversized_agent_counts() {
    expect_invalid_dimension(
        ScenarioSpec {
            hard_agent_count: FIXTURE_MAX_AGENTS + 1,
            stretch_agent_count: FIXTURE_MAX_AGENTS + 1,
            ..fixture_spec()
        },
        "hard_agent_count",
    );
    expect_invalid_dimension(
        ScenarioSpec {
            stretch_agent_count: FIXTURE_MAX_AGENTS + 1,
            ..fixture_spec()
        },
        "stretch_agent_count",
    );
}

#[test]
fn fixture_rejects_stretch_below_hard() {
    expect_invalid_dimension(
        ScenarioSpec {
            hard_agent_count: 100,
            stretch_agent_count: 50,
            ..fixture_spec()
        },
        "below hard_agent_count",
    );
}

#[test]
fn fixture_rejects_zero_dimensions() {
    for (spec, needle) in [
        (
            ScenarioSpec {
                width: 0,
                ..fixture_spec()
            },
            "width",
        ),
        (
            ScenarioSpec {
                height: 0,
                ..fixture_spec()
            },
            "height",
        ),
        (
            ScenarioSpec {
                cell_size_px: 0,
                ..fixture_spec()
            },
            "cell_size_px",
        ),
        (
            ScenarioSpec {
                sprite_size_px: 0,
                ..fixture_spec()
            },
            "sprite_size_px",
        ),
        (
            ScenarioSpec {
                hard_agent_count: 0,
                ..fixture_spec()
            },
            "hard_agent_count",
        ),
    ] {
        expect_invalid_dimension(spec, needle);
    }
}

#[test]
fn fixture_must_honour_the_renderer_contract() {
    // Fixtures may pick their own grid, never their own sprite-sheet geometry:
    // `frame_uv_rect` and ATLAS_COUNT address exactly 4 atlases × 8 dirs × 4
    // frames, so a divergent fixture would index outside the atlas.
    for (spec, needle) in [
        (
            ScenarioSpec {
                atlas_count: 3,
                ..fixture_spec()
            },
            "atlas_count",
        ),
        (
            ScenarioSpec {
                direction_count: 7,
                ..fixture_spec()
            },
            "direction_count",
        ),
        (
            ScenarioSpec {
                frame_count: 5,
                ..fixture_spec()
            },
            "frame_count",
        ),
    ] {
        expect_invalid_dimension(spec, needle);
    }
}

#[test]
fn fixture_rejects_fully_blocked_grid() {
    let spec = ScenarioSpec {
        width: 4,
        height: 3,
        destination: Cell { x: 3, y: 1 },
        obstacle_cells: (0..12).collect(),
        ..fixture_spec()
    };
    assert!(matches!(
        Scenario::from_spec(spec),
        Err(ScenarioError::InvalidObstacleRatio {
            obstacles: 12,
            cells: 12
        })
    ));
}

#[test]
fn fixture_still_enforces_the_shared_structural_rules() {
    // The relaxation is scoped to geometry. Seed, spawns and reachability are
    // enforced for every family.
    assert!(matches!(
        Scenario::from_spec(ScenarioSpec {
            seed: 0,
            ..fixture_spec()
        }),
        Err(ScenarioError::InvalidSeed)
    ));
    assert!(matches!(
        Scenario::from_spec(ScenarioSpec {
            spawn_cells: vec![],
            ..fixture_spec()
        }),
        Err(ScenarioError::EmptySpawns)
    ));
    // Seal the spawn corner (0,0) off from the destination.
    assert!(matches!(
        Scenario::from_spec(ScenarioSpec {
            obstacle_cells: vec![1, 8, 9],
            ..fixture_spec()
        }),
        Err(ScenarioError::UnreachableSpawn { x: 0, y: 0 })
    ));
    assert!(matches!(
        Scenario::from_spec(ScenarioSpec {
            spawn_cells: vec![Cell { x: 99, y: 0 }],
            ..fixture_spec()
        }),
        Err(ScenarioError::InvalidSpawn { x: 99, y: 0 })
    ));
}

#[test]
fn unknown_version_family_is_still_rejected() {
    // The prefix branch must not become a bypass for arbitrary version ids.
    for version in ["evil_v1", "fixture", "technical_prototype_v2", ""] {
        let spec = ScenarioSpec {
            version: version.to_string(),
            ..fixture_spec()
        };
        assert!(
            matches!(
                Scenario::from_spec(spec),
                Err(ScenarioError::UnsupportedVersion(_))
            ),
            "version {version:?} must be rejected"
        );
    }
}

// --- collision_scene_v1 family (T5) -----------------------------------------
//
// Synthetic minimal spec for the negative tests: same locked geometry as the
// tracked scenes, but a trivial single-spawn/no-obstacle body so the
// dimension/collision checks (which run before obstacle validation) are what
// gets exercised.
fn collision_scene_spec() -> ScenarioSpec {
    ScenarioSpec {
        version: COLLISION_SCENE_V1.to_string(),
        width: 480,
        height: 270,
        cell_size_px: 4,
        sprite_size_px: 30,
        hard_agent_count: 10_000,
        stretch_agent_count: 20_000,
        seed: 7_355_608_251_463_129_073,
        destination: Cell { x: 240, y: 135 },
        spawn_cells: vec![Cell { x: 2, y: 8 }],
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 320,
        separation_strength_q8: 256,
        obstacle_cells: vec![],
    }
}

#[test]
fn collision_scenes_load_and_verify() {
    let mid = Scenario::load_verified(scene_path(COLLISION_MID_SCENE)).expect("mid must load");
    let sprite =
        Scenario::load_verified(scene_path(COLLISION_SPRITE_SCENE)).expect("sprite must load");

    assert_eq!(mid.hard_agent_count(), 10_000);
    assert_eq!(mid.collision_radius_q8(), 320);
    assert_eq!(mid.version(), COLLISION_SCENE_V1);

    assert_eq!(sprite.hard_agent_count(), 1_200);
    assert_eq!(sprite.collision_radius_q8(), 960);
    assert_eq!(sprite.version(), COLLISION_SCENE_V1);
}

#[test]
fn collision_scene_locks_the_screen_geometry() {
    let spec = ScenarioSpec {
        width: 481,
        ..collision_scene_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidDimension(msg)) => {
            assert!(msg.contains("width"), "expected width in msg, got {msg:?}")
        }
        other => panic!("expected InvalidDimension, got {other:?}"),
    }
}

#[test]
fn collision_scene_refuses_a_bodyless_scene() {
    let spec = ScenarioSpec {
        collision_radius_q8: 0,
        separation_strength_q8: 0,
        ..collision_scene_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidCollision(msg)) => assert!(
            msg.contains("nonzero collision_radius_q8"),
            "expected msg to mention nonzero collision_radius_q8, got {msg:?}"
        ),
        other => panic!("expected InvalidCollision, got {other:?}"),
    }
}

#[test]
fn collision_scene_caps_its_population() {
    let spec = ScenarioSpec {
        hard_agent_count: COLLISION_SCENE_MAX_AGENTS + 1,
        stretch_agent_count: COLLISION_SCENE_MAX_AGENTS + 1,
        ..collision_scene_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidDimension(msg)) => assert!(
            msg.contains("exceeds cap"),
            "expected msg to mention exceeds cap, got {msg:?}"
        ),
        other => panic!("expected InvalidDimension, got {other:?}"),
    }
}

#[test]
fn v1_geometry_stays_frozen_against_the_fixture_relaxation() {
    // Belt-and-braces on the highest-risk edit in T29: a scenario that claims
    // to be the gate scene must still satisfy every frozen constant, so the
    // fixture branch cannot be reached by naming.
    let mut v1 = fixture_spec();
    v1.version = "technical_prototype_v1".to_string();
    match Scenario::from_spec(v1) {
        Err(ScenarioError::InvalidDimension(msg)) => {
            assert!(
                msg.contains("width"),
                "expected frozen width check, got {msg}"
            )
        }
        other => panic!("v1 must reject fixture geometry, got {other:?}"),
    }
}
