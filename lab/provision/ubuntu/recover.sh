#!/usr/bin/env bash
# Ubuntu ref external recovery skeleton (T16).
# EXTERNAL CONTROLLER ONLY — never invoke from candidate OS / untrusted tree.
#
# Physical path (blocked until lab exists):
#   1. Power/PXE host onto recovery VLAN
#   2. Stream RO raw image; readback SHA-256
#   3. Mint fresh SSH host keys + unprivileged user
#   4. Move recovery → provisioning → candidate VLANs
#   5. attest.sh (host contract)
#   6. External egress canary on candidate VLAN (must DENY)
#   7. Mark ready OR quarantine
#
# This script is a protocol skeleton + dry-run. It does not flash disks.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  recover.sh --dry-run [--image-manifest <path>] [--runner-manifest <path>]
             [--lab-bin <mmd-lab>] [--attest-fixture <json>]
  recover.sh --help

Dry-run validates image-manifest shape + runs protocol simulation via mmd-lab
(ubuntu-recover-simulate). No PXE, no disk write, no power control.

Live restore requires external controller, RO image store, VLANs, Ubuntu ref PC.
Exit: 0 dry-run protocol ready-for-candidate; 1 quarantine/fail; 2 usage/env.
EOF
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
IMAGE_MANIFEST="${ROOT}/lab/provision/ubuntu/image-manifest.toml"
RUNNER_MANIFEST="${ROOT}/lab/manifests/ubuntu-24.04-x86_64.toml"
ATTEST_FIXTURE="${ROOT}/lab/fixtures/ubuntu-attest/pass.json"
LAB_BIN="${MMD_LAB_BIN:-}"
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --image-manifest) IMAGE_MANIFEST="${2:-}"; shift 2 ;;
    --runner-manifest) RUNNER_MANIFEST="${2:-}"; shift 2 ;;
    --attest-fixture) ATTEST_FIXTURE="${2:-}"; shift 2 ;;
    --lab-bin) LAB_BIN="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ "$DRY_RUN" -ne 1 ]]; then
  cat >&2 <<'EOF'
live restore not available on this host.
need: Ubuntu 24.04 ref PC + external PXE/power controller + RO image store + recovery/provisioning/candidate VLANs
use: recover.sh --dry-run
EOF
  exit 2
fi

for f in "$IMAGE_MANIFEST" "$RUNNER_MANIFEST" "$ATTEST_FIXTURE"; do
  if [[ ! -f "$f" ]]; then
    echo "missing file: $f" >&2
    exit 2
  fi
done

# image-manifest must declare external initiator + RO image.
if ! grep -q 'initiator = "external-controller"' "$IMAGE_MANIFEST"; then
  echo "image-manifest: initiator must be external-controller" >&2
  exit 1
fi
if ! grep -q 'read_only = true' "$IMAGE_MANIFEST"; then
  echo "image-manifest: image must be read_only" >&2
  exit 1
fi
if ! grep -q 'candidate_egress_policy = "deny"' "$IMAGE_MANIFEST"; then
  echo "image-manifest: candidate egress must deny" >&2
  exit 1
fi

if [[ -z "$LAB_BIN" ]]; then
  if command -v mmd-lab >/dev/null 2>&1; then
    LAB_BIN="$(command -v mmd-lab)"
  elif [[ -x "${HOME}/.local/bin/mmd-lab" ]]; then
    LAB_BIN="${HOME}/.local/bin/mmd-lab"
  else
    echo "mmd-lab not found; set --lab-bin or MMD_LAB_BIN" >&2
    exit 2
  fi
fi

echo "recover: dry-run protocol simulation (no disk write)"
echo "recover: image-manifest $IMAGE_MANIFEST"
echo "recover: runner-manifest $RUNNER_MANIFEST"
echo "recover: initiator external-controller"
echo "recover: candidate-egress deny"

# Protocol state machine + attestation fixture (coordinator-side only).
exec "$LAB_BIN" ubuntu-recover-simulate \
  --image-manifest "$IMAGE_MANIFEST" \
  --runner-manifest "$RUNNER_MANIFEST" \
  --attest-fixture "$ATTEST_FIXTURE"
