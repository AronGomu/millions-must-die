# Ubuntu runner contract (T15)

Frozen host attestation for Ubuntu 24.04 x86_64 ref PC. **Not** candidate evidence.

## Separation

| Surface | Schema | Contents |
| --- | --- | --- |
| Runner contract (expected) | `ubuntu-runner-manifest-v1` | Pinned OS/HW/driver/BIOS/VBIOS/image/Vulkan rules |
| Host attestation (observed) | `ubuntu-host-attestation-v1` | Inspected host fields only |
| Candidate evidence | `lab-host-evidence-v1` | Archive hash + raw trials + claimed stats (untrusted) |

Candidate reports never supply attestation truth. Coordinator validates host state against the frozen manifest before recovery/candidate paths (T16+).

## Contract fields

Manifest: `lab/manifests/ubuntu-24.04-x86_64.toml`

| Field | Meaning |
| --- | --- |
| `platform` / `arch` / `os_*` | Ubuntu 24.04 LTS x86_64 |
| `backend` | `vulkan` only |
| `cpu.model_contains` | `8600G` |
| `gpu.model_contains` / `vram_mb` | RX 6400 / 4096 |
| `driver.name` / `version` | Frozen amdgpu pin |
| `firmware.bios_version` / `vbios_version` | Exact match; drift → quarantine |
| `firmware.secure_boot_required` | Must be enabled |
| `image.digest_sha256` | Frozen raw image digest |
| `vulkan.allow_software` | `false` — llvmpipe/lavapipe/SwiftShader rejected |
| `vulkan.device_name_contains` | Physical RX 6400 adapter |
| `vulkan.reject_substrings` | Software adapter name denylist |

Observed attestation JSON mirrors the same host fields (`cpu_model`, `gpu_model`, `image_digest_sha256`, `vulkan_device_*`, …) with **no** trial samples or claimed perf.

## Verdicts

| Verdict | When |
| --- | --- |
| `ready-for-recovery` | All pins match |
| `quarantine` | Image/BIOS/VBIOS/HW/driver/secure-boot drift |
| `reject` | Software Vulkan, wrong platform/backend/arch |

## Dry-run

```bash
# via script
lab/provision/ubuntu/attest.sh \
  --dry-run \
  --fixture lab/fixtures/ubuntu-attest/pass.json \
  --lab-bin path/to/mmd-lab

# via CLI
mmd-lab attest-ubuntu \
  --manifest lab/manifests/ubuntu-24.04-x86_64.toml \
  --observed lab/fixtures/ubuntu-attest/pass.json
```

Fixtures under `lab/fixtures/ubuntu-attest/`:

- `pass.json` → ready-for-recovery
- `wrong-image.json` → quarantine
- `software-vulkan.json` → reject
- `vbios-drift.json` → quarantine

Live host inspect lands in T16. No physical candidate run in T15.

## Tests

```bash
cargo test -p mmd-lab ubuntu_manifest
```
