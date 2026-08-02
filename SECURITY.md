# Security policy

## Supported versions

This project is pre-production. Only the latest commit on `main` receives security attention.

## Reporting a vulnerability

Do **not** open a public issue for security-sensitive reports.

Email the maintainer at the address listed on the GitHub profile for `@AronGomu`, with:

- affected commit / tag
- impact description
- reproduction steps or proof-of-concept when safe
- any known mitigations

Please allow reasonable time for assessment before public disclosure.

## Scope notes

- Candidate archives and PR trees are untrusted. Never execute coordinator/reset tooling supplied by a candidate.
- Reference lab hosts are intended to run without secrets and with blocked candidate egress.
- Online CI is intentionally absent; do not propose hosted runners that execute untrusted PR code with elevated trust.
