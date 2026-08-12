//! Scenario contract: versioned scene load + hash/validation gates.

use std::fs;
use std::path::{Path, PathBuf};

use mmd_engine::scenario::{
    COLLISION_SCENE_V1, Cell, FIXTURE_MAX_AGENTS, FIXTURE_MAX_CELLS, HQ_FOOTPRINT_CELLS,
    MAX_COLLISION_RADIUS_Q8, MAX_LIVE_AGENTS, MAX_SEPARATION_STRENGTH_Q8, MAX_START_RESOURCE,
    MAX_SUPPLY_CAP, RTS_PROTOTYPE_V1, RtsSpec, Scenario, ScenarioError, ScenarioSpec,
};
use mmd_engine::testkit::{
    ALL_COLLISION_SCENES, ALL_FIXTURES, COLLISION_MID_SCENE, COLLISION_SPRITE_SCENE, fixture_path,
    rts_scene_path, scene_path,
};

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
    assert_eq!(scene.sprite_size_px(), 48);
    assert_eq!(scene.hard_agent_count(), 5_000);
    assert_eq!(scene.stretch_agent_count(), 5_000);
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
    assert_eq!(scene.collision_radius_q8(), 1_536);
    assert_eq!(scene.separation_strength_q8(), 256);
    assert!(
        (scene.collision_radius_cells() - 6.0).abs() < f32::EPSILON,
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
    let mutated = text.replace("collision_radius_q8: 1536,", "collision_radius_q8: 1537,");
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
fn separation_strength_above_the_cap_is_refused() {
    // The radius cap had a test; the strength cap did not, so replacing its
    // whole check with `if false` left this file green.
    let spec = ScenarioSpec {
        separation_strength_q8: MAX_SEPARATION_STRENGTH_Q8 + 1,
        ..fixture_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidCollision(msg)) => assert!(
            msg.contains("separation_strength_q8"),
            "expected msg to mention separation_strength_q8, got {msg:?}"
        ),
        other => panic!("expected InvalidCollision, got {other:?}"),
    }
}

#[test]
fn the_collision_caps_are_inclusive_bounds() {
    // Both caps are the largest *accepted* value, not the first rejected one.
    // Without this, flipping either `>` to `>=` passed every other test here.
    let spec = ScenarioSpec {
        collision_radius_q8: MAX_COLLISION_RADIUS_Q8,
        separation_strength_q8: MAX_SEPARATION_STRENGTH_Q8,
        ..fixture_spec()
    };
    let scenario = Scenario::from_spec(spec).expect("both caps must be accepted at the boundary");
    assert_eq!(scenario.collision_radius_q8(), MAX_COLLISION_RADIUS_Q8);
    assert_eq!(
        scenario.separation_strength_q8(),
        MAX_SEPARATION_STRENGTH_Q8
    );
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
  sprite_size_px: 48,
  hard_agent_count: 5000,
  stretch_agent_count: 5000,
  seed: 1,
  destination: (x: 240, y: 135),
  spawn_cells: [(x: 0, y: 0)],
  atlas_count: 4,
  direction_count: 8,
  frame_count: 4,
  collision_radius_q8: 1536,
  separation_strength_q8: 256,
  separation_phases: 1,
  mass_class_count: 1,
  separation_threads: 1,
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
  sprite_size_px: 48,
  hard_agent_count: 5000,
  stretch_agent_count: 5000,
  seed: 99,
  destination: (x: 240, y: 135),
  spawn_cells: [(x: 0, y: 0), (x: 2, y: 0)],
  atlas_count: 4,
  direction_count: 8,
  frame_count: 4,
  collision_radius_q8: 1536,
  separation_strength_q8: 256,
  separation_phases: 1,
  mass_class_count: 1,
  separation_threads: 1,
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
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
        obstacle_cells: vec![],
        rts: None,
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
        sprite_size_px: 48,
        hard_agent_count: 1_200,
        stretch_agent_count: 5_000,
        seed: 7_355_608_251_463_129_073,
        destination: Cell { x: 240, y: 135 },
        spawn_cells: vec![Cell { x: 2, y: 8 }],
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 1_536,
        separation_strength_q8: 256,
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
        obstacle_cells: vec![],
        rts: None,
    }
}

