# macOS reference runner (recovery + attestation)

Normative: ADR 007, `lab/manifests/macos-15-arm64.toml`, plan T21–T23.  
Operator-facing. Candidate code never initiates restore or supplies attestation truth.

## Host contract (T21)

| Artifact | Role |
| --- | --- |
| `lab/manifests/macos-15-arm64.toml` | Frozen OS/build/HW/Metal/SSV/security/MDM/EACS pins |
| `lab/fixtures/macos-attest/*.json` | Observed host attestation fixtures |
| `lab/provision/macos/attest.sh` | Dry-run / later live inspect → `mmd-lab attest-macos` |

Verdicts: `ready-for-recovery` | `quarantine` | `maintenance-block` | `reject`.  
Non-M4 / software Metal → **reject**. SSV/Full Security/MDM/ADE/missed EACS reset ack → **quarantine**. OS build drift → **maintenance-block**.

## Recovery protocol (T22)

External controller owns EACS, ADE/MDM reenroll, VLAN moves, egress canary.  
Operator owns DFU fallback (second Mac + USB-C). Candidate OS **never** approves cleanup.

| Artifact | Role |
| --- | --- |
| `lab/provision/macos/mdm-profile.example.json` | Frozen profile id + restore policy + TODO(user) MDM/ABM placeholders |
| `lab/provision/macos/recover.sh` | Skeleton + dry-run protocol driver |
| `mmd-lab macos-recover-simulate` | Protocol state machine (unit + dry-run) |
| `mmd-lab doctor --runner macos` | Readiness: contracts present; physical/MDM drill status |

### Required physical path

1. **`TODO(user)`: select/provision MDM provider + Apple Business Manager/ADE account.**
2. Controller starts **EACS** preflight/wipe on **recovery** VLAN (Apple/APNs/MDM allowlist only).
3. **EACS reset ack** required; timeout/miss → **quarantine** (no provision).
4. **ADE/MDM reenroll** to frozen `mdm_profile_id`; fail → quarantine.
5. Mint **fresh host identity**; Full Security + SSV verified at attestation.
6. VLAN transition: recovery → **provisioning** → **candidate**.
7. Host attestation against frozen runner manifest (no candidate evidence).
8. External **egress canary** on candidate VLAN must be **denied**.
9. **DFU fallback**: second Mac + USB-C data cable when EACS fails; then reenroll + attest.
10. Success → `ReadyForCandidate`. Failure sticky-quarantines until new external restore or DFU.

### Dry-run (developer machine, no M4)

```bash
cargo build -p mmd-lab

lab/provision/macos/recover.sh --dry-run --lab-bin target/debug/mmd-lab
lab/provision/macos/recover.sh --dry-run --path dfu --lab-bin target/debug/mmd-lab

# or CLI only
mmd-lab macos-recover-simulate \
  --mdm-profile lab/provision/macos/mdm-profile.example.json \
  --runner-manifest lab/manifests/macos-15-arm64.toml \
  --attest-fixture lab/fixtures/macos-attest/pass.json \
  --path eacs

mmd-lab doctor --runner macos
```

Dry-run proves protocol logic + fixture attestation only. It does **not** satisfy physical validation.

### Live drill (blocked until lab + MDM exist)

Exact human actions:

1. Select/provision MDM provider + ABM/ADE account (clears `TODO(user)`).
2. Provision base M4 Mac mini 16 GB + second Mac + USB-C data cable.
3. Wire recovery / provisioning / candidate VLANs; recovery allowlist Apple/APNs/MDM only; candidate egress deny.
4. Freeze real MDM profile id into runner manifest + mdm-profile (replace example placeholders).
5. Drill EACS → attest → EACS without candidate code.
6. Drill DFU fallback → reenroll → attest.
7. Capture external flow logs proving candidate egress deny.
8. `$HOME/.local/bin/mmd-lab doctor --runner macos` reports physical ready.

## Trust boundary

| Trusted | Untrusted |
| --- | --- |
| External controller + MDM/ABM operator path | Candidate OS / PR tree |
| Installed `mmd-lab` + out-of-tree digest | Candidate-claimed cleanup |
| Coordinator attestation + protocol SM | Host-reported “I’m clean” |
| External firewall/flow logs | In-guest egress self-test alone |
| EACS / DFU from recovery bench | In-OS self-erase alone |
| Recovery-only Apple/APNs/MDM allowlist | General internet / App Store / iCloud |

## Reset-only vendor endpoints

| Endpoint class | Purpose | Allowed phase |
| --- | --- | --- |
| Apple Business Manager / ADE | Device identity + enrollment assignment | recovery / provisioning |
| MDM provider API | Push enrollment profile, confirm enrollment | recovery / provisioning |
| Apple Push (APNs) via MDM | MDM wake / check-in | recovery / provisioning |
| EACS | Remote wipe + preflight / reset ack | recovery only |
| Apple DFU restore (second Mac + cable) | Manual fallback when EACS fails | recovery only (operator) |

**Not allowed** on candidate path: general internet, App Store, iCloud user data, arbitrary Apple CDN, package mirrors, candidate binary egress.

## Protocol unit tests

```bash
cargo test -p mmd-lab macos_recovery
```

Covers: missed EACS ack quarantine, reenroll failure quarantine, candidate egress canary fail, stale identity, quarantine persistence, candidate-initiated restore reject, EACS happy path, DFU fallback after missed ack, SSV attestation quarantine.

## State

Until MDM selection + physical EACS/DFU drills complete: ticket **blocked_user**. Protocol/docs/skeleton may land earlier; they do not mark T22 done.
