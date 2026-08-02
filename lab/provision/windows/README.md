# Windows runner contract (T18)

Frozen host attestation for Windows 11 25H2 x86_64 ref PC. **Not** candidate evidence.

## Separation

| Surface | Schema | Contents |
| --- | --- | --- |
| Runner contract (expected) | `windows-runner-manifest-v1` | Pinned OS/build/HW/driver/BIOS/VBIOS/FFU/power/D3D12 rules |
| Host attestation (observed) | `windows-host-attestation-v1` | Inspected host fields only |
| Candidate evidence | `lab-host-evidence-v1` | Archive hash + raw trials + claimed stats (untrusted) |

Candidate reports never supply attestation truth. Coordinator validates host state against the frozen manifest before recovery/candidate paths (T19/T20).

## Contract fields

Manifest: `lab/manifests/windows-11-25h2-x86_64.toml`

| Field | Meaning |
| --- | --- |
| `platform` / `arch` / `os_*` | Windows 11 25H2 x86_64 |
| `os_build` | Exact build pin; drift → `maintenance-block` |
| `backend` | `d3d12` only |
| `cpu.model_contains` | `8600G` |
| `gpu.model_contains` / `vram_mb` | RX 6400 / 4096 |
| `driver.name` / `version` | Frozen AMD Adrenalin pin |
| `firmware.bios_version` / `vbios_version` | Exact match; drift → quarantine |
| `firmware.secure_boot_required` | Must be enabled |
| `firmware.measured_boot_required` | Must be enabled where supported |
| `ffu.digest_sha256` | Frozen FFU whole-disk digest |
| `power.plan_name` | Exact active power plan |
| `d3d12.allow_basic_render` | `false` — Basic Render Driver / WARP rejected |
| `d3d12.device_name_contains` | Physical RX 6400 adapter |
| `d3d12.reject_substrings` | Software/basic adapter denylist |

Observed attestation JSON mirrors host fields (`cpu_model`, `gpu_model`, `ffu_digest_sha256`, `d3d12_adapter_*`, `os_build`, `power_plan_name`, `secure_boot`, `measured_boot`, …) with **no** trial samples or claimed perf.

## Verdicts

| Verdict | When |
| --- | --- |
| `ready-for-recovery` | All pins match |
| `quarantine` | FFU/BIOS/VBIOS/HW/driver/power/secure/measured-boot drift |
| `maintenance-block` | OS build pin drift (scheduled refresh, not emergency reflash) |
| `reject` | Basic Render Driver / WARP, wrong platform/backend/arch |

## Dry-run

```powershell
# via script
lab/provision/windows/attest.ps1 `
  --dry-run `
  --fixture lab/fixtures/windows-attest/pass.json `
  --lab-bin path/to/mmd-lab

# via CLI
mmd-lab attest-windows `
  --manifest lab/manifests/windows-11-25h2-x86_64.toml `
  --observed lab/fixtures/windows-attest/pass.json
```

Fixtures under `lab/fixtures/windows-attest/`:

- `pass.json` → ready-for-recovery
- `wrong-ffu.json` → quarantine
- `basic-renderer.json` → reject
- `build-drift.json` → maintenance-block

Live host inspect + recovery protocol: see `docs/lab/windows-runner.md` (T19).

## Recovery skeleton (T19)

| Artifact | Role |
| --- | --- |
| `image-manifest.toml` | RO FFU identity + external WinPE/DISM restore policy |
| `recover.ps1` | Dry-run protocol driver → `mmd-lab windows-recover-simulate` |
| `docs/lab/windows-runner.md` | Operator runbook |

```powershell
lab/provision/windows/recover.ps1 --dry-run --lab-bin path/to/mmd-lab
mmd-lab doctor --runner windows   # contracts ok → blocked_user until physical drill
```

Physical WinPE/FFU/power/VLAN drill remains **blocked_user** without ref lab hardware.

## Tests

```bash
cargo test -p mmd-lab windows_manifest
```

Rust fixture matrix covers the same cases as PowerShell dry-run when `pwsh` is unavailable.