#[test]
fn collision_scenes_load_and_verify() {
    let mid = Scenario::load_verified(scene_path(COLLISION_MID_SCENE)).expect("mid must load");
    let sprite =
        Scenario::load_verified(scene_path(COLLISION_SPRITE_SCENE)).expect("sprite must load");

    assert_eq!(mid.hard_agent_count(), 5_000);
    assert_eq!(mid.collision_radius_q8(), 1_536);
    assert_eq!(mid.version(), COLLISION_SCENE_V1);

    assert_eq!(sprite.hard_agent_count(), 1_200);
    assert_eq!(sprite.collision_radius_q8(), 1_536);
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
fn collision_scene_refuses_a_body_with_no_separation_weight() {
    // A collision scene exists to demonstrate separation. Declaring a body and
    // then setting the weight to 0 passes every other check while
    // `CollisionParams::enabled()` returns false and the separation pass never
    // runs at all — a scene that silently demonstrates nothing.
    let spec = ScenarioSpec {
        separation_strength_q8: 0,
        ..collision_scene_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidCollision(msg)) => assert!(
            msg.contains("nonzero separation_strength_q8"),
            "expected msg to mention nonzero separation_strength_q8, got {msg:?}"
        ),
        other => panic!("expected InvalidCollision, got {other:?}"),
    }
}

#[test]
fn collision_scene_caps_its_population() {
    // The family has no cap of its own: a second, larger collision-scene cap
    // would let a demo scene declare a horde the engine refuses to run, so this
    // family is bound by the engine ceiling like every other.
    let spec = ScenarioSpec {
        hard_agent_count: MAX_LIVE_AGENTS + 1,
        stretch_agent_count: MAX_LIVE_AGENTS + 1,
        ..collision_scene_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidDimension(msg)) => assert!(
            msg.contains("exceeds MAX_LIVE_AGENTS"),
            "expected msg to mention exceeds MAX_LIVE_AGENTS, got {msg:?}"
        ),
        other => panic!("expected InvalidDimension, got {other:?}"),
    }
}

// --- the simultaneous-entity ceiling (T0) -----------------------------------
//
// `MAX_LIVE_AGENTS` is the absolute ceiling of the engine, not a per-family
// tuning knob. These tests pin it from three angles: no family may exceed it,
// no tracked scene does, and the locked body stays inside the radius cap so a
// later body bump cannot silently overshoot.

