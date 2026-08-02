# ADR 008: Open-Source Governance

- Status: Accepted
- Date: 2026-08-02
- Superseded by: —

## Context

Repo should accept public contributions while one owner controls official roadmap, merges, releases. User requests most permissive practical software license. Online CI remains absent.

## Decision

Hosting/governance:

- Public GitHub repo.
- GitHub Actions absent/disabled.
- PR required for `main`.
- Force-push, branch deletion, auto-merge blocked.
- Sole maintainer owns merge + official release authority.
- Contributors retain copyright in contributions.
- No PR accepted automatically.

License/contrib:

- SPDX `MIT-0`.
- Covers project-authored code, docs, shaders, generated placeholder assets.
- Dependencies/third-party material keep own terms.
- Project name/logo reserved; forks must not imply official status.
- DCO 1.1 sign-off required.
- `CONTRIBUTING.md`: inbound=outbound under repo license.
- No copyright assignment/CLA.

PR validation:

- Contributor runs fast local checks.
- Owner reviews first.
- Owner fetches exact commit; runs full local 3-host gate.
- Owner posts concise pass summary manually.
- Owner merges exact tested hash only.

## Consequences

Positive:

- Near-zero downstream attribution friction.
- Open contribution path.
- Official authority stays clear through access/marks.
- DCO gives low-friction provenance record.

Negative:

- MIT-0 has no express patent grant.
- Anyone may fork/sell proprietary or competing builds.
- Owner cannot unilaterally relicense contributor code incompatibly.
- Contributors cannot reproduce owner physical perf gate exactly.

## Rejected alternatives

- Apache-2.0: stronger patent grant; more notice/change duties; less “most free.”
- CC0: not OSI-approved for software; patent rights expressly excluded.
- Copyright assignment: excess contributor friction.
- Unrestricted self-hosted PR execution: unsafe.
- Online CI: explicitly rejected.

## Validation

- SPDX/license file checks.
- DCO sign-off reviewed before local gate.
- Branch settings manually verified.
- PR summary binds exact commit/archive hash.
- No GitHub Actions workflow files.

## References

- `LICENSE` (planned T1)
- `CONTRIBUTING.md` (planned T1)
- `docs/ADR/007_ADR_local_validation_lab_and_security.md`
- `.tmp/IMPLEMENTATION_PLAN_technical_prototype.md` T1, T13, T17
