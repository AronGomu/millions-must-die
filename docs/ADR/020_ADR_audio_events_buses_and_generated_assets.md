# ADR 020: Audio events, buses, runtime, and generated assets

- Status: Accepted
- Date: 2026-08-10
- Accepted: 2026-08-12 (T18, on landed phase-1.1 evidence)
- Supplements: [ADR 019](019_ADR_hud_minimap_and_input_routing.md)
- Plan: `ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`

## Context

Repo has no audio dep/impl/assets. User requested background music + unit voice feedback, then chose generated placeholder music until a licensed replacement exists. “Terran One” is copyrighted; no license/provenance/file exists.

Pinned core SDL3 already supports PCM WAV, playback device, streams, queue, gain. New mixer/codec dependency is unnecessary.

## Decision

### Boundary

Audio stays app-side. `RtsWorld` remains clock-free, deterministic, audio-free. Shared input actions return explicit success receipts; app derives semantic `AudioEvent`; sink handles I/O.

Offscreen/headless uses fixed fake sink, no physical audio. Interactive audio init/runtime failure is fatal exit1 after safe window release.

### Events

- Music: Start once/session; loop continuously through menus/pause/focus loss.
- Selection voice: newly selected player units only; ascending; first8.
- Order voice: units accepting Move/Gather/Build; ascending global first8/action.
- Partial/total rejected unit order: one distinct Reject cue/action.
- Accepted overflow is not rejection.
- UI SFX: successful pointer activation of gear/menu/settings edit/command card/minimap; once/action.
- Disabled/background/bar/keyboard UI actions emit no click SFX.

Voice cue kinds: Select, Move, Gather, Build, Reject. Buildings/nodes do not speak.

### Buses/gain

Buses Music/Voice/SFX plus master scalar.

Defaults %: master80, music35, voice70, SFX60. Range0..100 step5.

Semantic gain uses integer basis points:

```text
music=80*35=2800
voice=80*70=5600
sfx=80*60=4800
SDL gain=basis_points/10000
```

No polyphony normalization. Muted streams continue progress.

### Assets

`xtask audio` generates deterministic stereo PCM16 48kHz WAVs + schema1 SHA-256 manifest under `assets/audio/generated/`.

- 8s loop-safe placeholder music.
- select/move/gather/build/reject voice beeps.
- UI click beep.

Integer-only oscillator/envelope; no RNG/clock/trig. MIT-0. `--check` regenerates/byte-compares/validates headers, durations, headroom, loop endpoints, unexpected files.

Copyrighted replacement requires source/license/attribution/checksum. No StarCraft file now.

### SDL runtime

One device +14 bound streams:

- music1;
- accepted unit voice8;
- reject voice1 dedicated;
- UI SFX4 round-robin.

New accepted batch clears/replaces eight unit lanes. Reject never steals them. Fifth overlapping UI click steals oldest SFX lane.

`maintain()` queues music while queued bytes <2× track bytes. No loop API assumption.

Pre-window buffered sink records frame1 events. Interactive sink replays once after successful window claim. Offscreen fake replays once.

## Consequences

- `cargo run -p xtask -- audio --check` joins merge gate when implementation lands.
- No actual audible-output claim from headless gate. Dummy-driver tests prove API/lifecycle only; manual check proves hearing on current host.
- Startup fails if interactive audio unavailable—even if volumes muted—per confirmed choice.
- Asset spec/manifest becomes replacement contract.
- Future combat SFX needs separate content/polyphony decision.

## Rejected

- Bundle “Terran One” without rights: illegal distribution risk.
- SDL_mixer/rodio/cpal/hound: unnecessary deps/native/license surface.
- Audio events in `RtsWorld`: pollutes deterministic state with I/O intent.
- One mixed stream callback: more thread/unsafe/mixer complexity.
- Per-unit >8 voice stacking: clipping/cacophony.
- Pause music on focus/menu: contradicts confirmed continuous lifecycle.

## Implementation (as landed, T14–T16)

Shipped as decided. `cargo run -p xtask -- audio --check` is now on the
required merge gate (`docs/05-testing.md`), printing
`audio: ok (7 wav + manifest)`; the generator is `xtask/src/audio.rs`, the
semantic layer `src/rts_feedback.rs` (`AudioEvent`, `AudioBus`, `AudioSink`,
`effective_gains`) and the device layer `src/rts_audio.rs` (`SdlAudioSink`,
14 streams, `BufferedAudioSink` for pre-window frame-1 events).

One addition to the observation surface: the counters this record specified are
reported twice — as `rts: audio music=… voice=… cues=… reject=… ui=…
gains=…/…/…`, and split by meaning on the exit line as `music_starts`,
`voice_select`, `voice_order`, `voice_reject`, `sfx_ui`, so the 1,600-frame
acceptance run can assert selection cues apart from order cues.

The audible claim boundary is unchanged and load-bearing: no automated test on
this gate proves anything was heard. Dummy-driver tests prove API and lifecycle
(`dummy_driver_starts_and_maintains`,
`audio_device_open_failure_is_actionable`); hearing it is a human checklist
item.

## Validation contract

Tests prove byte-identical generated assets/manifest/headroom; exact event sorting/caps/reject/UI mapping/gains; fake sink no hash change; stream lane/watermark policy; interactive failure exit1; offscreen never opens device; one 1,600-frame acceptance reports exact music/voice/reject/SFX counters.

## Amendment 2026-08-15 — explicit mute flags

Supplemented by
[ADR 021](021_ADR_rts_feedback_polish_and_gather_collision.md): each bus gains
a persisted mute flag, toggled by clicking its label. A muted bus contributes
gain zero while its stream keeps running and its stored level is untouched, so
unmuting restores the exact level the user had. Muting is therefore not
"volume 0" — the generated assets, the lane policy and the counters in this
record are unchanged.