/// The v1 gate scene's own RON with its two population lines rewritten.
///
/// Rewriting the real file rather than hand-building a spec keeps the 25 920
/// obstacles, the exact-20% ratio and the reachability graph honest, so the
/// only thing under test is the population.
fn v1_ron_with_population(hard: u32, stretch: u32) -> String {
    let (ron_path, _) = v1_paths();
    let text = fs::read_to_string(&ron_path).expect("read v1 ron");
    let mut saw_hard = false;
    let mut saw_stretch = false;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if line.trim_start().starts_with("hard_agent_count:") {
            saw_hard = true;
            out.push_str(&format!("  hard_agent_count: {hard},"));
        } else if line.trim_start().starts_with("stretch_agent_count:") {
            saw_stretch = true;
            out.push_str(&format!("  stretch_agent_count: {stretch},"));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    assert!(
        saw_hard && saw_stretch,
        "population anchors not found in v1 ron"
    );
    out
}

/// The three scenario families, each validated at the given population.
///
/// A fourth family that forgets the ceiling has to be added here to be tested,
/// which is the point: the list is the enumeration the cap check must cover.
fn validate_family_at(family: &str, hard: u32, stretch: u32) -> Result<Scenario, ScenarioError> {
    match family {
        "technical_prototype_v1" => {
            Scenario::parse_and_validate(v1_ron_with_population(hard, stretch).as_bytes())
        }
        "collision_scene_v1" => Scenario::from_spec(ScenarioSpec {
            hard_agent_count: hard,
            stretch_agent_count: stretch,
            ..collision_scene_spec()
        }),
        "fixture_*" => Scenario::from_spec(ScenarioSpec {
            hard_agent_count: hard,
            stretch_agent_count: stretch,
            ..fixture_spec()
        }),
        other => panic!("unknown family {other:?}"),
    }
}

const SCENARIO_FAMILIES: [&str; 3] = ["technical_prototype_v1", "collision_scene_v1", "fixture_*"];

#[test]
fn population_above_the_ceiling_is_refused() {
    // One over the ceiling is refused, and the error names the ceiling so an
    // author can tell a cap violation from a mistyped locked constant.
    match validate_family_at(
        "collision_scene_v1",
        MAX_LIVE_AGENTS + 1,
        MAX_LIVE_AGENTS + 1,
    ) {
        Err(ScenarioError::InvalidDimension(msg)) => {
            assert!(
                msg.contains("hard_agent_count"),
                "expected the offending field, got {msg:?}"
            );
            assert!(
                msg.contains(&(MAX_LIVE_AGENTS + 1).to_string()),
                "expected the offending value, got {msg:?}"
            );
            assert!(
                msg.contains("MAX_LIVE_AGENTS"),
                "expected the cap to be named, got {msg:?}"
            );
        }
        other => panic!("expected InvalidDimension, got {other:?}"),
    }
    // The ceiling itself is the largest accepted value, not the first refused.
    validate_family_at("collision_scene_v1", MAX_LIVE_AGENTS, MAX_LIVE_AGENTS)
        .expect("the ceiling itself must be accepted");
}

#[test]
fn stretch_above_the_ceiling_is_refused() {
    // Hard stays legal; only the stretch tier breaches. Checked in every
    // family, because a stretch count is what a scene declares when it wants
    // headroom it is not allowed to have.
    for family in SCENARIO_FAMILIES {
        match validate_family_at(family, 64, MAX_LIVE_AGENTS + 1) {
            Err(ScenarioError::InvalidDimension(msg)) => {
                assert!(
                    msg.contains("stretch_agent_count") && msg.contains("MAX_LIVE_AGENTS"),
                    "{family}: expected field + cap in {msg:?}"
                );
            }
            other => panic!("{family}: expected InvalidDimension, got {other:?}"),
        }
    }
}

#[test]
fn the_ceiling_binds_every_scenario_family() {
    // Iterating the families is the guard: the cap must not be something one
    // validator happens to do. A new family that skips it fails here.
    for family in SCENARIO_FAMILIES {
        let err = validate_family_at(family, MAX_LIVE_AGENTS + 1, MAX_LIVE_AGENTS + 1).expect_err(
            &format!("{family} must refuse a population above the ceiling"),
        );
        match err {
            ScenarioError::InvalidDimension(msg) => assert!(
                msg.contains("MAX_LIVE_AGENTS"),
                "{family}: every family must refuse with the same cap error, got {msg:?}"
            ),
            other => panic!("{family}: expected InvalidDimension, got {other:?}"),
        }
    }
}

#[test]
fn a_body_is_half_a_sprite() {
    // The "no art overlap at contact" property, stated as arithmetic: two
    // touching agents are exactly one sprite width apart, so their sprites meet
    // edge to edge instead of eating into each other.
    for rel in [
        "assets/scenarios/technical_prototype_v1.ron",
        COLLISION_MID_SCENE,
        COLLISION_SPRITE_SCENE,
    ] {
        let scene = Scenario::load_verified(scene_path(rel)).expect("tracked scene must load");
        let diameter_px = scene.collision_radius_cells() * scene.cell_size_px() as f32 * 2.0;
        assert!(
            (diameter_px - scene.sprite_size_px() as f32).abs() < f32::EPSILON,
            "{rel}: body diameter {diameter_px} px != sprite {} px",
            scene.sprite_size_px()
        );
    }
}

#[test]
fn the_locked_radius_is_under_the_radius_cap() {
    // The locked v1 body must stay strictly inside the declarable maximum, so a
    // later body bump trips the radius cap instead of silently exceeding it.
    let scene = Scenario::load_verified(scene_path("assets/scenarios/technical_prototype_v1.ron"))
        .expect("v1 scene must load");
    assert!(
        scene.collision_radius_q8() < MAX_COLLISION_RADIUS_Q8,
        "locked radius {} must stay under the cap {MAX_COLLISION_RADIUS_Q8}",
        scene.collision_radius_q8()
    );
}

#[test]
fn every_tracked_scene_is_within_the_ceiling() {
    // Every scene that ships in the repo, not just the ones a validator test
    // happens to construct.
    let mut checked = 0;
    let tracked = ["assets/scenarios/technical_prototype_v1.ron"]
        .into_iter()
        .chain(ALL_COLLISION_SCENES.iter().copied())
        .map(scene_path)
        .chain(ALL_FIXTURES.iter().copied().map(fixture_path));
    for path in tracked {
        let scene = Scenario::load_verified(&path).expect("tracked scene must load");
        assert!(
            scene.hard_agent_count() <= MAX_LIVE_AGENTS
                && scene.stretch_agent_count() <= MAX_LIVE_AGENTS,
            "{}: {} / {} above the ceiling {MAX_LIVE_AGENTS}",
            path.display(),
            scene.hard_agent_count(),
            scene.stretch_agent_count()
        );
        checked += 1;
    }
    assert_eq!(checked, 7, "all seven tracked scenes must be covered");
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

// --- separation headroom knobs (T1) -----------------------------------------
//
// Contract only: three new scenario fields, all pinned to their identity
// value `1`. Nothing reads them yet — these tests exist so a later ticket
// that starts reading them cannot also quietly change what "identity" means.

#[test]
fn tracked_scenes_declare_the_identity_tuning() {
    let mut checked = 0;
    let tracked = ["assets/scenarios/technical_prototype_v1.ron"]
        .into_iter()
        .chain(ALL_COLLISION_SCENES.iter().copied())
        .map(scene_path)
        .chain(ALL_FIXTURES.iter().copied().map(fixture_path));
    for path in tracked {
        let scene = Scenario::load_verified(&path).expect("tracked scene must load");
        // collision_mid_v1 is T3's one deliberate opt-in: it amortises its
        // separation pass over 4 phases. Every other tracked scene, including
        // the rest of the collision family, still pins the identity.
        let expected_phases = if path.ends_with(COLLISION_MID_SCENE) {
            4
        } else {
            1
        };
        assert_eq!(
            scene.separation_phases(),
            expected_phases,
            "{}: separation_phases must stay at its declared tuning",
            path.display()
        );
        // collision_sprite_v1 is T5's one deliberate opt-in: it runs two push
        // priority classes. Every other tracked scene still pins the identity.
        let expected_classes = if path.ends_with(COLLISION_SPRITE_SCENE) {
            2
        } else {
            1
        };
        assert_eq!(
            scene.mass_class_count(),
            expected_classes,
            "{}: mass_class_count must stay at its declared tuning",
            path.display()
        );
        assert_eq!(
            scene.separation_threads(),
            1,
            "{}: separation_threads must stay at identity",
            path.display()
        );
        checked += 1;
    }
    assert_eq!(checked, 7, "all seven tracked scenes must be covered");
}

#[test]
fn zero_separation_phases_is_rejected() {
    let spec = ScenarioSpec {
        separation_phases: 0,
        ..fixture_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidCollision(_)) => {}
        other => panic!("expected InvalidCollision, got {other:?}"),
    }
}

#[test]
fn separation_phases_above_the_cap_is_rejected() {
    let spec = ScenarioSpec {
        separation_phases: 17,
        ..fixture_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidCollision(_)) => {}
        other => panic!("expected InvalidCollision, got {other:?}"),
    }
}

