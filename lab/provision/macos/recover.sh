#!/usr/bin/env bash
# macOS ref external recovery skeleton (T22).
# EXTERNAL CONTROLLER / OPERATOR ONLY — never invoke from candidate OS / untrusted tree.
#
# Physical path (blocked until MDM + M4 lab exist):
#   1. TODO(user): select/provision MDM provider + ABM/ADE account
#   2. Controller starts EACS preflight/wipe on recovery VLAN (Apple/APNs/MDM allowlist)
#   3. Await EACS reset ack; miss → quarantine
#   4. ADE/MDM reenroll to frozen profile id
#   5. Mint fresh host identity; move recovery → provisioning → candidate
#   6. attest.sh (host contract: Full Security/SSV/Metal/MDM)
#   7. External egress canary on candidate VLAN (must DENY)
#   8. DFU fallback: second Mac + USB-C when EACS fails; then reenroll+attest
#
# This script is a protocol skeleton + dry-run. It does not wipe or DFU devices.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  recover.sh --dry-run [--mdm-profile <path.json>] [--runner-manifest <path>]
             [--lab-bin <mmd-lab>] [--attest-fixture <json>] [--path eacs|dfu]
  recover.sh --help

Dry-run validates mdm-profile.example shape + runs protocol simulation via mmd-lab
(macos-recover-simulate). No EACS wipe, no DFU, no power control.

Live restore requires MDM/ABM/ADE, M4 Mac mini, second Mac + USB-C, recovery VLANs.
Exit: 0 dry-run protocol ready-for-candidate; 1 quarantine/fail; 2 usage/env.
EOF
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
MDM_PROFILE="${ROOT}/lab/provision/macos/mdm-profile.example.json"
RUNNER_MANIFEST="${ROOT}/lab/manifests/macos-15-arm64.toml"
ATTEST_FIXTURE="${ROOT}/lab/fixtures/macos-attest/pass.json"
LAB_BIN="${MMD_LAB_BIN:-}"
DRY_RUN=0
PATH_MODE="eacs"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --mdm-profile) MDM_PROFILE="${2:-}"; shift 2 ;;
    --runner-manifest) RUNNER_MANIFEST="${2:-}"; shift 2 ;;
    --attest-fixture) ATTEST_FIXTURE="${2:-}"; shift 2 ;;
    --lab-bin) LAB_BIN="${2:-}"; shift 2 ;;
    --path) PATH_MODE="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ "$DRY_RUN" -ne 1 ]]; then
  cat >&2 <<'EOF'
live restore not available on this host.
need:
  1. Select/provision MDM provider + ABM/ADE account (TODO(user))
  2. M4 Mac mini 16 GB + second Mac + USB-C data cable
  3. Recovery/provisioning/candidate VLANs; recovery Apple/APNs/MDM allowlist
  4. Drill EACS + DFU fallback
use: recover.sh --dry-run
EOF
  exit 2
fi

for f in "$MDM_PROFILE" "$RUNNER_MANIFEST" "$ATTEST_FIXTURE"; do
  if [[ ! -f "$f" ]]; then
    echo "missing file: $f" >&2
    exit 2
  fi
done

# mdm-profile must declare external initiator + deny candidate egress + profile id.
if ! grep -q '"initiator": "external-controller"' "$MDM_PROFILE"; then
  echo "mdm-profile: initiator must be external-controller" >&2
  exit 1
fi
if ! grep -q '"candidate_egress_policy": "deny"' "$MDM_PROFILE"; then
  echo "mdm-profile: candidate egress must deny" >&2
  exit 1
fi
if ! grep -q '"profile_id": "mmd-lab-macos-ref"' "$MDM_PROFILE"; then
  echo "mdm-profile: profile_id must match frozen runner pin mmd-lab-macos-ref" >&2
  exit 1
fi
if ! grep -q 'TODO(user)' "$MDM_PROFILE"; then
  echo "mdm-profile: expected TODO(user) MDM/ABM placeholders until provisioned" >&2
  exit 1
fi

case "$PATH_MODE" in
  eacs|dfu) ;;
  *) echo "unknown --path $PATH_MODE (eacs|dfu)" >&2; exit 2 ;;
esac

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

echo "recover: dry-run protocol simulation (no wipe/dfu)"
echo "recover: mdm-profile $MDM_PROFILE"
echo "recover: runner-manifest $RUNNER_MANIFEST"
echo "recover: initiator external-controller"
echo "recover: candidate-egress deny"
echo "recover: path $PATH_MODE"
echo "recover: TODO(user) MDM/ABM selection still required for live drill"

# Protocol state machine + attestation fixture (coordinator-side only).
exec "$LAB_BIN" macos-recover-simulate \
  --mdm-profile "$MDM_PROFILE" \
  --runner-manifest "$RUNNER_MANIFEST" \
  --attest-fixture "$ATTEST_FIXTURE" \
  --path "$PATH_MODE"
