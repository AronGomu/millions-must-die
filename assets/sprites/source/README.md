# Zombie sprite source

`stoner-games-zombie-strip12.png` is a pinned copy of Stoner Games' **Zombie Sprite**.

- Source page: <https://opengameart.org/content/zombie-sprite>
- Original download: <https://opengameart.org/sites/default/files/spr_ZombieEyeBall_strip12_0.png>
- Author: Stoner Games
- License: [CC0 1.0 Universal](https://creativecommons.org/publicdomain/zero/1.0/)
- Upstream credit request: `~Stoner Games~` (optional under CC0)
- Source SHA-256: `5207803a33b04bf45cfcb80308f340262128731b6cd51a5e569833ecdc7379f3`
- Source layout: 12 horizontal 128×128 RGBA frames; 1536×128 total

`cargo run -p xtask -- atlases` deterministically crops each frame to its central 80×128 character region, downsamples to 32×32, premultiplies alpha, mirrors west-facing direction rows, phase-offsets four atlas variants, then writes `assets/sprites/generated/`.

Project-authored code remains MIT-0. Source art plus derived atlases remain CC0-1.0.
