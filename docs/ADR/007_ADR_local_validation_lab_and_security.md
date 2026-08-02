# ADR 007: Local Validation Lab + Security

- Status: Accepted
- Date: 2026-08-02
- Superseded by: —

## Context

Public PR code can run arbitrary build/runtime code. Owner requires local-only tests; no online CI. Physical perf needs bare metal. Manual review cannot guarantee safety.

## Decision

Control plane:

- Installed trusted Rust `mmd-lab` coordinator at `$HOME/.local/bin/mmd-lab`.
- Out-of-tree trusted digest manifest; `self-check` required before dispatch.
- Operational cmds never use candidate/workspace `cargo run -p mmd-lab`.
- Content-addressed source archive over SSH.
- Remote SHA-256 verification before execution.
- Candidate never supplies coordinator/reset code.
- All 3 hosts required; exact hash bound to reports.
- Candidate report treated as untrusted input. Coordinator recomputes stats, image policy, thresholds from collected raw evidence.
- Host/source identity may be attested. Candidate behavior/perf is coordinator-verified evidence, not cryptographically attested truth.

Network/trust:

- Dedicated no-secret hosts.
- External recovery controller + read-only image source.
- Recovery, provisioning, candidate networks separated.
- Candidate egress blocked by external network controls.
- Unprivileged candidate account; fresh host identity.
- External append-only evidence collection where available.

Reset:

- Ubuntu: PXE/recovery env + raw whole-disk restore.
- Windows: WinPE + DISM FFU restore.
- macOS: EACS preflight/wipe + ADE/MDM reenroll; DFU fallback.
- Mac recovery may access allowlisted Apple/APNs/MDM endpoints. Candidate run may not.

Residual risk:

- Disk reset cannot clear every UEFI/GPU/SSD/NIC/controller firmware component.
- Attest secure boot/firmware state.
- Drift/reset failure → quarantine + reflash/replace.
- Bounded residual risk accepted; perfect containment not claimed.

## Consequences

Positive:

- No online CI dependency.
- Exact local consumer-HW evidence.
- Persistent OS-level compromise strongly reduced.

Negative:

- Recovery infra may exceed engine effort.
- macOS needs ABM/ADE, MDM, Apple connectivity during reset.
- Firmware/current-run report forgery risk remains.
- Restore time extends every merge.

## Rejected alternatives

- Normal owner machines after review: unsafe.
- Local FS snapshots: candidate admin can tamper.
- Disposable users only: does not reset kernel/boot/services.
- Public hosted CI: rejected product constraint.
- Claiming whole-disk reset removes firmware risk: false.

## Validation

- Fake-agent protocol TDD before real hosts.
- Full reset→attest→run→reset drill per OS.
- External flow logs verify candidate egress deny.
- EACS failure quarantine + manual DFU drill.
- Mixed hash/missing host/manifest drift block merge.

## References

- `docs/local-validation-lab-architecture.html`
- `docs/ADR/006_ADR_native_platform_and_reference_hardware.md`
- `.tmp/IMPLEMENTATION_PLAN_technical_prototype.md` T13–T17
