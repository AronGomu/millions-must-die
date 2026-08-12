# T7: Add persistent validated settings

**Plan:** `./ai_artefacts/PLAN_2026_08_10_rts-interaction-ui-audio-hardening.md`  
**Depends:** T1  
**Commit outcome:** versioned per-user RTS settings load/save with exact defaults/ranges; malformed config warns + defaults; offscreen runs touch no user files.

## Context (self-contained)

- Goal: one settings owner for camera/window/focus/pointer/audio; later UI only edits this model.
- This slice: schema, validation, recoverable persistence, offscreen isolation. No controls yet.
- Out of scope here: window application, camera behavior, menu rendering, physical audio.
- Assumptions: missing/malformed/unsupported cfg never bricks game; warning is visible; accepted edits persist immediately in T13; offscreen deterministic tests always use defaults.
- Decision: `docs/ADR/018_ADR_settings_window_canvas_and_camera.md`.

## Requirements

- Add root runtime `serde.workspace = true` in `Cargo.toml`; keep `serde_json`.
- Create `src/rts_settings.rs`:
  ```rust
  pub const SETTINGS_SCHEMA_VERSION: u32 = 1;
  pub const SETTINGS_ORG: &str = "AronGomu";
  pub const SETTINGS_APP: &str = "MillionsMustDie";
  pub const SETTINGS_FILE: &str = "settings-v1.json";

  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub enum WindowMode { BorderlessDesktop, Exclusive1920x1080, Windowed1280x720 }

  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub struct RtsSettings { /* schema_version, display, camera, gameplay, audio */ }

  pub struct SettingsStore { path: PathBuf }
  pub struct SettingsLoad { pub value: RtsSettings, pub warning: Option<String> }
  ```
- Fields/defaults:
  - display mode `BorderlessDesktop`; `confine_pointer=true`;
  - keyboard + edge pan = 48;
  - `pause_on_focus_loss=false`;
  - master 80, music 35, voice 70, SFX 60.
- Validation: pan `6..=96` and `%6==0`; volume `0..=100` and `%5==0`; schema exactly 1.
- `SettingsStore::pref_path()` = `sdl3::filesystem::get_pref_path(SETTINGS_ORG, SETTINGS_APP)?.join(SETTINGS_FILE)`.
- `SettingsStore::at(path)` test seam; no env/global override.
- `load`: missing target → defaults/no warning. Malformed, bad schema/range → defaults + warning containing path and reason. If target missing but `.bak` exists, restore/load backup.
- `save`: canonical pretty JSON ending newline. Same-dir recovery protocol: write + `sync_all` `.tmp`; rename target→`.bak`; rename tmp→target; on failure restore backup; delete backup only after success. Parent dir created once.
- `run`: detect `SDL_VIDEODRIVER=offscreen` before settings lookup; use `RtsSettings::default`; do not create pref dir/file.
- Add `RtsOptions::settings_store: Option<SettingsStore>` only as internal/test injection; clap surface unchanged.
- Print one `rts: settings warning=<escaped>` line on fallback; never silently reset.

## Inputs

- `src/rts_run.rs::run`, `RtsOptions`.
- `src/main.rs` module wiring.
- `Cargo.toml` workspace deps.
- **From Depends:** T1 phase constants only; no runtime API dependency.

## TDD

1. **Red** — defaults, every boundary/step, malformed/schema, backup recovery, write rollback, offscreen no-I/O tests.
2. **Green** — model/store; load once before interactive window init.
3. **Refactor** — centralize validation; one canonical JSON serializer; no setting-specific files.

## Test plan

| Test | Input | Expect |
| --- | --- | --- |
| `defaults_match_phase_1_1_contract` | `default()` | exact 48/48, modes/toggles, 80/35/70/60 |
| `settings_validate_steps_and_bounds` | legal + 5/97 pan, 3/101 volume | legal accepted; invalid rejected |
| `missing_file_loads_defaults` | absent temp path | defaults, no warning |
| `malformed_file_warns_and_uses_defaults` | bad JSON | defaults + path/reason warning |
| `unsupported_schema_warns` | schema 2 | defaults + warning |
| `save_load_round_trip_is_canonical` | nondefault legal cfg | equal value; stable bytes |
| `failed_replace_restores_backup` | injected rename failure seam | old target preserved |
| `offscreen_run_does_not_touch_settings` | temp pref sentinel | unchanged/no new file |

## Impl steps

- [x] 1. Add `serde` root dep + module test skeleton.
- [x] 2. Write red model validation/default tests.
- [x] 3. Write red temp-dir load/save/recovery tests.
- [x] 4. Implement `WindowMode`, nested settings structs, `Default`, `validate`.
- [x] 5. Implement pref path + recoverable store protocol.
- [x] 6. Load settings in interactive RTS path; bypass in offscreen path.
- [x] 7. Add warning stdout contract + CLI no-I/O test.

## Outputs

- New: `src/rts_settings.rs`.
- Modified: `Cargo.toml`, `src/main.rs`, `src/rts_run.rs`, app tests.
- Public app API: settings/store types above.
- Behavior: validated per-user cfg with fallback warning; deterministic offscreen defaults.
- Config: `${SDL pref path}/settings-v1.json`, schema 1.

## Validation

- [x] `cargo test -p millions_must_die --locked rts_settings`
- [x] `cargo test -p millions_must_die --locked --test rts_cli_contract offscreen_settings`
- [x] `cargo check --workspace --all-targets --all-features --locked`
- [x] manual check: launch, create cfg via test helper, relaunch → loaded values printed in debug overlay
- [x] app functional: `SDL_VIDEODRIVER=offscreen cargo run -- rts --frames 3`
- [x] commit msg draft: `feat(app): persist validated RTS settings outside deterministic runs`
