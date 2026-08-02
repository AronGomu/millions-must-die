# Windows reference runner (recovery + attestation)

Normative: ADR 007, `lab/manifests/windows-11-25h2-x86_64.toml`, plan T18–T20.  
Operator-facing. Candidate code never initiates restore or supplies attestation truth.

## Host contract (T18)

| Artifact | Role |
| --- | --- |
| `lab/manifests/windows-11-25h2-x86_64.toml` | Frozen OS/build/HW/driver/BIOS/VBIOS/FFU/power/D3D12 pins |
| `lab/fixtures/windows-attest/*.json` | Observed host attestation fixtures |
| `lab/provision/windows/attest.ps1` | Dry-run / later live inspect → `mmd-lab attest-windows` |

Verdicts: `ready-for-recovery` | `quarantine` | `maintenance-block` | `reject`.  
Basic Render Driver / WARP → **reject**. FFU/firmware drift → **quarantine**. OS build drift → **maintenance-block**.

## Recovery protocol (T19)

External controller owns power, WinPE boot, DISM FFU apply, VLAN moves, egress canary.  
Candidate OS **never** approves cleanup.

| Artifact | Role |
| --- | --- |
| `lab/provision/windows/image-manifest.toml` | RO FFU identity + restore policy |
| `lab/provision/windows/recover.ps1` | Skeleton + dry-run protocol driver |
| `mmd-lab windows-recover-simulate` | Protocol state machine (unit + dry-run) |
| `mmd-lab doctor --runner windows` | Readiness: contracts present; physical drill status |

### Required physical path

1. Controller powers host into **WinPE** on **recovery** VLAN.
2. **DISM `/Apply-FFU`** full-drive from **read-only** store; controller **readback SHA-256**.
3. DISM fail or digest mismatch → **quarantine** (no provision).
4. Mint **fresh host identity** + unprivileged user; pin exact power plan; stale/reused identity → quarantine.
5. VLAN transition: recovery → **provisioning** → **candidate**.
6. Host attestation against frozen runner manifest (no candidate evidence).
7. External **egress canary** on candidate VLAN must be **denied**.
8. Success → `ReadyForCandidate`. Failure sticky-quarantines until new external restore.

### Dry-run (developer machine, no ref PC)

```bash
cargo build -p mmd-lab

# PowerShell (Windows / pwsh)
lab/provision/windows/recover.ps1 --dry-run --lab-bin target/debug/mmd-lab

# or CLI only
mmd-lab windows-recover-simulate \
  --image-manifest lab/provision/windows/image-manifest.toml \
  --runner-manifest lab/manifests/windows-11-25h2-x86_64.toml \
  --attest-fixture lab/fixtures/windows-attest/pass.json

mmd-lab doctor --runner windows
```

Dry-run proves protocol logic + fixture attestation only. It does **not** satisfy physical validation.

### Live drill (blocked until lab exists)

Exact human action:

1. Provision Windows 11 25H2 ref PC (8600G + RX 6400 matched).
2. Stand up external WinPE/power controller + RO FFU image store.
3. Wire recovery / provisioning / candidate VLANs; candidate egress deny.
4. Capture/verify FFU; freeze digest into image-manifest + runner manifest.
5. Run FFU → attest → FFU drill.
6. Capture external flow logs proving candidate egress deny.
7. `$HOME/.local/bin/mmd-lab doctor --runner windows` reports physical ready.

## Trust boundary

| Trusted | Untrusted |
| --- | --- |
| External controller + RO FFU store | Candidate OS / PR tree |
| Installed `mmd-lab` + out-of-tree digest | Candidate-claimed cleanup |
| Coordinator attestation + protocol SM | Host-reported “I’m clean” |
| External firewall/flow logs | In-guest egress self-test alone |
| DISM apply from WinPE (controller-driven) | In-OS reset / push-button reset alone |

## Protocol unit tests

```bash
cargo test -p mmd-lab windows_recovery
```

Covers: FFU apply failure quarantine, digest mismatch, stale Windows identity, egress canary fail, quarantine persistence, candidate-initiated restore reject, happy-path ready.

## State

Until physical drill completes: ticket **blocked_user**. Protocol/docs/skeleton may land earlier; they do not mark T19 done.