#[test]
fn mass_classes_above_the_cap_is_rejected() {
    let spec = ScenarioSpec {
        mass_class_count: 9,
        ..fixture_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidCollision(_)) => {}
        other => panic!("expected InvalidCollision, got {other:?}"),
    }
}

#[test]
fn separation_threads_above_the_cap_is_rejected() {
    let spec = ScenarioSpec {
        separation_threads: 17,
        ..fixture_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidCollision(_)) => {}
        other => panic!("expected InvalidCollision, got {other:?}"),
    }
}

#[test]
fn a_bodyless_scenario_may_not_tune_separation() {
    let spec = ScenarioSpec {
        collision_radius_q8: 0,
        separation_strength_q8: 0,
        separation_phases: 4,
        ..fixture_spec()
    };
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidCollision(_)) => {}
        other => panic!("expected InvalidCollision, got {other:?}"),
    }
}

#[test]
fn the_gate_scene_pins_the_identity_tuning() {
    let (ron_path, _) = v1_paths();
    let text = fs::read_to_string(&ron_path).expect("read v1 ron");
    let mutated = text.replace("separation_phases: 1,", "separation_phases: 2,");
    assert_ne!(
        text, mutated,
        "separation_phases anchor not found in v1 ron"
    );
    let err =
        Scenario::parse_and_validate(mutated.as_bytes()).expect_err("retuned phases must fail");
    match err {
        ScenarioError::InvalidDimension(_) => {}
        other => panic!("expected InvalidDimension, got {other:?}"),
    }
}

