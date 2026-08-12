# T16: Add SDL audio runtime

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T10, T14, T15  
**Commit outcome:** interactive RTS plays continuous placeholder music + capped cues through pinned core SDL; device/asset failures exit 1; offscreen never opens audio.

## Context (self-contained)

- Goal: turn tested semantic events into audible output without adding mixer/codec deps or sim nondeterminism.
- This slice: WAV validation, SDL device/stream pools, loop maintenance, startup/runtime error policy.
- Out of scope here: copyrighted replacement track, combat SFX, device selector/hotplug UX.
- Assumptions: music continues through menus/focus loss; latest unit action replaces prior unit cue batch; reject has dedicated voice lane; SFX round-robin 4 lanes.
- Decision: ADR 020.

## Requirements

- Create `src/rts_audio.rs` implementing T15 `AudioSink` as `SdlAudioSink`.
- Load every T14 WAV via `AudioSpecWAV::load_wav`; independently validate manifest + exact PCM 48kHz/stereo/i16 before opening device.
- Use pinned safe core SDL only:
  - `AudioSubsystem::open_playback_device(&AudioSpec)`;
  - `AudioSubsystem::new_playback_stream(&app_spec,None)`;
  - `AudioDevice::bind_streams`, `AudioDevice::resume`;
  - stream `put_data`, `queued_bytes`, `clear`, `set_gain`.
- Own 14 streams: music1; unit voice8; reject voice1; UI SFX4. Bind once at startup.
- Event policy:
  - StartMusic marks active; `maintain` queues full music buffers while queued bytes `<2*music_len`; no clear/flush; first/last zero makes boundary click-free.
  - Voice batch clears all eight unit streams; queues cue assets into streams `0..len`; latest unit action wins.
  - Reject clears/requeues dedicated reject stream; never steals accepted unit cue.
  - UI uses 4-slot round-robin; fifth overlapping click clears/steals oldest slot.
- Gain: music/voice/SFX stream gains = basis points/10000. Device gain stays1. Muted streams still advance.
- Startup ordering:
  1. before frame1 use `BufferedAudioSink`; record StartMusic/frame1 events;
  2. if offscreen/no real window, convert/replay to FakeAudioSink; no physical audio/user cfg;
  3. after real window successfully created/claimed, build SDL sink + replay buffer before first present;
  4. load/open/create/bind/gain/queue/resume failure: release claimed window, return `RunError::Failed`, exit1; no offscreen fallback.
- Runtime stream error from emit/maintain/gain is fatal interactive error; release window then exit1 with operation + asset/device context.
- Focus/menu/pause code never pauses/clears music.
- Tests use `SDL_AUDIODRIVER=dummy`; no real-device sound assertion in merge gate.

## Inputs

- T14 manifest/WAV files/asset IDs.
- T15 sink/events/gains/fake/buffer.
- T10 safe window release lifecycle.
- pinned `sdl3 0.18.4` audio API.
- **From Depends:** exact files/types above.

## TDD

1. **Red** — manifest/spec rejection, fake stream policy, watermark/gain/lanes, startup mode/failure tests.
2. **Green** — loader + backend + buffered startup replay.
3. **Refactor** — abstract minimal stream ops for deterministic unit tests; real SDL smoke stays narrow/dummy.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `loader_rejects_wrong_wav_spec_or_hash` | tampered temp asset | actionable error before device |
| `music_watermark_queues_two_buffers` | empty/one/full queue | queues until >=2×len |
| `voice_batch_replaces_eight_lanes` | two batches | second assets only |
| `reject_uses_dedicated_voice_lane` | voice + reject | accepted lanes unchanged |
| `fifth_ui_click_steals_oldest_sfx_lane` | 5 rapid UI events | deterministic round robin |
| `live_gain_change_updates_all_bus_streams` | settings edit | exact float gains |
| `buffered_frame1_events_replay_once` | pre-window actions | no loss/dup |
| `audio_failure_aborts_interactive_startup` | invalid audio driver | exit1 + no clean exit |
| `offscreen_never_initializes_physical_audio` | failing audio env + offscreen | success + fake trace |
| `dummy_driver_starts_and_maintains` | SDL dummy | bind/resume/queue success |

## Impl steps

- [x] 1. Add pure asset/stream-policy tests + fake stream adapter.
- [x] 2. Implement manifest/WAV loader validation.
- [x] 3. Create/bind 14 streams and set exact gains.
- [x] 4. Implement event routing + music watermark maintenance.
- [x] 5. Add buffered pre-window sink/replay selection.
- [x] 6. Integrate interactive fatal error + release ordering.
- [x] 7. Add offscreen no-device + dummy-driver tests.
- [x] 8. Confirm no new Cargo dependency/feature/lock package.

## Outputs

- New: `src/rts_audio.rs`.
- Modified: main/run/feedback/CLI tests.
- App API: `SdlAudioSink`, validated asset bank, buffered sink.
- Behavior: physical interactive audio + strict failure; fake offscreen.
- Config: live gains consume schema1.

## Validation

- [x] `SDL_AUDIODRIVER=dummy cargo test -p millions_must_die --locked rts_audio` — 11 passed
- [x] `cargo test -p millions_must_die --locked --test rts_cli_contract audio_` — 8 passed (incl. new `audio_offscreen_survives_invalid_audio_driver`)
- [x] `cargo check --workspace --all-targets --all-features --locked` — clean
- [x] `git diff --exit-code -- Cargo.lock` — no diff (sha2 already present via xtask/mmd-engine; app crate move dev→normal dep changes nothing in the lock)
- [ ] manual check: music continuous through menu/Alt-Tab; select/order/reject/UI cues distinct; sliders audible — **deferred to the human checklist** (hard constraint: this worker must not self-verify audio through the live desktop's devices)
- [x] app functional: `SDL_VIDEODRIVER=offscreen SDL_AUDIODRIVER=invalid cargo run -- rts --frames 3` succeeds via fake sink — exit 0, `rts: audio` line present
- [x] commit msg draft: `feat(app): play deterministic RTS feedback through core SDL audio`
