# Contributing

Public contributions welcome. Sole maintainer owns roadmap, merge, and official release authority. Contributors retain copyright in their contributions.

## License (inbound = outbound)

By contributing, you agree your contribution is provided under the same license as this repository: **MIT-0** (see [`LICENSE`](LICENSE)).

- No CLA and no copyright assignment.
- Third-party / dependency code keeps its own license terms.
- Project name and logo are reserved; see [`TRADEMARKS.md`](TRADEMARKS.md).

## Developer Certificate of Origin (DCO) 1.1

Every commit must include a sign-off line:

```text
Signed-off-by: Your Name <your.email@example.com>
```

Use `git commit -s` (or equivalent). Sign-off certifies:

```text
Developer Certificate of Origin
Version 1.1

Copyright (C) 2004, 2006 The Linux Foundation and its contributors.

Everyone is permitted to copy and distribute verbatim copies of this
license document, but changing it is not allowed.


Developer's Certificate of Origin 1.1

By making a contribution to this project, I certify that:

(a) The contribution was created in whole or in part by me and I
    have the right to submit it under the open source license
    indicated in the file; or

(b) The contribution is based upon previous work that, to the best
    of my knowledge, is covered under an appropriate open source
    license and I have the right under that license to submit that
    work with modifications, whether created in whole or in part
    by me, under the same open source license (unless I am
    permitted to submit under a different license), as indicated
    in the file; or

(c) The contribution was provided directly to me by some other
    person who certified (a), (b) or (c) and I have not modified
    it.

(d) I understand and agree that this project and the contribution
    are public and that a record of the contribution (including all
    personal information I submit with it, including my sign-off) is
    maintained indefinitely and may be redistributed consistent with
    this project or the open source license(s) involved.
```

## Pull requests

1. Open a PR against `main`. Direct pushes to `main` are blocked.
2. Run fast local checks before requesting review:

   ```sh
   cargo fmt --all -- --check
   cargo test --workspace --locked
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   ```

3. Maintainer reviews, fetches the **exact** commit, runs the full local multi-host gate, posts a concise pass summary, and merges **only** that tested hash.
4. No online CI. No auto-merge.

## Branch protection (manual GitHub settings)

Configure on the default branch (maintainer-operated; not enforced by repo files):

- Require a pull request before merging.
- Block force-push.
- Block branch deletion.
- Disable auto-merge.
- Restrict who can merge to the repository owner.

GitHub Actions must remain absent or disabled for this repository.

## Trust boundary

- App/engine code from a PR is **untrusted candidate** input.
- `mmd-lab` is the trusted coordinator and must be installed from trusted `main`, never from a candidate archive.
- Do not add GitHub Actions workflow files.
