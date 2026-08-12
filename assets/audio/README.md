# Generated placeholder audio

Everything under `assets/audio/generated/` is produced offline by
`cargo run -p xtask -- audio` (verify with `cargo run -p xtask -- audio
--check`). No third-party audio file — StarCraft ("Terran One" or any other
Blizzard track/voice/SFX) or otherwise — is present or referenced anywhere in
this tree or its generator.

## What it is

Integer-only synthesis: a phase-accumulator triangle oscillator plus a 5ms
integer linear attack/release envelope. No floating-point trig, no RNG, no
host clock — so `xtask audio` is deterministic and produces byte-identical
output on every host and every run. `xtask/src/audio.rs` is the single
source of truth for every constant (frequencies, frame counts, amplitudes);
`manifest.json` records id/file/role/frame count/format/peak/SHA-256 per
asset.

Generator id: `mmd-audio-placeholder-v1`. License: `MIT-0` (this is
project-authored generator output, not a derivative of any external
recording).

## WAV contract

RIFF/WAVE, PCM tag 1, only RIFF/`fmt `(16)/`data` chunks, 48,000 Hz,
signed little-endian 16-bit, stereo with both channels holding an identical
sample (mono content, duplicated), block align 4, byte rate 192,000.

## Replacing a placeholder with real licensed audio

1. Get a properly licensed replacement (composed/recorded for this project,
   or licensed under terms compatible with `MIT-0`/redistribution — no
   copyrighted commercial game audio).
2. Convert it to the WAV contract above.
3. Replace the file under `assets/audio/generated/` and update its
   `manifest.json` entry (`frames`, `peak`, `sha256`) to match.
4. Record, in the commit message or an adjacent note, the source, its
   license, and attribution if the license requires it, plus the new file's
   SHA-256.
5. `cargo run -p xtask -- audio --check` will then fail (by design) until
   the generator itself is updated to stop producing that asset, or the
   asset is moved out of the generated/deterministic-check set.
