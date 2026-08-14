# Plan: Audit issue #5 — archive resource bounds

## Goal

Bound `mmd-lab` source packing before stack, heap, file-size, aggregate-size, or encoded-size exhaustion. Preserve deterministic `MMDARC01` output for valid trees; return typed, stable resource errors for rejected trees.

## Scope

- In: `tools/mmd-lab/src/archive.rs`; focused archive tests; `docs/lab/local-validation.md` contract update.
- Out: F5 numeric validation; transport redesign; SSH impl; parser/extractor hardening; archive format change; CLI limit overrides; live large-payload tests; broad docs/manual-checklist work.

## Assumptions

- Autonomous Markdown-only override active → no HTML plan, ADR, architecture HTML, browser open.
- One compile-green commit-sized slice suffices. No dep or format migration.
- Prod defaults fixed: depth `32`; visited dir entries `16_384`; packed files `8_192`; per file `16 MiB`; cumulative decoded data `64 MiB`; final encoded blob `80 MiB`.
- Current clean-tree evidence: depth `5`; visited entries `481`; files `408`; largest file `1_536_044 B`; decoded data `7_247_432 B`; encoded blob `7_269_276 B`. Defaults retain 6.4×–34× structural headroom, 9.3× decoded headroom, 11.5× blob headroom.
- `64 MiB + 3 × 80 MiB = 304 MiB` conservative coordinator envelope covers retained source data plus original encoded blob plus two fake-transport/hash-verification clones. Actual phases overlap less. Ref hosts document `16 GB` RAM.
- Limits remain non-configurable safety ceilings. Existing `pack_tree` / `build_archive` signatures stay source-compatible.

## Ticket flowchart

```mermaid
flowchart TD
    T1[T1: Bound deterministic archive packing]
```

## Ticket order

| ID | Title | Depends | Commit outcome | File |
| --- | --- | --- | --- | --- |
| T1 | Bound deterministic archive packing | — | Every prod archive pack uses fixed ceilings, bounded iterative IO, typed errors, boundary tests, documented contract | `PLAN_2026_08_14_audit_issue_5_archive_resource_bounds/T1_bound-deterministic-archive-packing.md` |

## Tickets

- [T1: Bound deterministic archive packing](PLAN_2026_08_14_audit_issue_5_archive_resource_bounds/T1_bound-deterministic-archive-packing.md) — depends: none
