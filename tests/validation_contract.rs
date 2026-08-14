//! T28 validation contract: the documented merge gate carries no perf threshold.
//!
//! Phase-0 acceptance is behavioural, not timed. Performance gating is retired
//! to the later optimization phase (single source of truth:
//! `docs/05-testing.md`). This file is the executable half of that contract —
//! it fails the moment a required-path command starts consuming a frame-time
//! or noise threshold again, or a doc resumes claiming a performance pass.
//!
//! It deliberately does *not* police historical numbers: measured results are
//! retained as history in `docs/technical-prototype-results.md`. Only the
//! documented required-command sections are scanned.

use std::path::PathBuf;
use std::process::Command;

/// Doc that owns the merge gate definition.
const CONTRACT_DOC: &str = "docs/05-testing.md";
/// Doc that mirrors the gate for contributors.
const README_DOC: &str = "README.md";
/// Doc retaining the retired measurements as history.
const RESULTS_DOC: &str = "docs/technical-prototype-results.md";
/// Doc that closes phase 0 on functional evidence (T33).
const CLOSE_DOC: &str = "docs/technical-prototype-functional-close.md";
/// Doc that closes phase 1 on functional evidence (T16).
const PHASE1_CLOSE_DOC: &str = "docs/rts-engine-prototype-functional-close.md";
/// Phase-1 architecture page. A live doc: it describes what shipped.
const PHASE1_ARCH_DOC: &str = "docs/rts-engine-prototype-architecture.html";
/// Doc that closes phase 1.1 on functional evidence (T18).
const PHASE1_1_CLOSE_DOC: &str = "docs/rts-interaction-ui-audio-hardening-functional-close.md";
/// Phase-1.1 architecture page. A live doc: it describes what shipped.
const PHASE1_1_ARCH_DOC: &str = "docs/rts-interaction-ui-audio-hardening-architecture.html";
/// Doc that states what each phase claims (roadmap + vision + MVP scope, consolidated).
const ROADMAP_DOC: &str = "docs/CONTEXT.md";

/// Heading text (any level) introducing the required-command list. Matched
/// case-insensitively and ignoring trailing punctuation.
const GATE_HEADING: &str = "Required merge gate";

/// Commands documented as required must never mention these. Any hit means a
/// timing/noise number is back on the required path.
const PERF_THRESHOLD_TOKENS: &[&str] = &[
    "p95",
    "p99",
    "nmad",
    "16.67",
    "25 ms",
    "frame-time",
    "frame time",
    "percentile",
    "median",
    "throughput",
    "latency",
    "fps",
];

/// Tools whose output is measurement evidence, never a merge verdict. Frozen
/// in place and still built + unit-tested, but not runnable as a gate.
const NON_GATING_COMMANDS: &[&str] = &[
    "bench",
    "mmd-lab",
    "release-freeze",
    "release-check",
    "calibrate",
    "pilot",
    "merge-gate",
];

/// Commands that must stay required — guards against satisfying this contract
/// by gutting the gate instead of retiring the perf part of it. Compared after
/// whitespace normalization, so reflowing the docs is not a failure.
const REQUIRED_COMMANDS: &[&str] = &[
    "cargo fmt --all -- --check",
    "cargo test --workspace --locked",
    "cargo clippy --workspace --all-targets --all-features -- -D warnings",
    "nix flake check",
];

/// The deterministic audio-asset check phase 1.1 added to the gate (T18). Its
/// own test, because the generated WAVs are the only tracked binary content a
/// contributor can regenerate by accident: dropping the check from the gate
/// would let drifted audio bytes merge unreviewed.
const AUDIO_GATE_COMMAND: &str = "cargo run -p xtask -- audio --check";

/// Offline DCO range gate (audit F11 / issue #10). Tokens TRUSTED_BASE_SHA and
/// EXACT_CANDIDATE_SHA are literal placeholders in the doc fence; maintainer
/// substitutes real SHAs at run time. No angle-bracket argv tokens (shell-safe).
const DCO_GATE_COMMAND: &str = "./scripts/check-dco TRUSTED_BASE_SHA EXACT_CANDIDATE_SHA";

/// The interactive smokes, exactly as documented. The RTS one carries its
/// frame budget and its tracked script inline: a smoke silently shortened to
/// 160 frames, or pointed at an untracked script, still exits 0 while proving
/// far less than the close doc claims.
const PHASE_SMOKE_COMMANDS: &[&str] = &[
    "cargo run -- run --agents 5000 --frames 300",
    "cargo run -- run --scenario assets/scenarios/collision_mid_v1.ron --frames 300",
    "cargo run -- run --scenario assets/scenarios/collision_sprite_v1.ron --frames 300",
    "cargo run -- rts --frames 1600 --inject-input-file assets/scenarios/rts_acceptance_v1.script",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_doc(rel: &str) -> String {
    read_repo_file(rel)
}

/// Read any tracked text file — a doc or a Rust source — relative to the repo
/// root. `read_doc` is the doc-flavoured name the T28 tests already use.
fn read_repo_file(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A markdown line tagged with whether it sits inside a fenced code block.
///
/// Fence state matters: a shell comment (`# note`) inside a ```` ``` ```` block
/// must never be mistaken for a markdown heading, or section extraction would
/// silently truncate the command list and let a perf gate slip back in unseen.
struct Line<'a> {
    text: &'a str,
    fenced: bool,
    /// The ```` ``` ```` delimiter itself, which is neither heading nor command.
    delimiter: bool,
}

fn scan(doc: &str) -> Vec<Line<'_>> {
    let mut fenced = false;
    doc.lines()
        .map(|text| {
            let delimiter = text.trim_start().starts_with("```");
            if delimiter {
                fenced = !fenced;
                Line {
                    text,
                    fenced: true,
                    delimiter,
                }
            } else {
                Line {
                    text,
                    fenced,
                    delimiter,
                }
            }
        })
        .collect()
}

/// Heading level of a markdown line, or `None` when it is not a heading.
/// Callers must only apply this to non-fenced lines.
fn heading_level(line: &str) -> Option<usize> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (hashes > 0 && line[hashes..].starts_with(' ')).then_some(hashes)
}

/// Compare heading text tolerantly: case- and trailing-punctuation-insensitive.
fn heading_matches(line: &str, heading: &str) -> bool {
    let Some(level) = heading_level(line) else {
        return false;
    };
    let text = line[level..].trim().trim_end_matches([':', '.']);
    text.eq_ignore_ascii_case(heading)
}

