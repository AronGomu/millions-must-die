# Ubuntu reference runner (recovery + attestation)

Normative: ADR 007, `lab/manifests/ubuntu-24.04-x86_64.toml`, plan T15–T17.  
Operator-facing. Candidate code never initiates restore or supplies attestation truth.

## Host contract (T15)

| Artifact | Role |
| --- | --- |
| `lab/manifests/ubuntu-24.04-x86_64.toml` | Frozen OS/HW/driver/BIOS/VBIOS/image/Vulkan pins |
| `lab/fixtures/ubuntu-attest/*.json` | Observed host attestation fixtures |
| `lab/provision/ubuntu/attest.sh` | Dry-run / later live inspect → `mmd-lab attest-ubuntu` |

Verdicts: `ready-for-recovery` | `quarantine` | `reject`.  
Software Vulkan (llvmpipe/lavapipe/…) → **reject**. Image/firmware drift → **quarantine**.

## Recovery protocol (T16)

External controller owns power, PXE, image stream, VLAN moves, egress canary.  
Candidate OS **never** approves cleanup.

| Artifact | Role |
| --- | --- |
| `lab/provision/ubuntu/image-manifest.toml` | RO raw image identity + restore policy |
| `lab/provision/ubuntu/recover.sh` | Skeleton + dry-run protocol driver |
| `mmd-lab ubuntu-recover-simulate` | Protocol state machine (unit + dry-run) |
| `mmd-lab doctor --runner ubuntu` | Readiness: contracts present; physical drill status |

### Required physical path

1. Controller powers host onto **recovery** VLAN / PXE.
2. Stream **read-only** raw whole-disk image; controller **readback SHA-256**.
3. Digest mismatch → **quarantine** (no provision).
4. Mint **fresh SSH host keys** + unprivileged user; stale/reused key → quarantine.
5. VLAN transition: recovery → **provisioning** → **candidate**.
6. Host attestation against frozen runner manifest (no candidate evidence).
7. External **egress canary** on candidate VLAN must be **denied**.
8. Success → `ReadyForCandidate`. Failure sticky-quarantines until new external restore.

### Dry-run (developer machine, no ref PC)

```bash
cargo build -p mmd-lab
lab/provision/ubuntu/recover.sh --dry-run --lab-bin target/debug/mmd-lab

# or
mmd-lab ubuntu-recover-simulate \
  --image-manifest lab/provision/ubuntu/image-manifest.toml \
  --runner-manifest lab/manifests/ubuntu-24.04-x86_64.toml \
  --attest-fixture lab/fixtures/ubuntu-attest/pass.json

mmd-lab doctor --runner ubuntu
```

Dry-run proves protocol logic + fixture attestation only. It does **not** satisfy physical validation.

### Live drill (blocked until lab exists)

Exact human action:

1. Provision Ubuntu 24.04 ref PC (8600G + RX 6400 matched).
2. Stand up external PXE/power controller + RO image store.
3. Wire recovery / provisioning / candidate VLANs; candidate egress deny.
4. Freeze real image digest into image-manifest + runner manifest.
5. Run restore → attest → restore drill.
6. Capture external flow logs proving candidate egress deny.
7. `$HOME/.local/bin/mmd-lab doctor --runner ubuntu` reports physical ready.

## Trust boundary

| Trusted | Untrusted |
| --- | --- |
| External controller + RO image store | Candidate OS / PR tree |
| Installed `mmd-lab` + out-of-tree digest | Candidate-claimed cleanup |
| Coordinator attestation + protocol SM | Host-reported “I’m clean” |
| External firewall/flow logs | In-guest egress self-test alone |

## Protocol unit tests

```bash
cargo test -p mmd-lab ubuntu_recovery
```

Covers: digest mismatch quarantine, stale host key, egress canary fail, quarantine persistence, candidate-initiated restore reject, happy-path ready.

## State

Until physical drill completes: ticket **blocked_user**. Protocol/docs/skeleton may land earlier; they do not mark T16 done.
