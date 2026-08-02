# ADR 006: Native Platforms + Reference Hardware

- Status: Accepted
- Date: 2026-08-02
- Superseded by: —

## Context

Cross-platform compile success does not prove native backend correctness/perf. Stable perf gates need fixed physical HW + pinned software manifests.

## Decision

Support matrix:

| OS | Arch | Backend | Reference HW |
| --- | --- | --- | --- |
| Ubuntu 24.04 LTS | x86_64 | Vulkan | Ryzen 5 8600G, RX 6400 4 GB, 16 GB RAM |
| Windows 11 25H2 | x86_64 | D3D12 | Matched Ryzen 5 8600G, RX 6400 4 GB, 16 GB RAM |
| macOS 15 | arm64 | Metal | Base M4 Mac mini, 16 GB |

Policy:

- Every merge validates all 3 physical refs.
- Same absolute 50k frame limits on every platform.
- Per-platform relative baselines.
- Freeze OS/driver/BIOS/VBIOS/power/cooling manifests.
- Updates occur on scheduled maintenance branch.
- Reviewed evidence + recalibration required.
- Procure matched RX 6400 spare.

No exact Apple↔PC GPU equivalence claim. Each platform proves own threshold.

## Consequences

Positive:

- Real Vulkan/D3D12/Metal evidence.
- Consumer-entry HW target.
- Reproducible long-term regression signal.

Negative:

- HW procurement/ops cost.
- Single host/platform creates queue + availability bottleneck.
- M4 not performance-equivalent to RX 6400.
- RX 6400 age/stock risk.

## Rejected alternatives

- Standard hosted CI: unstable/no guaranteed graphics GPU.
- Cloud T4/M2-only gate: not entry consumer profile.
- One perf host + functional other OSes: weak full-confidence claim.
- Dual-boot PC: serial jobs + reset/reboot coupling.
- ARM Linux/Windows + Intel Mac: excess phase-0 matrix.

## Validation

- Machine manifest captured every run.
- Backend/device/driver assert before workload.
- Thermal/power state preflight.
- Manifest mismatch blocks result.
- 50 clean pilot runs/platform before relative gates.

## References

- `docs/local-validation-lab-architecture.html`
- `docs/ADR/005_ADR_benchmark_measurement_and_baselines.md`
- `.tmp/IMPLEMENTATION_PLAN_technical_prototype.md` T9–T18
