# Architecture Decision Records

ADRs 001, 005, 006 and 007 are **superseded in part** by T28 (2026-08-05): the
performance acceptance criteria, benchmark gates, platform matrix and the
lab's merge-gate authority are retired to a later optimization phase. Their
non-performance decisions still stand. See
[testing strategy](../05-testing.md#retired-performance-gating).

ADR 003 is **superseded in part** by ADR 009 (2026-08-08): agents no longer pass
freely through each other. Its navigation and movement decisions still stand.

ADR 009 is **supplemented** by ADR 010 and ADR 011 (2026-08-09): the separation
model is unchanged; how often it runs, how it is indexed, how it is weighted
and which threads run it are recorded there.

ADR 004 is **superseded in part** by ADR 012 (2026-08-09): its "no per-frame
depth sort" line no longer holds — isometric depth is ordered by a depth test,
not a sort. The fixed atlas order and the rest of its decisions still stand.

ADR 012 is **superseded in part** by ADR 014 (2026-08-10): its decision (e)
"the camera is fixed, not scrolling" no longer holds — the camera pans and
edge-pans, and `IsoView::with_center_cell` re-derives the depth bias with the
origin so the depth key does not move with it. Zoom, which ADR 012 also listed
as phase-1 work, is deferred. Every other decision in ADR 012 stands.

ADR 003 is **supplemented** by ADR 013 (2026-08-10): the horde simulation is
unchanged and player units still never path per-unit — they descend a pooled
flow field keyed by destination cell.

Phase-1.1 records 016–020 are **accepted** (2026-08-12, T18): the RTS-only
control slice shipped and each record carries an implementation section naming
what landed differently from its own proposal. ADR 017 gives RTS units hard
bodies while ADR 009 remains authoritative for overlap-capable horde `sim/` —
no document may claim hard collision for the horde. ADR 018 **supersedes in
part** ADR 014: its fixed 24-cell/s camera and raw grid-edge clamp are gone,
replaced by split keyboard/edge speeds and a projected-map frontier; ADR 014's
texture-table, depth and UI-layer decisions stand. ADR 019 supersedes ADR 014's
output-only HUD and, with it, phase 1's Escape-quits binding: Escape opens the
pause menu and a script ends with the `quit` token. ADR 013's phase-1 scope
line "no minimap, no menus, no sound" is superseded by 019 and 020 for phase
1.1 only; everything else in ADR 013 stands.

What those records claim is closed against real tests in the
[phase-1.1 functional close](../rts-interaction-ui-audio-hardening-functional-close.md).

Accepted phase-0 decisions:

1. [Technical prototype scope + acceptance](001_ADR_technical_prototype_scope_and_acceptance.md)
2. [Workspace, toolchain + trust boundaries](002_ADR_workspace_toolchain_and_trust_boundaries.md)
3. [Simulation + flow field](003_ADR_simulation_and_flow_field.md)
4. [SDL3 sprite renderer + assets](004_ADR_sdl3_sprite_renderer_and_assets.md)
5. [Benchmark measurement + baselines](005_ADR_benchmark_measurement_and_baselines.md)
6. [Native platforms + reference hardware](006_ADR_native_platform_and_reference_hardware.md)
7. [Local validation lab + security](007_ADR_local_validation_lab_and_security.md)
8. [Open-source governance](008_ADR_open_source_governance.md)
9. [Agent separation + collision](009_ADR_agent_separation_and_collision.md)
10. [Separation amortisation, bin stamping + push priority](010_ADR_separation_amortisation_and_push_priority.md)
11. [Parallel separation + the allocation invariant](011_ADR_parallel_separation_and_the_allocation_invariant.md)
12. [StarCraft-scale entities, hitbox rings + isometric render](012_ADR_starcraft_scale_and_isometric_render.md)

Accepted phase-1 decisions:

13. [Phase-1 scope + the RTS entity model](013_ADR_phase1_scope_and_rts_entity_model.md)
14. [Movable camera, texture table + the UI layer](014_ADR_movable_camera_texture_table_and_ui_layer.md)
15. [Economy, construction + production determinism](015_ADR_economy_construction_and_production_determinism.md)

Accepted phase-1.1 decisions:

16. [Phase-1.1 scope + input geometry](016_ADR_phase1_1_scope_and_input_geometry.md)
17. [RTS hard collision, radius-aware navigation + formations](017_ADR_rts_hard_collision_navigation_and_formations.md)
18. [Settings, window modes, logical canvas + camera frontier](018_ADR_settings_window_canvas_and_camera.md)
19. [HUD, minimap, menu + input routing](019_ADR_hud_minimap_and_input_routing.md)
20. [Audio events, buses, runtime + generated assets](020_ADR_audio_events_buses_and_generated_assets.md)

Accepted feedback-polish decision:

21. [RTS feedback polish + gather-worker collision policy](021_ADR_rts_feedback_polish_and_gather_collision.md)
    — narrows ADR 017's unconditional no-penetration invariant to everything
    except an active gather-worker pair and its bounded exit; ADR 017–020 carry
    forward amendments rather than rewritten decisions.

Accepted phase-2 (combat prototype) decisions:

22. [Combat model + enemy faction](022_ADR_combat_model_and_enemy_faction.md)
    — enemies are RTS entities under `OWNER_ENEMY`, ordinary hard pairs (ADR
    021's gather exception is not widened); instant-hit flat-stat combat.
23. [Combat gate, scale + the one-time re-baseline](023_ADR_combat_gate_scale_and_rebaseline.md)
    — waves-only enemies on the tracked scene, first spawn after every pinned
    window, exit-line combat tokens.

New decision → new ADR. Changed decision → superseding ADR; do not rewrite history silently.
