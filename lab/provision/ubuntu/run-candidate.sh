#!/usr/bin/env bash
# Ubuntu candidate gate lane (T17).
# EXTERNAL COORDINATOR ONLY — never invoke from candidate OS / untrusted tree.
#
# Lane (coordinator-verified; candidate report never trusted):
#   1. Coordinator self-check
#   2. Host attestation (identity only) + recovery ready-for-candidate gate
#   3. Deliver exact content-addressed archive; remote hash verify (mismatch = no exec)
#   4. Run visible Vulkan smoke + offscreen scale curve   [deferred-hw: native run]
#   5. Collect raw samples / readback / manifests
#   6. Coordinator recomputes stats + golden diff from raw evidence
#   7. Force post-run external restore                    [deferred-hw: physical reset]
#
# Fixture mode drives the exact coordinator protocol with fake transport +
# committed raw/report fixtures. Live mode needs the physical Ubuntu ref lab.
#
# Bounded risk (accepted): attestation covers host/source identity only; a
# hostile candidate can still deny service or forge its raw output wholesale —
# including the adapter/os strings the golden binding reads from candidate
# evidence (binding identity cannot come from attestation until the golden is
# recaptured on the ref host). Coordinator recompute + forced reset bound,
# but do not remove, that risk.
#
# Fixture note: the committed pass fixtures bind to the currently captured
# linux-vulkan golden (dev adapter). Recapture on the frozen RX 6400 ref host
# is deferred-hw and will retire that binding.
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  run-candidate.sh --fixture-mode --commit <exact-hash> [--lab-bin <mmd-lab>]
                   [--evidence <json>] [--readback <png>] [--attest <json>]
                   [--runner-manifest <toml>] [--golden-dir <dir>] [--root <dir>]
  run-candidate.sh --help

Fixture mode runs the trusted coordinator lane (`mmd-lab validate-runner
--runner ubuntu`) against fake transport + raw/report fixtures.

Live candidate execution requires the physical Ubuntu ref host + external
recovery lab (deferred-hw).

Exit: 0 coordinator-verified pass; 1 fail; 2 usage/env/inconclusive;
      3 coordinator error.
EOF
}

need_val() {
  if [[ $# -lt 2 || -z "${2:-}" ]]; then
    echo "missing value for $1" >&2
    exit 2
  fi
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
LAB_BIN="${MMD_LAB_BIN:-}"
FIXTURE_MODE=0
COMMIT=""
EVIDENCE=""
READBACK=""
ATTEST=""
RUNNER_MANIFEST=""
GOLDEN_DIR=""
SRC_ROOT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --fixture-mode) FIXTURE_MODE=1; shift ;;
    --commit) need_val "$@"; COMMIT="$2"; shift 2 ;;
    --evidence) need_val "$@"; EVIDENCE="$2"; shift 2 ;;
    --readback) need_val "$@"; READBACK="$2"; shift 2 ;;
    --attest) need_val "$@"; ATTEST="$2"; shift 2 ;;
    --runner-manifest) need_val "$@"; RUNNER_MANIFEST="$2"; shift 2 ;;
    --golden-dir) need_val "$@"; GOLDEN_DIR="$2"; shift 2 ;;
    --root) need_val "$@"; SRC_ROOT="$2"; shift 2 ;;
    --lab-bin) need_val "$@"; LAB_BIN="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown arg: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ "$FIXTURE_MODE" -ne 1 ]]; then
  cat >&2 <<'EOF'
live candidate run not available on this host (deferred-hw).
need: Ubuntu 24.04 ref PC (8600G / RX 6400) + external recovery lab (T16)
use: run-candidate.sh --fixture-mode --commit <exact-hash>
EOF
  exit 2
fi

if [[ -z "$COMMIT" ]]; then
  echo "missing --commit <exact-hash> (candidate identity is mandatory)" >&2
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

ARGS=(validate-runner --runner ubuntu --commit "$COMMIT")
[[ -n "$SRC_ROOT" ]] && ARGS+=(--root "$SRC_ROOT")
[[ -n "$RUNNER_MANIFEST" ]] && ARGS+=(--runner-manifest "$RUNNER_MANIFEST")
[[ -n "$ATTEST" ]] && ARGS+=(--attest-fixture "$ATTEST")
[[ -n "$EVIDENCE" ]] && ARGS+=(--evidence-fixture "$EVIDENCE")
[[ -n "$READBACK" ]] && ARGS+=(--readback-fixture "$READBACK")
[[ -n "$GOLDEN_DIR" ]] && ARGS+=(--golden-dir "$GOLDEN_DIR")

echo "run-candidate: fixture mode (fake transport; native run deferred-hw)"
echo "run-candidate: coordinator $LAB_BIN"
echo "run-candidate: workspace $ROOT"

exec "$LAB_BIN" "${ARGS[@]}"