/// Lines of the section whose heading text is `heading`, up to the next
/// non-fenced heading of the same or higher level. `None` when absent.
fn section<'a>(lines: &'a [Line<'a>], heading: &str) -> Option<&'a [Line<'a>]> {
    let start = lines
        .iter()
        .position(|l| !l.fenced && heading_matches(l.text, heading))?;
    let level = heading_level(lines[start].text).expect("matched line is a heading");
    let rest = &lines[start + 1..];
    let end = rest
        .iter()
        .position(|l| !l.fenced && heading_level(l.text).is_some_and(|found| found <= level))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Every non-blank line inside fenced blocks of `body`, comments included —
/// an annotation reintroducing a threshold must be scanned too.
fn fenced_lines<'a>(body: &'a [Line<'a>]) -> Vec<&'a str> {
    body.iter()
        .filter(|l| l.fenced && !l.delimiter && !l.text.trim().is_empty())
        .map(|l| l.text.trim())
        .collect()
}

/// Collapse whitespace and drop a trailing `#` comment, so that reflowing or
/// annotating a documented command is not mistaken for dropping it.
fn normalize_command(raw: &str) -> String {
    let code = match raw.find(" #") {
        Some(idx) => &raw[..idx],
        None => raw,
    };
    code.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The parsed required-merge-gate section of one doc.
struct GateSection {
    /// Every non-blank fenced line, shell comments included.
    fenced: Vec<String>,
    /// Executable commands only, whitespace-normalized.
    commands: Vec<String>,
    /// Everything outside the fences, lowercased.
    prose: String,
}

fn gate_section(rel: &str) -> GateSection {
    let doc = read_doc(rel);
    let lines = scan(&doc);
    let body = section(&lines, GATE_HEADING)
        .unwrap_or_else(|| panic!("{rel} must document a '{GATE_HEADING}' section"));

    let fenced: Vec<String> = fenced_lines(body).iter().map(|l| l.to_string()).collect();
    let commands: Vec<String> = fenced
        .iter()
        .filter(|l| !l.starts_with('#'))
        .map(|l| normalize_command(l))
        .filter(|c| !c.is_empty())
        .collect();
    let prose = body
        .iter()
        .filter(|l| !l.fenced)
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();

    assert!(
        commands.len() >= REQUIRED_COMMANDS.len(),
        "{rel} '{GATE_HEADING}' lists {} commands, expected at least {}. \
         (If a command is present but unlisted here, check it is inside the \
         fenced block and not commented out.)",
        commands.len(),
        REQUIRED_COMMANDS.len()
    );
    for required in REQUIRED_COMMANDS {
        assert!(
            commands.iter().any(|c| c == required),
            "{rel} '{GATE_HEADING}' no longer lists required command `{required}` \
             (compared after whitespace normalization); retiring perf gating \
             must not gut the behavioural gate"
        );
    }
    GateSection {
        fenced,
        commands,
        prose,
    }
}

#[test]
fn gate_list_has_no_perf_thresholds() {
    for rel in [CONTRACT_DOC, README_DOC] {
        let gate = gate_section(rel);

        // Scan raw fenced lines (comments included) for threshold numbers...
        for line in &gate.fenced {
            let lower = line.to_ascii_lowercase();
            for token in PERF_THRESHOLD_TOKENS {
                assert!(
                    !lower.contains(token),
                    "{rel}: required-gate line `{line}` states perf threshold `{token}`; \
                     performance gating is retired to the optimization phase"
                );
            }
        }

        // ...and the executable commands for frozen, non-gating tooling.
        for command in &gate.commands {
            let lower = command.to_ascii_lowercase();
            for tool in NON_GATING_COMMANDS {
                assert!(
                    !lower.contains(tool),
                    "{rel}: required command `{command}` invokes non-gating perf tool `{tool}`; \
                     it is frozen developer tooling, not a merge gate"
                );
            }
        }

        // Prose in the gate section must not reintroduce a threshold either.
        for token in PERF_THRESHOLD_TOKENS {
            assert!(
                !gate.prose.contains(token),
                "{rel}: '{GATE_HEADING}' prose states perf threshold `{token}`"
            );
        }
    }
}

#[test]
fn gate_docs_state_perf_gating_is_retired() {
    // Negative assertions alone pass vacuously on an empty doc; require the
    // positive statement too.
    let contract = read_doc(CONTRACT_DOC).to_ascii_lowercase();
    assert!(
        contract.contains("optimization phase"),
        "{CONTRACT_DOC} must point perf work at the optimization phase"
    );
    assert!(
        contract.contains("not a gate"),
        "{CONTRACT_DOC} must label the frozen perf tooling 'not a gate'"
    );

    // Every doc that states what gates a merge points at the one source of
    // truth instead of restating a deferral note — or its own gate list — of
    // its own. CONTRIBUTING.md is included because it tells contributors what
    // the maintainer runs before merging.
    // Paths come from the consts above — a doc rename must break in one place, not two.
    for rel in [README_DOC, ROADMAP_DOC, RESULTS_DOC, "CONTRIBUTING.md"] {
        let doc = read_doc(rel);
        assert!(
            doc.contains("05-testing.md"),
            "{rel} must link the single validation-contract source of truth ({CONTRACT_DOC})"
        );
    }
}

#[test]
fn results_doc_is_superseded_history_not_a_claim() {
    let doc = read_doc(RESULTS_DOC);
    assert!(
        doc.to_ascii_uppercase().contains("SUPERSEDED"),
        "{RESULTS_DOC} must be marked superseded"
    );
    // History is retained, not deleted: the measured curve stays readable.
    assert!(
        doc.contains("1.670"),
        "{RESULTS_DOC} must retain the measured numbers as history"
    );
    for banned in [
        "gate open",
        "rerun required",
        "one quiet-host rerun remains",
    ] {
        assert!(
            !doc.to_ascii_lowercase().contains(banned),
            "{RESULTS_DOC} still presents `{banned}` as live phase-0 work; \
             the perf gate is retired, not pending"
        );
    }
}

// ---------------------------------------------------------------------------
// T33 phase close: coverage map + live-doc perf-claim scan.
//
// These two complement `gate_list_has_no_perf_thresholds`, they do not repeat
// it. That test parses one section (`Required merge gate`) of two docs and asks
// whether a *command* consumes a threshold. The two below ask different
// questions: does every system phase 0 claims to prove actually own a named,
// existing test, and does any live doc make a bare performance claim anywhere
// on the page — inside or outside the gate section.
// ---------------------------------------------------------------------------

/// One phase-0 system and the named tests that prove it.
///
/// This list — not any document — is the source of truth for what phase 0
/// claims (decision D11). `every_system_has_a_test` resolves it in both
/// directions: against a source scan of the test tree, and against the close
/// doc's published map. A doc that agrees with itself proves nothing, so the
/// doc is never consulted for what the systems *are*.
struct SystemCoverage {
    /// The close document publishing this system's map. Phase 0 and phase 1
    /// close on separate pages, and a phase-1 system named in the phase-0
    /// close would rewrite a record that is already closed.
    close_doc: &'static str,
    /// Human-readable system name; must appear verbatim in the close doc.
    system: &'static str,
    /// Repo-relative file that must contain every test named below.
    file: &'static str,
    /// `#[test]` fns proving the system behaves.
    tests: &'static [&'static str],
    /// Subset of `tests` that is `#[ignore]`d and therefore does **not** run
    /// on a plain `cargo test`. Declaring one here is the only way to map it,
    /// and the close doc must label it too. The list is checked in both
    /// directions — an `#[ignore]`d test missing from it fails, and a name
    /// here that is *not* actually `#[ignore]`d fails — so it can be neither
    /// forgotten nor padded to silence the rule.
    gpu_only: &'static [&'static str],
}

const SCOPE_SYSTEMS: &[SystemCoverage] = &[
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Simulation — movement, obstacles, recycling",
        file: "crates/mmd-engine/tests/simulation.rs",
        tests: &[
            "tick_moves_eight_cells_per_second",
            "blocked_step_holds_position",
            "obstacles_are_never_entered",
            "no_agent_is_stuck_against_an_obstacle",
            "arrival_radius_recycles",
            "population_stays_at_the_cap",
            "determinism_holds_at_the_cap",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Collision — agent separation and neighbour bins",
        file: "crates/mmd-engine/tests/separation.rs",
        tests: &[
            "spatial_bins_hold_every_agent_exactly_once",
            "spatial_bucket_order_is_ascending_agent_index",
            "spatial_clamps_positions_outside_the_world",
            "separation_of_a_pair_is_equal_and_opposite",
            "separation_is_capped_at_eight_neighbours",
            "coincident_agents_separate_on_the_first_tick",
            "separation_keeps_the_step_length",
            "a_released_stack_spreads_apart",
            "a_bodyless_scenario_walks_the_flow_only_path",
            "sprite_scene_pulls_agents_out_of_deep_overlap",
            "collision_scene_agents_never_enter_an_obstacle",
            "spatial_bin_counts_match_the_bucket_lengths",
            "spatial_reuses_bins_without_clearing_them",
            "spatial_survives_a_stamp_wrap",
            "separation_phases_of_one_is_the_identity",
            "an_amortised_agent_keeps_its_repulsion_between_phases",
            "the_grid_rebuilds_once_per_phase_cycle",
            "an_amortised_stack_still_spreads",
            "bin_row_matches_bin_by_bin_order",
            "bin_row_clamps_to_the_last_column",
            "bin_row_is_empty_off_the_grid",
            "a_lone_agent_accumulates_no_repulsion",
            "one_mass_class_leaves_every_agent_equal",
            "mass_is_assigned_round_robin_by_index",
            "a_heavier_neighbour_pushes_a_lighter_one_harder",
            "mass_classes_change_the_bodied_digest",
            "threads_do_not_change_the_walk",
            "threads_do_not_change_an_amortised_walk",
            "a_single_thread_spawns_no_workers",
            "a_pool_spawns_one_fewer_worker_than_participants",
            "the_pool_shuts_down_cleanly",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Navigation — flow field",
        file: "crates/mmd-engine/tests/flow_field.rs",
        tests: &[
            "destination_cost_is_zero",
            "obstacles_unreachable",
            "diagonal_cannot_cut_corner",
            "vectors_descend",
            "every_reachable_cell_has_a_valid_direction",
            "agent_in_an_unreachable_region_is_inert_not_panicking",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Scenario loading and hash contract",
        file: "crates/mmd-engine/tests/scenario_contract.rs",
        tests: &[
            "loads_v1_scene",
            "rejects_wrong_hash",
            "rejects_unreachable_spawn",
            "v1_geometry_stays_frozen_against_the_fixture_relaxation",
            "tracked_scenes_declare_the_identity_tuning",
            "zero_separation_phases_is_rejected",
            "separation_phases_above_the_cap_is_rejected",
            "mass_classes_above_the_cap_is_rejected",
            "separation_threads_above_the_cap_is_rejected",
            "a_bodyless_scenario_may_not_tune_separation",
            "the_gate_scene_pins_the_identity_tuning",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Deterministic test harness",
        file: "crates/mmd-engine/tests/harness.rs",
        tests: &[
            "same_seed_same_state_hash",
            "different_seed_differs",
            "tick_count_is_exact",
            "no_wall_clock_dependence",
            "fixture_scenarios_are_hash_verified",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Runtime frame loop",
        file: "crates/mmd-engine/tests/runtime_frame.rs",
        tests: &[
            "frame_ticks_once",
            "pause_keeps_checksum",
            "builds_one_instance_per_agent",
            "partitions_four_groups",
            "input_actions_are_stable",
            "toggling_hitboxes_changes_only_the_rings",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Render correctness — instance data, projection, whole frame",
        file: "crates/mmd-engine/tests/render_correctness.rs",
        tests: &[
            "instance_per_alive_agent",
            "instances_carry_agent_position_and_animation_uvs",
            "packing_is_a_pure_projection_of_sim_state",
            "world_to_clip_transform",
            "world_to_clip_matches_gpu_raster",
            "golden_frame_matches",
            "no_gpu_skips_cleanly",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Debug hitbox overlay",
        file: "crates/mmd-engine/tests/render_correctness.rs",
        tests: &[
            "a_ring_is_packed_for_every_agent",
            "a_bodyless_scene_packs_no_rings",
            "the_ring_traces_the_real_body",
            "the_ring_traces_a_body_that_is_not_half_a_sprite",
            "every_tracked_manifest_pins_the_live_shader_and_atlas",
            "a_ring_is_hollow",
            "a_ring_and_a_sprite_share_a_pass",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Isometric projection and depth order",
        file: "crates/mmd-engine/tests/render_correctness.rs",
        tests: &[
            "ring_and_sprite_stand_on_the_same_ground_point",
            "shader_defines_match_their_rust_mirrors",
            "depth_is_the_ground_point_not_the_quad",
            "offscreen_quads_are_culled",
            "an_agent_in_front_occludes_one_behind",
            "rings_are_never_occluded",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "GPU smoke and tracked asset hashes",
        file: "crates/mmd-engine/tests/gpu_smoke.rs",
        tests: &[
            "instance_layout_is_stable",
            "tracked_atlas_hashes_are_enforced",
            "readback_is_1920x1080",
            "four_groups_drawn",
        ],
        gpu_only: &["readback_is_1920x1080", "four_groups_drawn"],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Golden-image comparator",
        file: "crates/mmd-engine/tests/gpu_golden.rs",
        tests: &[
            "exact_image_passes",
            "delta_above_tolerance_fails",
            "backend_cannot_use_other_golden",
            "placeholder_golden_cannot_pass",
            "tampered_golden_image_fails_hash",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Allocation invariant",
        file: "crates/mmd-engine/tests/frame_allocations.rs",
        tests: &[
            "alloc_invariant_still_enforced",
            "warmup_allocation_passes",
            "guard_resets_between_trials",
            "panic_restores_guard",
            "foreign_thread_allocations_do_not_leak_into_a_measure_scope",
            "spatial_rebuild_allocates_nothing",
            "a_collision_tick_allocates_nothing",
            "a_threaded_collision_tick_allocates_nothing",
            "ring_packing_allocates_nothing",
            "iso_packing_allocates_nothing",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "App and CLI lifecycle",
        file: "tests/cli_contract.rs",
        tests: &[
            "run_exits_after_n_frames",
            "windowed_run_honours_the_frame_budget",
            "pause_freezes_state",
            "overlay_toggle_is_inert",
            "quit_exits_clean_and_releases_window",
            "injection_that_never_fires_is_an_error",
            "missing_scenario_fails_clean",
            "hitbox_toggle_is_scriptable",
        ],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: CLOSE_DOC,
        system: "Merge-gate contract",
        file: "tests/validation_contract.rs",
        tests: &[
            "gate_list_has_no_perf_thresholds",
            "gate_docs_state_perf_gating_is_retired",
            "results_doc_is_superseded_history_not_a_claim",
            "bench_binary_still_builds",
            "every_system_has_a_test",
            "no_perf_claim_in_docs",
            "perf_claim_scanner_catches_what_it_is_meant_to",
        ],
        gpu_only: &[],
    },
    // ---- Phase 1: the six systems the RTS prototype claims (T16). Each maps
    // one representative test here; the rest of each system's evidence is
    // published by the phase-1 close doc's table and resolved by
    // `phase1_close_doc_names_only_real_tests`.
    SystemCoverage {
        close_doc: PHASE1_CLOSE_DOC,
        system: "camera",
        file: "crates/mmd-engine/tests/camera.rs",
        tests: &["cell_at_returns_the_cell_a_sprite_was_packed_from"],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: PHASE1_CLOSE_DOC,
        system: "selection",
        file: "crates/mmd-engine/tests/rts_selection.rs",
        tests: &["box_selects_every_own_unit_inside"],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: PHASE1_CLOSE_DOC,
        system: "workers",
        file: "crates/mmd-engine/tests/rts_economy.rs",
        tests: &["a_full_round_trip_banks_crystal"],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: PHASE1_CLOSE_DOC,
        system: "economy",
        file: "crates/mmd-engine/tests/rts_economy.rs",
        tests: &["the_node_loses_exactly_the_carried_amount"],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: PHASE1_CLOSE_DOC,
        system: "building",
        file: "crates/mmd-engine/tests/rts_build.rs",
        tests: &["a_depot_finishes_in_its_documented_time"],
        gpu_only: &[],
    },
    SystemCoverage {
        close_doc: PHASE1_CLOSE_DOC,
        system: "unit production",
        file: "crates/mmd-engine/tests/rts_production.rs",
        tests: &["queueing_cannot_exceed_the_cap"],
        gpu_only: &[],
    },
];

/// Floor on the total `#[test]` fns discovered across the mapped files. A scan
/// that silently stopped finding tests would otherwise satisfy every
/// membership check vacuously — every lookup would fail loudly, but a *broken
/// regex* that matched everything would not. This pins the scanner itself.
const MIN_SCANNED_TESTS: usize = 555;

/// Number of systems phases 0 and 1 claim together. Pinned so that deleting a
/// `SystemCoverage` entry — which shrinks the claim without breaking any
/// lookup — fails loudly.
const SCOPE_SYSTEM_COUNT: usize = 20;

/// One `#[test]` fn found by the source scan.
#[derive(Debug)]
struct DeclaredTest {
    name: String,
    /// `#[ignore]` anywhere in the same attribute block. An ignored test still
    /// counts as *declared* — that keeps the map honest about where a test
    /// lives — but the rules below stop it passing as everyday proof.
    ignored: bool,
}

/// True when an attribute line marks the item `#[ignore]`d, including the
/// conditional `#[cfg_attr(<cond>, ignore)]` form.
fn attribute_ignores(trimmed: &str) -> bool {
    trimmed.starts_with("#[ignore")
        || (trimmed.starts_with("#[cfg_attr(")
            && (trimmed.contains(", ignore)") || trimmed.contains(",ignore)")))
}

/// `#[test]`-annotated fns declared in one source file, in order.
///
/// The scan accumulates a whole attribute block and only decides when it
/// reaches the signature, because `#[test]`, `#[ignore]`, `#[should_panic]`
/// and doc comments may appear in any order. Anything that is neither an
/// attribute, a comment, nor blank ends the block — otherwise a signature the
/// parser failed to recognise would leak its `#[ignore]` onto the *next* test,
/// which is exactly how a live test gets silently recorded as ignored.
fn declared_tests(rel: &str) -> Vec<DeclaredTest> {
    let src = read_repo_file(rel);
    let mut found = Vec::new();
    let mut is_test = false;
    let mut ignored = false;
    for line in src.lines() {
        let trimmed = line.trim_start();

        if trimmed.starts_with("#[") || trimmed.starts_with("#!") {
            is_test |= trimmed.starts_with("#[test]");
            ignored |= attribute_ignores(trimmed);
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }

        // A signature, possibly behind visibility/async/unsafe qualifiers.
        let mut rest = trimmed;
        for prefix in ["pub(crate) ", "pub(super) ", "pub ", "async ", "unsafe "] {
            rest = rest.strip_prefix(prefix).unwrap_or(rest);
        }
        if let Some(sig) = rest.strip_prefix("fn ")
            && is_test
        {
            let name: String = sig
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                found.push(DeclaredTest { name, ignored });
            }
        }
        // Any non-attribute, non-comment line closes the block.
        is_test = false;
        ignored = false;
    }
    found
}

#[test]
fn every_system_has_a_test() {
    assert!(
        !SCOPE_SYSTEMS.is_empty(),
        "the scope list must name at least one system"
    );

    assert_eq!(
        SCOPE_SYSTEMS.len(),
        SCOPE_SYSTEM_COUNT,
        "the scope list changed size. Adding a system is fine — bump the \
         constant. *Removing* one shrinks what a closed phase claims to prove \
         and must be a deliberate, reviewed decision, not a quiet edit."
    );

    let mut scanned_total = 0usize;

    for entry in SCOPE_SYSTEMS {
        let close_doc = entry.close_doc;
        let close = read_doc(close_doc);
        assert!(
            !entry.tests.is_empty(),
            "system `{}` claims scope but names no test",
            entry.system
        );

        let declared = declared_tests(entry.file);
        scanned_total += declared.len();
        assert!(
            !declared.is_empty(),
            "{} declares no #[test] fn at all; system `{}` has no proof",
            entry.file,
            entry.system
        );

        let mut live = 0usize;
        for name in entry.tests {
            let found = declared.iter().find(|d| d.name == **name);
            let Some(found) = found else {
                let names: Vec<&str> = declared.iter().map(|d| d.name.as_str()).collect();
                panic!(
                    "system `{}` maps to `{name}`, which does not exist in {}. \
                     Found there: {names:?}. A renamed, moved or deleted test \
                     must fail the close, not silently shrink the claim.",
                    entry.system, entry.file
                );
            };

            // An `#[ignore]`d test does not run on a plain `cargo test`, so it
            // may only be mapped when the map says so and the doc labels it.
            // Without this, a system stayed "proven" while half its evidence
            // never executed, because a live sibling satisfied `live > 0`.
            assert_eq!(
                found.ignored,
                entry.gpu_only.contains(name),
                "`{name}` ({}) is {} but the map {} it under `gpu_only`. \
                 Ignored tests must be declared and labelled, never mapped as \
                 everyday proof.",
                entry.file,
                if found.ignored {
                    "#[ignore]d"
                } else {
                    "not #[ignore]d"
                },
                if entry.gpu_only.contains(name) {
                    "lists"
                } else {
                    "omits"
                }
            );
            if found.ignored {
                assert!(
                    close.contains(&format!("`{name}` (GPU-only)")),
                    "{close_doc} lists `{name}` without the `(GPU-only)` label; \
                     a reader must not mistake it for evidence the gate runs"
                );
            } else {
                live += 1;
            }
        }
        assert!(
            live > 0,
            "every test mapped to system `{}` is #[ignore]d; an ignored test \
             proves nothing on the merge gate",
            entry.system
        );

        // The doc must publish the same map. The const list stays the source of
        // truth — the doc is checked against it, never the reverse.
        assert!(
            close.contains(entry.system),
            "{close_doc} does not name system `{}` in its system -> test map",
            entry.system
        );
        assert!(
            close.contains(entry.file),
            "{close_doc} does not name `{}` as the file proving `{}`",
            entry.file,
            entry.system
        );
        for name in entry.tests {
            assert!(
                close.contains(name),
                "{close_doc} omits `{name}`, which system `{}` relies on",
                entry.system
            );
        }
    }

    assert!(
        scanned_total >= MIN_SCANNED_TESTS,
        "source scan found only {scanned_total} #[test] fns across the mapped \
         files (expected >= {MIN_SCANNED_TESTS}); the scanner is broken, so \
         every membership check above is worthless"
    );

    // Reverse direction: the doc must not advertise a test that no longer
    // exists. Every snake_case identifier the close doc quotes in backticks is
    // resolved against the scan. Mapped names need no exemption — the loop
    // above already proved each one exists in the file it claims.
    let corpus: Vec<String> = SCOPE_SYSTEMS
        .iter()
        .flat_map(|e| declared_tests(e.file))
        .map(|d| d.name)
        .collect();
    let close = read_doc(CLOSE_DOC);
    for quoted in backtick_spans(&close) {
        if !looks_like_a_test_name(&quoted) {
            continue;
        }
        assert!(
            corpus.contains(&quoted),
            "{CLOSE_DOC} advertises test `{quoted}`, which no mapped file \
             declares; the close doc must not outlive its evidence"
        );
    }
}

/// Contents of every `` `backtick` `` span in the **prose** of `doc`.
///
/// Fenced blocks are skipped via the shared [`scan`] helper: a ```` ``` ````
/// opener is itself three backticks, so scanning the raw string would shift
/// every pair after it and start quoting code as if it were prose.
fn backtick_spans(doc: &str) -> Vec<String> {
    let prose: String = scan(doc)
        .iter()
        .filter(|l| !l.fenced && !l.delimiter)
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n");
    backtick_spans_in(&prose)
}

fn backtick_spans_in(doc: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = doc;
    while let Some(open) = rest.find('`') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('`') else { break };
        out.push(rest[..close].to_string());
        rest = &rest[close + 1..];
    }
    out
}

/// Heuristic: a bare snake_case word long enough to be a test fn name. Paths,
/// commands, flags, types and env vars are excluded so the reverse check reads
/// only what it can actually resolve.
fn looks_like_a_test_name(s: &str) -> bool {
    s.len() >= 8
        && s.contains('_')
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

// ---------------------------------------------------------------------------
// T16 phase-1 close: the close doc's own table, the ADR index, and doc links.
// ---------------------------------------------------------------------------

/// Header cells identifying the phase-1 close doc's coverage table.
const PHASE1_TABLE_HEADER: [&str; 3] = ["System", "Test binary", "Named tests"];

/// Floor on the rows of that table: the six phase-1 systems, plus the
/// supporting subsystems the close doc also publishes. A table that shrank to
/// six rows would satisfy every per-row check while quietly dropping the
/// evidence for navigation, packing, the HUD, the CLI and the acceptance run.
const PHASE1_MIN_ROWS: usize = 18;

/// Floor on the total test names the table maps, set at what it maps today.
/// Adding evidence is free; removing it has to be a deliberate edit here.
const PHASE1_MIN_MAPPED_TESTS: usize = 178;

/// Cells of a markdown table row, without the leading/trailing empties.
fn table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix('|')
        .and_then(|s| s.strip_suffix('|'))
        .unwrap_or(trimmed);
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

/// `(system, test binary, named tests)` rows of a close doc's coverage map.
///
/// Parsed rather than duplicated in Rust: the point of the tests below is that
/// the *published* table is resolvable, so re-typing it here would only prove
/// the copy agrees with itself. `rel` names the doc for the failure messages —
/// phases 1 and 1.1 publish the same table shape on separate pages.
fn close_doc_rows(rel: &str, doc: &str) -> Vec<(String, String, Vec<String>)> {
    let lines = scan(doc);
    let header = lines
        .iter()
        .position(|l| {
            !l.fenced && {
                let cells = table_cells(l.text);
                cells.len() == 3 && cells.iter().zip(PHASE1_TABLE_HEADER).all(|(c, h)| c == h)
            }
        })
        .unwrap_or_else(|| {
            panic!(
                "{rel} must publish a `{}` table",
                PHASE1_TABLE_HEADER.join(" | ")
            )
        });

    let mut rows = Vec::new();
    // +2 skips the header and the `| --- |` separator beneath it.
    for line in lines[header + 2..]
        .iter()
        .take_while(|l| !l.fenced && l.text.trim_start().starts_with('|'))
    {
        let cells = table_cells(line.text);
        assert_eq!(
            cells.len(),
            3,
            "{rel}: coverage row has {} cells, expected 3:\n  {}",
            cells.len(),
            line.text
        );
        let file = backtick_spans_in(&cells[1]);
        assert_eq!(
            file.len(),
            1,
            "{rel}: row `{}` must name exactly one test binary in backticks",
            cells[0]
        );
        rows.push((
            cells[0].clone(),
            file[0].clone(),
            backtick_spans_in(&cells[2]),
        ));
    }
    rows
}

/// Resolve one close doc's published coverage table against the source tree.
///
/// A close doc's `System -> Test binary -> Named tests` table is a claim about
/// the repo, and a claim nothing checks rots within a ticket. This parses the
/// table out of the markdown, resolves every name against a scan of `#[test]`
/// functions in the named file, and then runs the reverse check: prose may not
/// advertise a test no mapped file declares.
fn resolve_close_doc(rel: &str, min_rows: usize, min_mapped: usize) {
    let doc = read_doc(rel);
    let rows = close_doc_rows(rel, &doc);

    assert!(
        rows.len() >= min_rows,
        "{rel} maps {} systems, expected at least {min_rows}; \
         the phase closed on more evidence than that, and the map may not \
         shrink without the claim shrinking with it",
        rows.len()
    );

    let mut mapped = 0usize;
    let mut corpus: Vec<String> = Vec::new();
    for (system, file, names) in &rows {
        assert!(
            repo_root().join(file).is_file(),
            "{rel}: system `{system}` names `{file}`, which is not a file"
        );
        let declared = declared_tests(file);
        assert!(
            !declared.is_empty(),
            "{file} declares no #[test] fn at all; system `{system}` has no proof"
        );
        assert!(!names.is_empty(), "{rel}: system `{system}` names no test");
        for name in names {
            let found = declared.iter().find(|d| d.name == *name);
            let Some(found) = found else {
                let all: Vec<&str> = declared.iter().map(|d| d.name.as_str()).collect();
                panic!(
                    "{rel}: system `{system}` maps to `{name}`, which \
                     does not exist in {file}. Found there: {all:?}. A renamed, \
                     moved or deleted test must fail the close, not silently \
                     shrink the claim."
                );
            };
            assert!(
                !found.ignored,
                "{rel}: `{name}` ({file}) is #[ignore]d, so it does \
                 not run on a plain `cargo test`; this map takes no ignored test"
            );
            mapped += 1;
        }
        corpus.extend(declared.into_iter().map(|d| d.name));
    }

    assert!(
        mapped >= min_mapped,
        "{rel} maps {mapped} tests, expected at least {min_mapped}; \
         the table thinned out instead of the claim"
    );

    // Reverse direction, as for phase 0: prose must not advertise a test that
    // no binary in the table declares.
    for quoted in backtick_spans(&doc) {
        if !looks_like_a_test_name(&quoted) {
            continue;
        }
        assert!(
            corpus.contains(&quoted),
            "{rel} advertises test `{quoted}`, which no binary in \
             its own map declares; the close doc must not outlive its evidence"
        );
    }
}

/// Every test the phase-1 close document maps to a system must exist.
#[test]
fn phase1_close_doc_names_only_real_tests() {
    resolve_close_doc(PHASE1_CLOSE_DOC, PHASE1_MIN_ROWS, PHASE1_MIN_MAPPED_TESTS);
}

// ---------------------------------------------------------------------------
// T18 phase-1.1 close: the hardening slice's own coverage map and system list.
// ---------------------------------------------------------------------------

/// Floor on the rows of the phase-1.1 coverage table.
const PHASE1_1_MIN_ROWS: usize = 14;

/// Floor on the total test names that table maps, set at what it maps today.
const PHASE1_1_MIN_MAPPED_TESTS: usize = 120;

/// The systems phase 1.1 claims, and one representative test each.
///
/// As with `SCOPE_SYSTEMS`, this list — not the document — is the source of
/// truth for what the phase claims. The close doc is checked against it.
/// Unlike phase 0's list, the files here include app-crate modules: half of
/// phase 1.1 is app-side (settings, window, menu, audio), and mapping only the
/// engine binaries would leave that half unclaimed.
const PHASE1_1_SYSTEMS: &[(&str, &str, &str)] = &[
    (
        "pick geometry",
        "crates/mmd-engine/tests/rts_selection.rs",
        "unit_pick_is_sprite_rect_union_body_circle",
    ),
    (
        "hard bodies",
        "crates/mmd-engine/tests/rts_collision.rs",
        "head_on_units_never_penetrate",
    ),
    (
        "static navigation",
        "crates/mmd-engine/tests/rts_radius_nav.rs",
        "body_clears_static_rectangles",
    ),
    (
        "formations",
        "crates/mmd-engine/tests/rts_formation.rs",
        "formation_slots_are_six_cells_apart",
    ),
    (
        "settings",
        "src/rts_settings.rs",
        "malformed_file_warns_and_uses_defaults",
    ),
    (
        "logical canvas",
        "crates/mmd-engine/tests/display_viewport.rs",
        "round_trip_inverts_render_transform_pillarbox_and_letterbox",
    ),
    (
        "window modes",
        "src/rts_window.rs",
        "failed_mode_change_rolls_back_before_reclaim",
    ),
    (
        "camera frontier",
        "crates/mmd-engine/tests/camera.rs",
        "camera_cannot_cross_any_frontier_edge",
    ),
    (
        "HUD",
        "crates/mmd-engine/tests/rts_hud.rs",
        "hud_regions_cover_bottom_without_overlap",
    ),
    (
        "minimap",
        "crates/mmd-engine/tests/rts_minimap.rs",
        "minimap_projection_round_trips_map_corners",
    ),
    (
        "audio",
        "src/rts_feedback.rs",
        "selection_batch_is_sorted_and_capped",
    ),
];

/// Every test the phase-1.1 close document maps to a system must exist.
#[test]
fn phase1_1_close_names_only_real_tests() {
    resolve_close_doc(
        PHASE1_1_CLOSE_DOC,
        PHASE1_1_MIN_ROWS,
        PHASE1_1_MIN_MAPPED_TESTS,
    );
}

/// Every system phase 1.1 claims owns a live behavioural test, and the close
/// doc names both the system and the file that proves it.
///
/// `phase1_1_close_names_only_real_tests` resolves whatever the doc happens to
/// publish; this one resolves what the phase *claims*, so a system quietly
/// dropped from the page fails instead of passing by omission.
#[test]
fn phase1_1_systems_have_behavioral_tests() {
    let close = read_doc(PHASE1_1_CLOSE_DOC);
    assert_eq!(
        PHASE1_1_SYSTEMS.len(),
        11,
        "phase 1.1 claims 11 systems; adding one is fine — bump this number. \
         Removing one shrinks a closed phase's claim and must be deliberate."
    );

    for (system, file, representative) in PHASE1_1_SYSTEMS {
        let declared = declared_tests(file);
        let found = declared
            .iter()
            .find(|d| d.name == *representative)
            .unwrap_or_else(|| {
                let all: Vec<&str> = declared.iter().map(|d| d.name.as_str()).collect();
                panic!(
                    "system `{system}` maps to `{representative}`, which does not \
                     exist in {file}. Found there: {all:?}"
                )
            });
        assert!(
            !found.ignored,
            "`{representative}` ({file}) is #[ignore]d; an ignored test proves \
             nothing about system `{system}` on the merge gate"
        );
        assert!(
            close.contains(system),
            "{PHASE1_1_CLOSE_DOC} does not name system `{system}` in its map"
        );
        assert!(
            close.contains(file),
            "{PHASE1_1_CLOSE_DOC} does not name `{file}` as the file proving `{system}`"
        );
        assert!(
            close.contains(representative),
            "{PHASE1_1_CLOSE_DOC} omits `{representative}`, which system `{system}` relies on"
        );
    }
}

/// The generated-audio check is on the required gate, in both mirrors.
#[test]
fn required_gate_contains_audio_check() {
    for rel in [CONTRACT_DOC, README_DOC] {
        let gate = gate_section(rel);
        assert!(
            gate.commands.iter().any(|c| c == AUDIO_GATE_COMMAND),
            "{rel} '{GATE_HEADING}' does not list `{AUDIO_GATE_COMMAND}`; the \
             tracked audio bytes would then be the only generated assets no \
             gate regenerates and compares"
        );
    }
}

/// DCO range check stays on the required gate in both doc mirrors.
#[test]
fn required_gate_contains_dco_check() {
    for rel in [CONTRACT_DOC, README_DOC] {
        let gate = gate_section(rel);
        assert!(
            gate.commands.iter().any(|c| c == DCO_GATE_COMMAND),
            "{rel} '{GATE_HEADING}' does not list `{DCO_GATE_COMMAND}`; \
             unsigned candidate commits could pass every other gate command"
        );
    }
}

/// The interactive smokes stay on the gate, verbatim — same scenes, same
/// frame budgets, same tracked RTS script.
#[test]
fn required_gate_keeps_phase_smokes() {
    for rel in [CONTRACT_DOC, README_DOC] {
        let gate = gate_section(rel);
        for smoke in PHASE_SMOKE_COMMANDS {
            assert!(
                gate.commands.iter().any(|c| c == smoke),
                "{rel} '{GATE_HEADING}' no longer lists smoke `{smoke}` \
                 (compared after whitespace normalization)"
            );
        }
    }
}

/// Docs that make live claims. History is deliberately excluded: the retired
/// measurements in `RESULTS_DOC` and the superseded ADRs must stay readable and
/// keep their numbers — `results_doc_is_superseded_history_not_a_claim` is what
/// guards those, by requiring the SUPERSEDED marker and the retained figures.
/// Adding either here would force deleting history to go green.
/// `CONTRIBUTING.md` is included for the same reason
/// `gate_docs_state_perf_gating_is_retired` includes it: it tells contributors
/// what the maintainer runs before merging, so a perf claim there is as live as
/// one in the README.
const LIVE_DOCS: &[&str] = &[
    README_DOC,
    ROADMAP_DOC,
    CONTRACT_DOC,
    CLOSE_DOC,
    PHASE1_CLOSE_DOC,
    PHASE1_ARCH_DOC,
    PHASE1_1_CLOSE_DOC,
    PHASE1_1_ARCH_DOC,
    "CONTRIBUTING.md",
];

/// Extra tokens layered on top of [`PERF_THRESHOLD_TOKENS`]. Anything the
/// gate-section scan already treats as a threshold is a live claim out here
/// too, so this test can never end up weaker than its T28 sibling on a token
/// they share — that is why the two lists are composed rather than copied.
///
/// Deliberately excludes bare `ms`, which appears inside ordinary words;
/// millisecond figures are caught by `states_a_duration` instead.
const EXTRA_PERF_CLAIM_TOKENS: &[&str] = &[
    "frames per second",
    "frames/s",
    "per second",
    "hertz",
    " hz",
    "millisecond",
    "microsecond",
    "nanosecond",
    "real-time",
    "realtime",
    "faster",
    "slower",
    "benchmark result",
];

fn perf_claim_tokens() -> Vec<&'static str> {
    PERF_THRESHOLD_TOKENS
        .iter()
        .chain(EXTRA_PERF_CLAIM_TOKENS)
        .copied()
        .collect()
}

/// Phrases that mark a line as retired, negated, or explicitly unproven. A line
/// carrying a perf token needs one of these or it reads as a live claim.
///
/// Every entry must be readable *only* as "this number decides nothing", and
/// each was checked against a counter-example before being admitted:
///
/// - bare negators (`never`, `not`, `no`) were rejected — "the renderer never
///   drops below 60 fps" is a claim, not a retirement;
/// - `deferred` was rejected — "frame time is 1.6 ms; cross-platform work is
///   deferred" states a live number and defers something else;
/// - `history` / `historical` were rejected for the same reason
///   ("historically and today the renderer holds 60 fps"), and cost nothing,
///   because the history docs are excluded from `LIVE_DOCS` outright;
/// - `frozen` was rejected — it describes the *code*, not the number.
const RETIREMENT_MARKERS: &[&str] = &[
    "retired",
    "retirement",
    "superseded",
    "not a gate",
    "gates nothing",
    "unmeasured",
    "no longer",
    "says nothing",
    "claims nothing",
    "claiming nothing",
    "not proven",
    "no performance claim",
];

/// True when the line quotes a duration in milliseconds (`16.67 ms`, `0.9ms`).
fn states_a_duration(line: &str) -> bool {
    let mut idx = 0;
    while let Some(hit) = line[idx..].find("ms") {
        let at = idx + hit;
        // Preceding non-space character must be a digit for this to be a
        // measurement rather than a word ending in "ms" (systems, forms, ...).
        let before = line[..at].trim_end();
        if before.as_bytes().last().is_some_and(u8::is_ascii_digit)
            // ...and "ms" must end the word, so `hashes`/`msg` do not match.
            && line[at + 2..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric())
        {
            return true;
        }
        idx = at + 2;
    }
    false
}

/// The perf token a line states, or `None`. Case-insensitive.
fn perf_token_in(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    perf_claim_tokens()
        .into_iter()
        .find(|t| lower.contains(t))
        .map(str::to_string)
        .or_else(|| states_a_duration(&lower).then(|| "millisecond figure".to_string()))
}

/// True when the line states a performance number and nothing on it retires,
/// negates, or disclaims that number.
///
/// Split out so it can be unit-tested against fixtures directly — the doc scan
/// alone would go quiet the moment a marker got too broad, and a silently
/// permissive scanner is indistinguishable from clean docs.
fn line_states_a_live_perf_claim(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    perf_token_in(line).is_some() && !RETIREMENT_MARKERS.iter().any(|m| lower.contains(m))
}

#[test]
fn perf_claim_scanner_catches_what_it_is_meant_to() {
    // Live claims — every one of these must be caught, or the doc scan is
    // decorative. Each was an actual escape during T33 development.
    for claim in [
        "The 50k scene sustains p95 under 16.67 ms on the dev host.",
        "Median frame cost is 1.6 ms.",
        "The renderer never drops below 60 fps.",
        "Frame time is 1.6 ms; cross-platform work is deferred.",
        "Historically and today the renderer holds 60 fps.",
        "The frozen runner reports 1.670 ms a frame.",
        "A frame took 1.67 milliseconds.",
        "The renderer sustains 60 Hz with 50000 agents.",
        "The 50k scene runs at 300 frames/s on the dev host.",
        "The 50k scene is comfortably real-time.",
        "Throughput held at 50k agents.",
        "Rendering is faster than the simulation.",
    ] {
        assert!(
            line_states_a_live_perf_claim(claim),
            "live perf claim slipped through the scanner: {claim:?}"
        );
    }

    // Retired references — these must pass, or the docs cannot describe their
    // own history and the test becomes a reason to delete the record.
    for retired in [
        "| 50k frame-time gate (median p95 <= 16.67 ms) | frozen, **not a gate** |",
        "It says nothing about throughput.",
        "Frame-time gating is retired to the optimization phase.",
        "Performance is unmeasured; the 16.67 ms budget gates nothing.",
        "The superseded results doc records p99 figures.",
    ] {
        assert!(
            !line_states_a_live_perf_claim(retired),
            "retired reference wrongly flagged as a live claim: {retired:?}"
        );
    }

    // Ordinary prose must not trip the millisecond detector.
    for innocent in [
        "The simulation and rendering systems are deterministic.",
        "Forms, hashes and msgs are unaffected.",
        "Every system has a test.",
    ] {
        assert!(
            !line_states_a_live_perf_claim(innocent),
            "ordinary prose flagged as a perf claim: {innocent:?}"
        );
    }
}

#[test]
fn no_perf_claim_in_docs() {
    // Every doc is scanned whole, fences included: a perf claim written into a
    // shell comment is still a claim a reader will believe.
    let mut token_lines: Vec<(&str, usize)> = Vec::new();

    for rel in LIVE_DOCS {
        let doc = read_doc(rel);
        for (n, line) in doc.lines().enumerate() {
            let Some(token) = perf_token_in(line) else {
                continue;
            };
            token_lines.push((rel, n + 1));
            assert!(
                !line_states_a_live_perf_claim(line),
                "{rel}:{}: states `{token}` with nothing on the line marking it \
                 retired or unmeasured:\n  {line}\nPhase 0 proves behaviour, \
                 not speed — a live doc may reference a measurement only while \
                 saying it no longer claims anything.",
                n + 1
            );
        }
    }

    // Anti-vacuity, per doc rather than in total. A single global floor is
    // satisfied by one paragraph, so rewriting that paragraph fails the guard
    // instead of the real check — which teaches the next editor to lower the
    // constant. Requiring each doc that *should* discuss the retirement to
    // actually discuss it keeps the failure pointed at the right file.
    for (rel, least) in [(CONTRACT_DOC, 2usize), (README_DOC, 1)] {
        let seen = token_lines.iter().filter(|(d, _)| *d == rel).count();
        assert!(
            seen >= least,
            "{rel} carries {seen} line(s) naming a perf measurement, expected \
             at least {least}. The scan is passing because there is nothing \
             left to scan — state the retirement in prose, or this test stops \
             proving anything about the docs."
        );
    }
}

/// Every ADR file is listed by the ADR index, and every listed link resolves.
///
/// The index is how a reader finds a decision at all. An ADR that exists but
/// is unlisted is a decision nobody will read; a listed link that resolves to
/// nothing is worse, because it looks like the decision was checked.
#[test]
fn adr_index_lists_every_adr_file() {
    let dir = repo_root().join("docs/ADR");
    let index_rel = "docs/ADR/README.md";
    let index = read_doc(index_rel);

    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|e| {
            e.expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|n| n.ends_with(".md") && n != "README.md")
        .collect();
    files.sort();

    assert!(
        files.len() >= 20,
        "docs/ADR holds {} records, expected at least 20 (001-020, phase 1.1 \
         included); the scan is broken",
        files.len()
    );

    for file in &files {
        assert!(
            index.contains(&format!("]({file})")),
            "{index_rel} does not link `{file}`; an unlisted ADR is a decision \
             nobody will find"
        );
    }

    // ...and nothing listed is missing. `every_doc_link_resolves` covers this
    // repo-wide, but the index is the one place a dead ADR link is invisible.
    for target in markdown_link_targets(&index) {
        let path = dir.join(&target);
        assert!(
            path.exists(),
            "{index_rel} links `{target}`, which does not exist"
        );
    }
}

/// Relative link targets of a markdown document, anchors stripped.
///
/// External schemes and bare fragments are skipped: this test resolves files,
/// and a network fetch would make the merge gate depend on the internet.
fn markdown_link_targets(doc: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = doc;
    while let Some(open) = rest.find("](") {
        rest = &rest[open + 2..];
        let Some(close) = rest.find(')') else { break };
        let target = &rest[..close];
        rest = &rest[close + 1..];
        if target.is_empty()
            || target.contains(char::is_whitespace)
            || target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("mailto:")
            || target.starts_with('#')
        {
            continue;
        }
        let path = target.split('#').next().unwrap_or(target);
        if !path.is_empty() {
            out.push(path.to_string());
        }
    }
    out
}

/// Markdown files under `docs/`, recursively, repo-relative.
fn docs_markdown_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![repo_root().join("docs")];
    while let Some(dir) = stack.pop() {
        for entry in
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Every relative markdown link under `docs/` points at a file that exists.
///
/// Docs are the phase record. A link that 404s locally is how a record quietly
/// stops being one — the reader assumes the target was deleted deliberately
/// rather than that the link rotted.
#[test]
fn every_doc_link_resolves() {
    let files = docs_markdown_files();
    assert!(
        files.len() >= 8,
        "found {} markdown files under docs/, expected at least 8; the walk is \
         broken and every check below is vacuous",
        files.len()
    );

    let mut checked = 0usize;
    for file in &files {
        let doc = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("read {file:?}: {e}"));
        let dir = file.parent().expect("doc has a parent directory");
        for target in markdown_link_targets(&doc) {
            let resolved = dir.join(&target);
            assert!(
                resolved.exists(),
                "{}: link `{target}` resolves to {}, which does not exist",
                file.display(),
                resolved.display()
            );
            checked += 1;
        }
    }

    assert!(
        checked >= 100,
        "only {checked} relative links found under docs/, expected at least 100; \
         the link scanner stopped finding links"
    );
}

#[test]
fn bench_binary_still_builds() {
    // Freeze-in-place proof: the bench module is not deleted, so the app binary
    // still links it and exposes the subcommand.
    let output = Command::new(env!("CARGO_BIN_EXE_millions_must_die"))
        .args(["bench", "--help"])
        .output()
        .expect("run app bench --help");
    assert!(
        output.status.success(),
        "bench --help exit nonzero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.to_ascii_lowercase().contains("not a gate"),
        "bench must be labelled non-gating developer tooling:\n{stdout}"
    );
}
