# T14: Generate tracked placeholder audio

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T1  
**Commit outcome:** offline `xtask audio` emits byte-identical licensed placeholder music/voice/UI WAVs + manifest; `--check` detects drift.

## Context (self-contained)

- Goal: supply legal deterministic audio before runtime code; no StarCraft file enters repo.
- This slice: integer-only generator, WAV format, provenance/hash gate.
- Out of scope here: playback, events, volume settings UI.
- Assumptions: core pinned SDL loads PCM WAV; no mixer/codec/new crate; generated source is MIT-0 project output.
- Decision: `docs/ADR/020_ADR_audio_events_buses_and_generated_assets.md`.

## Requirements

- Create `xtask/src/audio.rs`; add `xtask audio [--check]` in `xtask/src/main.rs` using existing clap style.
- Output under `assets/audio/generated/`:
  - `music_placeholder.wav`
  - `voice_select.wav`, `voice_move.wav`, `voice_gather.wav`, `voice_build.wav`, `voice_reject.wav`
  - `ui_click.wav`
  - `manifest.json`
- Shared WAV contract: RIFF/WAVE PCM tag1; 48,000Hz; signed little-endian 16-bit; stereo duplicated channels; block align4; byte rate192,000; only RIFF/fmt(16)/data chunks.
- Integer-only triangle/DDS generation. No RNG, clock, host trig/float serialization.
- Exact assets:
  - music: 384,000 frames (8s), loop-safe 16 half-second steps; bass `110,110,147,147,98,98,131,131`; lead `220,262,330,262,196,247,294,247`; pattern repeats; first/last sample zero; peak <=6144.
  - select 5,760 frames/660Hz; move 5,760/520Hz; gather 5,760/740Hz; build 5,760/440Hz; each 120ms.
  - reject 8,640 frames/180ms: 220Hz then165Hz.
  - UI 2,880 frames/60ms/880Hz.
  - cues use 5ms integer attack/release; each peak <=1536; hashes distinct.
- Manifest schema1/generator `mmd-audio-placeholder-v1`/license `MIT-0`; per asset id/file/role/frames/rate/channels/bits/peak/SHA-256.
- Add `assets/audio/README.md`: generated provenance, replacement contract, no copyrighted “Terran One,” licensed replacement must include source/license/attribution/checksum.
- `--check`: generate temp; byte-compare files/canonical pretty manifest; reject missing/unexpected WAV; parse headers/lengths; verify stereo duplication/frames/peaks/endpoints/distinct hashes.
- Add no `hound`, `sdl3_mixer`, `rodio`, network access.

## Inputs

- `xtask/src/atlases.rs`, `placeholder_art.rs`, `main.rs` patterns.
- `xtask/Cargo.toml` already has serde_json/sha2/tempfile(dev).
- **From Depends:** T1 only establishes phase scope; audio asset work independent.

## TDD

1. **Red** — generator/header/duration/headroom/loop/hash/check-drift tests first.
2. **Green** — integer samples + canonical WAV/manifest writer + CLI.
3. **Refactor** — one asset spec table drives generation/manifest/check; no duplicated numbers.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `generated_wavs_use_locked_pcm_format` | all specs | exact header/rate/channels/bits |
| `generated_durations_are_exact` | all files | exact frames above |
| `music_loop_endpoints_are_zero` | music | first/last L/R zero |
| `cue_hashes_are_distinct` | six cues | unique SHA-256 |
| `source_mix_has_headroom` | music + 9 voice +4 SFX peaks | sum <=26112 |
| `double_generation_is_byte_identical` | two temp dirs | all bytes equal |
| `audio_check_detects_tamper` | flip byte/add file | actionable failure |
| `manifest_hashes_match_files` | tracked dir | exact |

## Impl steps

- [ ] 1. Add `audio.rs` unit tests and asset spec table.
- [ ] 2. Implement canonical WAV header/sample writer.
- [ ] 3. Implement integer triangle/envelopes/music sequence.
- [ ] 4. Implement manifest generation/hash/validation.
- [ ] 5. Wire `xtask audio` and `--check`.
- [ ] 6. Generate tracked assets + README.
- [ ] 7. Tamper-copy test proves `--check` fails without mutating tracked files.

## Outputs

- New: generator, 7 WAVs, manifest, README.
- Modified: xtask main/module tests.
- CLI API: `cargo run -p xtask -- audio [--check]`.
- Behavior: reproducible legal placeholder asset family.
- Assets/config: schema-1 audio manifest; no third-party binary.

## Validation

- [ ] `cargo test -p xtask --locked audio`
- [ ] `cargo run -p xtask -- audio --check`
- [ ] `cargo check --workspace --all-targets --all-features --locked`
- [ ] manual check: inspect README/manifest; confirm no StarCraft/Terran binary/source URL
- [ ] app functional: unchanged; assets not loaded yet
- [ ] commit msg draft: `feat(xtask): generate licensed placeholder RTS audio deterministically`