// --- rts_prototype_v1 family (T5) -------------------------------------------
//
// The RTS family carries an optional `rts:` block, present exactly on this
// family. `phase0_scene_bytes_are_unchanged` is the load-bearing test: it
// proves `#[serde(default)]` did not force a regeneration of any tracked
// phase-0 scene.

/// A minimal, valid RTS spec: locked geometry, a free HQ site, one node of
/// each kind, no obstacles (so every cell is reachable by construction).
fn rts_spec() -> ScenarioSpec {
    ScenarioSpec {
        version: RTS_PROTOTYPE_V1.to_string(),
        width: 320,
        height: 320,
        cell_size_px: 4,
        sprite_size_px: 48,
        hard_agent_count: 0,
        stretch_agent_count: 0,
        seed: 1,
        destination: Cell { x: 10, y: 10 },
        spawn_cells: vec![Cell { x: 0, y: 0 }],
        atlas_count: 4,
        direction_count: 8,
        frame_count: 4,
        collision_radius_q8: 1_536,
        separation_strength_q8: 256,
        separation_phases: 1,
        mass_class_count: 1,
        separation_threads: 1,
        obstacle_cells: vec![],
        rts: Some(RtsSpec {
            start_crystal: 300,
            start_gas: 100,
            start_supply_cap: 10,
            hq_cell: Cell { x: 200, y: 200 },
            crystal_nodes: vec![Cell { x: 5, y: 5 }],
            gas_nodes: vec![Cell { x: 6, y: 6 }],
        }),
    }
}

fn expect_invalid_rts(spec: ScenarioSpec, needle: &str) {
    match Scenario::from_spec(spec) {
        Err(ScenarioError::InvalidRts(msg)) => assert!(
            msg.contains(needle),
            "expected InvalidRts containing {needle:?}, got {msg:?}"
        ),
        other => panic!("expected InvalidRts containing {needle:?}, got {other:?}"),
    }
}

fn sha256_hex_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

#[test]
fn rts_baseline_spec_is_valid() {
    // Guards the negative tests below: each mutates exactly one field, so the
    // baseline must pass or they would prove nothing.
    let scene = Scenario::from_spec(rts_spec()).expect("baseline rts spec must validate");
    assert_eq!(scene.version(), RTS_PROTOTYPE_V1);
    assert!(scene.rts().is_some());
}

#[test]
fn rts_scene_loads_verified() {
    let scene = Scenario::load_verified(rts_scene_path()).expect("rts scene must load");
    assert_eq!(scene.version(), RTS_PROTOTYPE_V1);
}

