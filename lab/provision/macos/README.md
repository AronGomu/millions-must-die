# macOS runner contract (T21)

Frozen host attestation for macOS 15 arm64 M4 Mac mini 16 GB ref. **Not** candidate evidence.

## Separation

| Surface | Schema | Contents |
| --- | --- | --- |
| Runner contract (expected) | `macos-runner-manifest-v1` | Pinned OS/build/HW/Metal/SSV/security/MDM/EACS rules |
| Host attestation (observed) | `macos-host-attestation-v1` | Inspected host fields only |
| Candidate evidence | `lab-host-evidence-v1` | Archive hash + raw trials + claimed stats (untrusted) |

Candidate reports never supply attestation truth. Coordinator validates host state against the frozen manifest before recovery/candidate paths (T22/T23).

## Contract fields

Manifest: `lab/manifests/macos-15-arm64.toml`

| Field | Meaning |
| --- | --- |
| `platform` / `arch` / `os_*` | macOS 15 arm64 |
| `os_build` | Exact build pin; drift → `maintenance-block` |
| `backend` | `metal` only |
| `hardware.model_contains` | `Mac mini` |
| `hardware.chip_contains` | `M4` — non-M4 → `reject` |
| `hardware.memory_gb` | `16` |
| `metal.require_metal` | Must be available |
| `metal.device_name_contains` | Physical Apple M4 adapter |
| `metal.reject_substrings` | MoltenVK / software denylist |
| `security.security_mode` | `Full Security` |
| `security.ssv_valid_required` | Valid Signed System Volume seal |
| `security.full_security_required` | Full Security mode active |
| `enrollment.ade_required` | Apple Device Enrollment (ADE) |
| `enrollment.mdm_enrolled_required` | MDM profile present |
| `enrollment.mdm_profile_id` | Frozen profile id pin |
| `eacs.preflight_ok_required` | EACS wipe preflight green |
| `eacs.reset_ack_required` | Missed reset ack → quarantine |

Observed attestation JSON mirrors host fields (`hardware_model`, `chip`, `ssv_valid`, `mdm_enrolled`, `eacs_reset_ack`, …) with **no** trial samples or claimed perf.

## Verdicts

| Verdict | When |
| --- | --- |
| `ready-for-recovery` | All pins match |
| `quarantine` | SSV/Full Security/MDM/ADE/EACS/memory drift; missed reset ack |
| `maintenance-block` | OS build pin drift (scheduled refresh) |
| `reject` | Wrong model/chip, missing Metal, software adapter, wrong platform/backend/arch |

## Dry-run

```bash
# via script
lab/provision/macos/attest.sh \
  --dry-run \
  --fixture lab/fixtures/macos-attest/pass.json \
  --lab-bin path/to/mmd-lab

# via CLI
mmd-lab attest-macos \
  --manifest lab/manifests/macos-15-arm64.toml \
  --observed lab/fixtures/macos-attest/pass.json
```

Fixtures under `lab/fixtures/macos-attest/`:

- `pass.json` → ready-for-recovery
- `wrong-model.json` → reject
- `invalid-ssv.json` → quarantine
- `missing-mdm.json` → quarantine

Live host inspect + recovery protocol: see `docs/lab/macos-runner.md`.

## Recovery skeleton (T22)

| Artifact | Role |
| --- | --- |
| `mdm-profile.example.json` | Frozen profile id + restore policy; `TODO(user)` MDM/ABM placeholders |
| `recover.sh` | Dry-run protocol driver → `mmd-lab macos-recover-simulate` |

```bash
lab/provision/macos/recover.sh --dry-run --lab-bin path/to/mmd-lab
lab/provision/macos/recover.sh --dry-run --path dfu --lab-bin path/to/mmd-lab
mmd-lab doctor --runner macos
```

Live path blocked until MDM/ABM selected + M4 + second Mac DFU drill.

## Reset-only vendor endpoints

Recovery may contact **reset/enrollment vendors only**. Candidate VLAN stays blocked.

| Endpoint class | Purpose | Allowed phase |
| --- | --- | --- |
| Apple Business Manager / ADE | Device identity + enrollment assignment | recovery / provisioning |
| MDM provider API | Push enrollment profile, confirm enrollment | recovery / provisioning |
| Apple Push (APNs) via MDM | MDM wake / check-in | recovery / provisioning |
| EACS / Erase All Content and Settings | Remote wipe + preflight / reset ack | recovery only |
| Apple DFU restore (second Mac + cable) | Manual fallback when EACS fails | recovery only (operator) |

**Not allowed** on candidate path: general internet, App Store, iCloud user data, arbitrary Apple CDN, package mirrors, candidate binary egress.

`TODO(user)`: select MDM provider/account before physical drill.

## Tests

```bash
cargo test -p mmd-lab macos_manifest
cargo test -p mmd-lab --test macos_recovery
```
