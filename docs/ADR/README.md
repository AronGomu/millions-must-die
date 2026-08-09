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

New decision → new ADR. Changed decision → superseding ADR; do not rewrite history silently.