#[test]
fn rts_scene_is_horde_free() {
    let scene = Scenario::load_verified(rts_scene_path()).expect("rts scene must load");
    assert_eq!(scene.hard_agent_count(), 0);
    assert_eq!(scene.stretch_agent_count(), 0);
}

#[test]
fn rts_scene_geometry_is_locked() {
    let scene = Scenario::load_verified(rts_scene_path()).expect("rts scene must load");
    assert_eq!(scene.width(), 320);
    assert_eq!(scene.height(), 320);
    assert_eq!(scene.cell_size_px(), 4);
    assert_eq!(scene.sprite_size_px(), 48);
}

#[test]
fn tracked_rts_scene_remains_320_by_320() {
    let scene = Scenario::load_verified(rts_scene_path()).expect("rts scene must load");
    assert_eq!(scene.width(), 320);
    assert_eq!(scene.height(), 320);
}

#[test]
fn rts_scene_compat_radius_matches_three_cells() {
    let scene = Scenario::load_verified(rts_scene_path()).expect("rts scene must load");
    // Compatibility metadata only: shipping RTS collision reads
    // `UnitKind::body_radius_cells`, not this scenario radius.
    assert_eq!(scene.collision_radius_q8(), 768);
}

#[test]
fn rts_map_edge_cap_is_512() {
    let mut big = rts_spec();
    big.width = 512;
    big.height = 512;
    big.spawn_cells = vec![Cell { x: 0, y: 0 }];
    big.destination = Cell { x: 10, y: 10 };
    big.rts.as_mut().unwrap().hq_cell = Cell { x: 400, y: 400 };
    big.rts.as_mut().unwrap().crystal_nodes = vec![Cell { x: 5, y: 5 }];
    big.rts.as_mut().unwrap().gas_nodes = vec![Cell { x: 6, y: 6 }];
    Scenario::from_spec(big).expect("512x512 rts spec must validate");

    let mut too_wide = rts_spec();
    too_wide.width = 513;
    too_wide.height = 320;
    expect_invalid_dimension(too_wide, "width");
}

#[test]
fn rts_scene_carries_its_block() {
    let scene = Scenario::load_verified(rts_scene_path()).expect("rts scene must load");
    let rts = scene.rts().expect("rts block must be present");
    assert_eq!(rts.start_crystal, 300);
    assert_eq!(rts.start_gas, 100);
    assert_eq!(rts.start_supply_cap, 10);
    assert_eq!(rts.hq_cell, Cell { x: 160, y: 160 });
    assert_eq!(rts.crystal_nodes.len(), 8);
    assert_eq!(rts.gas_nodes.len(), 2);
}

#[test]
fn rts_obstacles_match_the_published_formula() {
    let scene = Scenario::load_verified(rts_scene_path()).expect("rts scene must load");
    let rts = scene.rts().expect("rts block must be present");
    let width = scene.width();
    let height = scene.height();
    let hq_center = (165i64, 165i64);
    let all_nodes: Vec<(i64, i64)> = rts
        .crystal_nodes
        .iter()
        .chain(rts.gas_nodes.iter())
        .map(|c| (c.x as i64, c.y as i64))
        .collect();
    let spawns: std::collections::HashSet<(u32, u32)> =
        scene.spawn_cells().iter().map(|c| (c.x, c.y)).collect();
    let dest = (scene.destination().x, scene.destination().y);

    let chebyshev = |a: (i64, i64), b: (i64, i64)| (a.0 - b.0).abs().max((a.1 - b.1).abs());

    let mut expected = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if (x as i64 * 7 + y as i64 * 13) % 97 != 0 {
                continue;
            }
            if chebyshev((x as i64, y as i64), hq_center) <= 28 {
                continue;
            }
            if all_nodes
                .iter()
                .any(|&n| chebyshev((x as i64, y as i64), n) <= 6)
            {
                continue;
            }
            if spawns.contains(&(x, y)) {
                continue;
            }
            if (x, y) == dest {
                continue;
            }
            expected.push(x + y * width);
        }
    }
    expected.sort_unstable();
    assert_eq!(expected, scene.obstacle_cells());
}

