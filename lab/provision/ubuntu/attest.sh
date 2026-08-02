#!/usr/bin/env bash
# Ubuntu ref host inspect + contract check (dry-run via fixtures; live inspect later).
# Host attestation only — never candidate evidence / trial samples.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  attest.sh --dry-run --fixture <path.json> [--manifest <path.toml>] [--lab-bin <mmd-lab>]
  attest.sh --help

Dry-run loads observed attestation JSON fixture and validates against frozen
Ubuntu runner manifest via trusted mmd-lab (or cargo bin under test).

Exit: 0 ready-for-recovery; 1 quarantine|reject; 2 usage/tool error.
EOF
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
MANIFEST="${ROOT}/lab/manifests/ubuntu-24.04-x86_64.toml"
LAB_BIN="${MMD_LAB_BIN:-}"
DRY_RUN=0
FIXTURE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --fixture) FIXTURE="${2:-}"; shift 2 ;;
    --manifest) MANIFEST="${2:-}"; shift 2 ;;
    --lab-bin) LAB_BIN="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ "$DRY_RUN" -ne 1 ]]; then
  echo "live host inspect not implemented (T16). use --dry-run --fixture <json>" >&2
  exit 2
fi

if [[ -z "$FIXTURE" || ! -f "$FIXTURE" ]]; then
  echo "missing --fixture path" >&2
  exit 2
fi

if [[ ! -f "$MANIFEST" ]]; then
  echo "manifest missing: $MANIFEST" >&2
  exit 2
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

exec "$LAB_BIN" attest-ubuntu --manifest "$MANIFEST" --observed "$FIXTURE"
