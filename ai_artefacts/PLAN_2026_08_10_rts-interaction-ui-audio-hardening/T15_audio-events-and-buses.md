# T15: Emit deterministic audio events and buses

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T2, T5, T7, T12, T14  
**Commit outcome:** fake sink deterministically observes music start, capped per-unit selection/order cues, one reject cue, UI SFX, exact bus gains; engine state stays audio-free.

## Context (self-contained)

- Goal: define/test sound semantics before nondeterministic SDL I/O.
- This slice: app-level outcomes/events/buses/fake sink + command/UI integration.
- Out of scope here: physical device/stream/WAV playback.
- Assumptions: accepted IDs sorted; global first8 cap/action; accepted overflow is not rejection; buildings/nodes never voice; reject cue once/action; keyboard hotkeys do not emit pointer-click SFX.
- Decision: `docs/ADR/020_ADR_audio_events_buses_and_generated_assets.md`.

## Requirements

- Keep all audio types in app (`src/rts_feedback.rs`), outside `mmd-engine` state/hash:
  ```rust
  pub const MAX_UNIT_CUES_PER_ACTION: usize = 8;
  pub enum AudioBus { Music, Voice, Sfx }
  pub enum VoiceCue { Select, Move, Gather, Build }
  pub struct UnitCue { pub entity: EntityId, pub cue: VoiceCue }
  pub struct VoiceBatch { pub entries: [Option<UnitCue>; 8], pub len: u8 }
  pub enum UiCue { Menu, Settings, CommandGrid, Minimap }
  pub enum AudioEvent { StartMusic, Voice(VoiceBatch), Reject, Ui(UiCue) }
  pub struct EffectiveGains { pub music_basis_points: u16, pub voice_basis_points: u16, pub sfx_basis_points: u16 }
  pub trait AudioSink { fn set_gains(&mut self, gains: EffectiveGains) -> Result<(), AudioError>; fn emit(&mut self, event: AudioEvent) -> Result<(), AudioError>; fn maintain(&mut self) -> Result<(), AudioError>; }
  ```
- `effective_gains`: `master*bus` basis points. Defaults produce music2800, voice5600, SFX4800. SDL conversion later `/10000.0`.
- `FakeAudioSink` stores fixed-cap trace/counts; no heap growth after `new`; app/offscreen acceptance uses it.
- Selection event: snapshot selected player-unit IDs before action; apply click/Shift/drag/HUD icon; sorted set difference new-old; emit first8 Select. Deselection/reselect already-selected only → silence.
- Context outcomes consume T2/T5 receipts:
  - accepted Move/Gather/Build → matching cue, merge/sort globally, first8;
  - rejected candidates >0 → one Reject in addition to accepted batch;
  - invalid goal with selected orderable unit → one Reject;
  - empty selection/HUD/bar invalid point → silence.
- UI SFX once after successful enabled pointer action: gear/pause Settings button → Menu; settings edit → Settings; command card → CommandGrid; valid minimap recenter → Minimap. Disabled/background/keyboard → silence.
- Queue `StartMusic` once per RTS session before frame1. Music remains logically active across menu/focus.
- T7 setting edit immediately calls `set_gains` hook in addition to persistence. Fake failure follows T13 rollback/error path.
- `RtsSession` owns fixed event scratch and sink generic/trait object established at startup; scripted/live paths call same `apply` and same event derivation.

## Inputs

- T2 `OrderReceiptBuffer`, `IssuedOrder`, shared context path.
- T5 deterministic sorted group receipts.
- T7 audio settings/defaults.
- T12 UI action sources/router.
- T14 asset IDs (runtime path not used yet).
- **From Depends:** all semantics above; no source ticket reading required beyond current files.

## TDD

1. **Red** — gains, selection delta, mixed outcome/global cap, partial reject, UI mapping, music once, no engine hash effect.
2. **Green** — fixed event types/fake sink; instrument shared action results.
3. **Refactor** — one receipt→batch fn; no post-hoc world state inference for orders.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `default_effective_gains_are_exact` | 80/35/70/60 | 2800/5600/4800 |
| `selection_cues_only_new_player_units` | select/reselect/deselect | first action only |
| `selection_batch_is_sorted_and_capped` | 12 shuffled IDs | first8 ascending |
| `mixed_resource_order_uses_one_global_cap` | workers+soldiers | <=8 mixed Gather/Move |
| `partial_success_emits_voice_and_one_reject` | accepted + rejected | batch + Reject |
| `total_rejection_emits_one_reject` | invalid selected order | Reject once |
| `accepted_overflow_is_not_rejection` | 20 accepted | batch8, no Reject |
| `ui_actions_map_to_sfx_sources` | four enabled clicks | exact Ui variants |
| `disabled_or_keyboard_action_has_no_ui_sfx` | disabled/card hotkey | none |
| `music_starts_once_and_survives_focus` | session/focus/menu | one StartMusic |
| `audio_events_do_not_change_world_hash` | same commands, fake/null sink | equal hash |

## Impl steps

- [ ] 1. Add `rts_feedback.rs` tests + fixed fake sink.
- [ ] 2. Implement volume validation conversion/effective basis points.
- [ ] 3. Add selection before/after delta derivation.
- [ ] 4. Convert T2/T5 receipts to sorted capped voice batch + reject.
- [ ] 5. Emit UI events from T12/T13 accepted pointer actions.
- [ ] 6. Queue one StartMusic + maintain hook each rendered frame.
- [ ] 7. Connect settings gain hook + rollback error.
- [ ] 8. Add offscreen trace counters to test observation seam; do not add audio to world hash.

## Outputs

- New: `src/rts_feedback.rs`.
- Modified: main/run/ui/settings + app unit/CLI tests.
- App API: event/bus/sink types above.
- Behavior: deterministic semantic audio trace/fake sink.
- Config: consumes existing volume fields; no schema change.

## Validation

- [ ] `cargo test -p millions_must_die --locked rts_feedback`
- [ ] `cargo test -p millions_must_die --locked --test rts_cli_contract audio_events_`
- [ ] `cargo check --workspace --all-targets --all-features --locked`
- [ ] app functional: offscreen script emits StartMusic once; world hash matches null-sink run
- [ ] manual check: none; no physical playback yet
- [ ] commit msg draft: `feat(app): derive deterministic RTS audio feedback from accepted actions`