#[test]
fn hq_footprint_is_free_in_the_tracked_scene() {
    let scene = Scenario::load_verified(rts_scene_path()).expect("rts scene must load");
    let rts = scene.rts().expect("rts block must be present");
    let mut checked = 0;
    for dy in 0..HQ_FOOTPRINT_CELLS {
        for dx in 0..HQ_FOOTPRINT_CELLS {
            let x = rts.hq_cell.x + dx;
            let y = rts.hq_cell.y + dy;
            let idx = x + y * scene.width();
            assert!(
                !scene.is_obstacle_index(idx),
                "hq footprint cell ({x}, {y}) must be free"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, HQ_FOOTPRINT_CELLS * HQ_FOOTPRINT_CELLS);
}

#[test]
fn every_phase0_scene_has_no_rts_block() {
    let mut checked = 0;
    let tracked = ["assets/scenarios/technical_prototype_v1.ron"]
        .into_iter()
        .chain(ALL_COLLISION_SCENES.iter().copied())
        .map(scene_path)
        .chain(ALL_FIXTURES.iter().copied().map(fixture_path));
    for path in tracked {
        let scene = Scenario::load_verified(&path).expect("tracked scene must load");
        assert!(
            scene.rts().is_none(),
            "{}: phase-0 scene must carry rts: None",
            path.display()
        );
        checked += 1;
    }
    assert_eq!(
        checked, 7,
        "all seven tracked phase-0 scenes must be covered"
    );
}

#[test]
fn phase0_scene_bytes_are_unchanged() {
    // Proves the `#[serde(default)]` on `rts` did not force a regeneration of
    // any tracked phase-0 `.ron`: every one still matches its committed
    // `.sha256` sidecar byte for byte.
    let mut checked = 0;
    let tracked = ["assets/scenarios/technical_prototype_v1.ron"]
        .into_iter()
        .chain(ALL_COLLISION_SCENES.iter().copied())
        .map(scene_path)
        .chain(ALL_FIXTURES.iter().copied().map(fixture_path));
    for path in tracked {
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let sha_path = path.with_extension("sha256");
        let expected =
            fs::read_to_string(&sha_path).unwrap_or_else(|e| panic!("{}: {e}", sha_path.display()));
        let actual = sha256_hex_of(&bytes);
        assert_eq!(
            actual,
            expected.trim(),
            "{}: bytes drifted from its committed .sha256 sidecar",
            path.display()
        );
        checked += 1;
    }
    assert_eq!(
        checked, 7,
        "all seven tracked phase-0 scenes must be covered"
    );
}

#[test]
fn an_rts_scene_without_a_block_is_rejected() {
    let spec = ScenarioSpec {
        rts: None,
        ..rts_spec()
    };
    expect_invalid_rts(spec, "rts block must be present");
}

#[test]
fn a_phase0_family_with_an_rts_block_is_rejected() {
    let spec = ScenarioSpec {
        rts: Some(RtsSpec {
            start_crystal: 300,
            start_gas: 100,
            start_supply_cap: 10,
            hq_cell: Cell { x: 0, y: 0 },
            crystal_nodes: vec![Cell { x: 4, y: 4 }],
            gas_nodes: vec![Cell { x: 5, y: 5 }],
        }),
        ..fixture_spec()
    };
    expect_invalid_rts(spec, "rts block must be present");
}

#[test]
fn a_nonzero_population_is_rejected_for_the_rts_family() {
    let spec = ScenarioSpec {
        hard_agent_count: 1,
        ..rts_spec()
    };
    expect_invalid_dimension(spec, "hard_agent_count");
}

#[test]
fn a_nonzero_stretch_is_rejected_for_the_rts_family() {
    let spec = ScenarioSpec {
        stretch_agent_count: 1,
        ..rts_spec()
    };
    expect_invalid_dimension(spec, "stretch_agent_count");
}

#[test]
fn an_empty_node_list_is_rejected() {
    let mut base = rts_spec();
    base.rts.as_mut().unwrap().crystal_nodes = vec![];
    expect_invalid_rts(base, "crystal_nodes must not be empty");
}

#[test]
fn a_blocked_node_is_rejected() {
    let mut base = rts_spec();
    let node = base.rts.as_ref().unwrap().crystal_nodes[0];
    base.obstacle_cells = vec![node.x + node.y * base.width];
    expect_invalid_rts(base, "is blocked");
}

#[test]
fn an_unreachable_node_is_rejected() {
    let mut base = rts_spec();
    // Wall a full obstacle ring around the crystal node at (5, 5), sealing it
    // off from the destination.
    let node = base.rts.as_ref().unwrap().crystal_nodes[0];
    let mut ring = Vec::new();
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let x = (node.x as i32 + dx) as u32;
            let y = (node.y as i32 + dy) as u32;
            ring.push(x + y * base.width);
        }
    }
    base.obstacle_cells = ring;
    expect_invalid_rts(base, "unreachable");
}

