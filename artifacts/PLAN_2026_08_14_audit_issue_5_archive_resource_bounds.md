# Plan: Audit issue #5 — archive resource bounds

## Goal

Bound `mmd-lab` source packing before stack, heap, file-size, aggregate-size, or encoded-size exhaustion. Preserve deterministic `MMDARC01` output for valid trees; return typed, stable resource errors for rejected trees.

## Scope

- In: `tools/mmd-lab/src/archive.rs`; focused archive tests in that file; `docs/lab/local-validation.md` contract update.
- Out: F5 numeric validation; transport redesign; SSH impl; parser/extractor hardening; archive format change; CLI limit overrides; live large-payload tests; broad docs/manual-checklist work.

## Assumptions and measured evidence

- Autonomous Markdown-only override active → no HTML plan, ADR, architecture HTML, browser open.
- One compile-green commit-sized slice suffices. No dep or format migration.
- Prod defaults stay fixed: depth `32`; visited dir entries `16_384`; packed files `8_192`; per file `16 MiB`; cumulative decoded data `64 MiB`; final encoded blob `80 MiB`.
- Filesystem measurement source is exact pre-repair plan commit `994d292f7c01a314d16e01302377daf64a96904a`, not post-impl tree: depth `5`; visited entries `488` under proposed pre-filter rule, including enumerated `.git`, `.pi-subagents`, `.tmp`, `target`; files `410`; largest file `1_536_044 B`; decoded data `7_269_289 B`; format/path overhead `22_045 B`; encoded blob `7_291_334 B`. Defaults retain ample structural + byte headroom.
- `64 MiB + 3 × 80 MiB = 304 MiB` accounts only major byte buffers: retained decoded data, encoded blob, fake-delivery clone, hash-verification clone. It is not exact total process peak. Path/entry metadata is structurally bounded by depth/count + host filesystem name/path limits but lacks separate byte cap; allocator/hash/std/OS overhead also sits outside `304 MiB` subtotal. Operator docs must call this bounded-buffer subtotal, not RAM ceiling.
- Limits remain non-configurable safety ceilings. Existing `pack_tree` / `build_archive` signatures stay source-compatible.

## Ticket flowchart

```mermaid
flowchart TD
    T1[T1: Bound deterministic archive packing]
```

## Ticket order

| ID | Title | Depends | Commit outcome | File |
| --- | --- | --- | --- | --- |
| T1 | Bound deterministic archive packing | — | Every prod archive pack uses fixed ceilings, bounded iterative IO, typed errors, 17 boundary/regression tests, documented contract | `PLAN_2026_08_14_audit_issue_5_archive_resource_bounds/T1_bound-deterministic-archive-packing.md` |

## Tickets

- [T1: Bound deterministic archive packing](PLAN_2026_08_14_audit_issue_5_archive_resource_bounds/T1_bound-deterministic-archive-packing.md) — depends: none