#[test]
fn a_duplicate_node_across_kinds_is_rejected() {
    let mut base = rts_spec();
    let dup = base.rts.as_ref().unwrap().crystal_nodes[0];
    base.rts.as_mut().unwrap().gas_nodes = vec![dup];
    expect_invalid_rts(base, "appears in both crystal_nodes and gas_nodes");
}

#[test]
fn a_blocked_hq_footprint_is_rejected() {
    let mut base = rts_spec();
    let hq = base.rts.as_ref().unwrap().hq_cell;
    base.obstacle_cells = vec![hq.x + hq.y * base.width];
    expect_invalid_rts(base, "hq footprint cell");
}

#[test]
fn an_out_of_bounds_hq_footprint_is_rejected() {
    let mut base = rts_spec();
    base.rts.as_mut().unwrap().hq_cell = Cell { x: 315, y: 315 };
    expect_invalid_rts(base, "out of bounds");
}

#[test]
fn a_node_inside_the_hq_footprint_is_rejected() {
    let mut base = rts_spec();
    let hq = base.rts.as_ref().unwrap().hq_cell;
    base.rts.as_mut().unwrap().crystal_nodes = vec![Cell {
        x: hq.x + 3,
        y: hq.y + 3,
    }];
    expect_invalid_rts(base, "inside the HQ footprint");
}

#[test]
fn a_spawn_inside_the_hq_footprint_is_rejected() {
    let mut base = rts_spec();
    let hq = base.rts.as_ref().unwrap().hq_cell;
    base.spawn_cells = vec![Cell {
        x: hq.x + 3,
        y: hq.y + 3,
    }];
    expect_invalid_rts(base, "lies inside the HQ footprint");
}

#[test]
fn a_supply_cap_over_the_pillar_is_rejected() {
    let mut base = rts_spec();
    base.rts.as_mut().unwrap().start_supply_cap = MAX_SUPPLY_CAP + 1;
    expect_invalid_rts(base, "start_supply_cap");
}

#[test]
fn the_supply_cap_ceiling_is_an_inclusive_bound() {
    // The cap is the largest *accepted* value, not the first rejected one.
    let mut base = rts_spec();
    base.rts.as_mut().unwrap().start_supply_cap = MAX_SUPPLY_CAP;
    Scenario::from_spec(base).expect("the ceiling itself must be accepted");
}

#[test]
fn a_zero_supply_cap_is_rejected() {
    let mut base = rts_spec();
    base.rts.as_mut().unwrap().start_supply_cap = 0;
    expect_invalid_rts(base, "start_supply_cap");
}

#[test]
fn an_over_generous_start_stock_is_rejected() {
    let mut base = rts_spec();
    base.rts.as_mut().unwrap().start_crystal = MAX_START_RESOURCE + 1;
    expect_invalid_rts(base, "start_crystal");
}

#[test]
fn the_renderer_contract_still_binds_the_rts_family() {
    let spec = ScenarioSpec {
        atlas_count: 5,
        ..rts_spec()
    };
    expect_invalid_dimension(spec, "atlas_count");
}
