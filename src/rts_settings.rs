//! Persistent, validated per-user RTS settings (T7).
//!
//! One settings owner for camera/window/focus/pointer/audio. This module
//! provides schema, validation, and recoverable disk persistence only — no
//! controls yet (T13 wires editable UI to this model, later slices apply
//! `display`/`camera` to window/camera behavior and `audio` to real sound).
//!
//! # Isolation contract
//!
//! [`SettingsStore::pref_path`] is the *only* place this module touches the
//! real per-user config location (`SDL_GetPrefPath`, which creates the pref
//! directory as a side effect of being called). [`crate::rts_run::run`] must
//! not call it under any non-interactive `SDL_VIDEODRIVER` — `offscreen` and
//! `dummy` alike — those runs always use [`RtsSettings::default`] and never
//! resolve, read, or write that path. [`SettingsStore::at`] is a test seam so
//! unit tests never call `pref_path` either; they always point a store at a
//! temp-dir path.
//!
//! # Recoverable persistence
//!
//! [`SettingsStore::save`] never leaves the target file half-written: it
//! writes a sibling `.tmp` file and `sync_all`s it, renames the current
//! target (if any) to a sibling `.bak`, then renames `.tmp` onto the target.
//! A failure at the final rename restores the `.bak` before returning the
//! error, so the previous valid settings file always survives a failed
//! save. The `.bak` is deleted only after the final rename succeeds.
//! [`SettingsStore::load`] falls back to that same `.bak` when the target is
//! missing, and falls back to [`RtsSettings::default`] (plus a warning) on
//! any malformed file, unsupported schema, or out-of-range value — a bad
//! config file must never brick the game.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Only schema version this build accepts. A settings file from a different
/// build (older or newer) is treated as unsupported, never partially read.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;
pub const SETTINGS_ORG: &str = "AronGomu";
pub const SETTINGS_APP: &str = "MillionsMustDie";
pub const SETTINGS_FILE: &str = "settings-v1.json";

const PAN_MIN: u32 = 6;
const PAN_MAX: u32 = 96;
const PAN_STEP: u32 = 6;
const VOLUME_MIN: u32 = 0;
const VOLUME_MAX: u32 = 100;
const VOLUME_STEP: u32 = 5;

/// Window presentation mode. Applying this to the real window is out of
/// scope for T7; this is schema + validation only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    BorderlessDesktop,
    Exclusive1920x1080,
    Windowed1280x720,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplaySettings {
    pub mode: WindowMode,
    pub confine_pointer: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            mode: WindowMode::BorderlessDesktop,
            confine_pointer: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraSettings {
    pub keyboard_pan: u32,
    pub edge_pan: u32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            keyboard_pan: 48,
            edge_pan: 48,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameplaySettings {
    pub pause_on_focus_loss: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioSettings {
    pub master: u32,
    pub music: u32,
    pub voice: u32,
    pub sfx: u32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master: 80,
            music: 35,
            voice: 70,
            sfx: 60,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RtsSettings {
    pub schema_version: u32,
    pub display: DisplaySettings,
    pub camera: CameraSettings,
    pub gameplay: GameplaySettings,
    pub audio: AudioSettings,
}

impl Default for RtsSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            display: DisplaySettings::default(),
            camera: CameraSettings::default(),
            gameplay: GameplaySettings::default(),
            audio: AudioSettings::default(),
        }
    }
}

fn in_step(v: u32, lo: u32, hi: u32, step: u32) -> bool {
    v >= lo && v <= hi && v.is_multiple_of(step)
}

impl RtsSettings {
    /// Validates schema version and every bounded field. `Err` names the
    /// field, value, and legal range/step — the text a load-fallback
    /// warning or a save rejection reports.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SETTINGS_SCHEMA_VERSION {
            return Err(format!(
                "unsupported schema_version {} (this build reads {SETTINGS_SCHEMA_VERSION})",
                self.schema_version
            ));
        }
        if !in_step(self.camera.keyboard_pan, PAN_MIN, PAN_MAX, PAN_STEP) {
            return Err(format!(
                "camera.keyboard_pan {} must be {PAN_MIN}..={PAN_MAX} in steps of {PAN_STEP}",
                self.camera.keyboard_pan
            ));
        }
        if !in_step(self.camera.edge_pan, PAN_MIN, PAN_MAX, PAN_STEP) {
            return Err(format!(
                "camera.edge_pan {} must be {PAN_MIN}..={PAN_MAX} in steps of {PAN_STEP}",
                self.camera.edge_pan
            ));
        }
        for (name, v) in [
            ("audio.master", self.audio.master),
            ("audio.music", self.audio.music),
            ("audio.voice", self.audio.voice),
            ("audio.sfx", self.audio.sfx),
        ] {
            if !in_step(v, VOLUME_MIN, VOLUME_MAX, VOLUME_STEP) {
                return Err(format!(
                    "{name} {v} must be {VOLUME_MIN}..={VOLUME_MAX} in steps of {VOLUME_STEP}"
                ));
            }
        }
        Ok(())
    }

    /// One debug line for the manual "loaded values are visible" check.
    /// Not part of the frame-loop HUD contract (`rts_overlay`).
    pub fn debug_line(&self) -> String {
        format!(
            "rts: settings mode={:?} confine_pointer={} keyboard_pan={} edge_pan={} \
             pause_on_focus_loss={} master={} music={} voice={} sfx={}",
            self.display.mode,
            self.display.confine_pointer,
            self.camera.keyboard_pan,
            self.camera.edge_pan,
            self.gameplay.pause_on_focus_loss,
            self.audio.master,
            self.audio.music,
            self.audio.voice,
            self.audio.sfx,
        )
    }
}

/// Result of [`SettingsStore::load`]: the value to use, plus a human-readable
/// warning when it is not the caller's own on-disk data.
pub struct SettingsLoad {
    pub value: RtsSettings,
    pub warning: Option<String>,
}

/// Escapes a warning message to one whitespace-free token, so the
/// `rts: settings warning=<escaped>` line stays a single space-separated
/// `key=value` per the rest of this binary's stdout contract.
pub fn escape_warning(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .collect()
}

fn append_ext(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

fn read_and_validate(path: &Path) -> Result<RtsSettings, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let value: RtsSettings = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    value.validate()?;
    Ok(value)
}

#[cfg_attr(not(test), allow(dead_code))]
fn canonical_json(settings: &RtsSettings) -> Result<String, String> {
    let mut text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    text.push('\n');
    Ok(text)
}

#[cfg(test)]
thread_local! {
    /// Test-only fault injector for [`SettingsStore::save`]'s final rename.
    /// `thread_local` rather than a shared global: `cargo test` runs cases
    /// on separate threads, so this never leaks between tests.
    static FAIL_FINAL_RENAME: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn set_fail_final_rename(fail: bool) {
    FAIL_FINAL_RENAME.with(|f| f.set(fail));
}

/// Owns one settings file path and its recoverable load/save protocol.
#[derive(Clone, Debug)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    /// The real per-user settings path:
    /// `${SDL pref path}/settings-v1.json`.
    ///
    /// Calling this resolves (and, per `SDL_GetPrefPath`, creates) the real
    /// OS pref directory — callers must not reach it from a non-interactive or
    /// deterministic run. Use [`Self::at`] for tests.
    pub fn pref_path() -> Result<PathBuf, String> {
        sdl3::filesystem::get_pref_path(SETTINGS_ORG, SETTINGS_APP)
            .map(|dir| dir.join(SETTINGS_FILE))
            .map_err(|e| e.to_string())
    }

    /// Test/internal-injection seam: a store pointed at an explicit path,
    /// bypassing `pref_path`'s env/global lookup entirely.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    fn bak_path(&self) -> PathBuf {
        append_ext(&self.path, "bak")
    }

    // Not yet called outside `save`/tests: `#[allow(dead_code)]` in non-test
    // builds rather than dropping it — `save` is part of this ticket's public
    // store API (T13 wires the caller that persists edited settings).
    #[cfg_attr(not(test), allow(dead_code))]
    fn tmp_path(&self) -> PathBuf {
        append_ext(&self.path, "tmp")
    }

    /// Loads validated settings, falling back through: target file →
    /// `.bak` → [`RtsSettings::default`]. Any fallback past the target file
    /// carries a warning naming the file and the reason; a plain missing
    /// target (no `.bak` either) is silent — that is the expected first run.
    pub fn load(&self) -> SettingsLoad {
        if self.path.is_file() {
            return match read_and_validate(&self.path) {
                Ok(value) => SettingsLoad {
                    value,
                    warning: None,
                },
                Err(reason) => SettingsLoad {
                    value: RtsSettings::default(),
                    warning: Some(format!("{}: {reason}", self.path.display())),
                },
            };
        }

        let bak = self.bak_path();
        if bak.is_file() {
            return match read_and_validate(&bak) {
                Ok(value) => SettingsLoad {
                    value,
                    warning: None,
                },
                Err(reason) => SettingsLoad {
                    value: RtsSettings::default(),
                    warning: Some(format!("{}: {reason}", bak.display())),
                },
            };
        }

        SettingsLoad {
            value: RtsSettings::default(),
            warning: None,
        }
    }

    /// Writes `settings` via the recoverable same-dir replace protocol
    /// described on the module. Rejects an invalid value before touching
    /// disk at all.
    ///
    /// Unused outside tests in this ticket (no controls yet to edit and
    /// persist a setting) — kept public and exercised by the round-trip and
    /// recovery tests, T13 wires the interactive caller.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn save(&self, settings: &RtsSettings) -> Result<(), String> {
        settings
            .validate()
            .map_err(|e| format!("refusing to save invalid settings: {e}"))?;

        let dir = self
            .path
            .parent()
            .ok_or_else(|| "settings path has no parent directory".to_string())?;
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;

        let json = canonical_json(settings)?;
        let tmp = self.tmp_path();
        let bak = self.bak_path();

        {
            let mut f = File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
            f.write_all(json.as_bytes())
                .map_err(|e| format!("{}: {e}", tmp.display()))?;
            f.sync_all()
                .map_err(|e| format!("{}: {e}", tmp.display()))?;
        }

        let had_target = self.path.is_file();
        if had_target {
            fs::rename(&self.path, &bak).map_err(|e| format!("{}: {e}", bak.display()))?;
        }

        #[cfg(test)]
        if FAIL_FINAL_RENAME.with(|f| f.get()) {
            if had_target {
                let _ = fs::rename(&bak, &self.path);
            }
            return Err("injected final-rename failure (test seam)".to_string());
        }

        match fs::rename(&tmp, &self.path) {
            Ok(()) => {
                if had_target {
                    let _ = fs::remove_file(&bak);
                }
                Ok(())
            }
            Err(e) => {
                if had_target {
                    let _ = fs::rename(&bak, &self.path);
                }
                Err(format!("{}: {e}", self.path.display()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_phase_1_1_contract() {
        let d = RtsSettings::default();
        assert_eq!(d.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(d.display.mode, WindowMode::BorderlessDesktop);
        assert!(d.display.confine_pointer);
        assert_eq!(d.camera.keyboard_pan, 48);
        assert_eq!(d.camera.edge_pan, 48);
        assert!(!d.gameplay.pause_on_focus_loss);
        assert_eq!(d.audio.master, 80);
        assert_eq!(d.audio.music, 35);
        assert_eq!(d.audio.voice, 70);
        assert_eq!(d.audio.sfx, 60);
        assert!(d.validate().is_ok(), "defaults must themselves validate");
    }

    #[test]
    fn settings_validate_steps_and_bounds() {
        let mut s = RtsSettings::default();
        assert!(s.validate().is_ok());

        s.camera.keyboard_pan = 5;
        assert!(s.validate().is_err(), "5 is not a multiple of 6");
        s.camera.keyboard_pan = 97;
        assert!(s.validate().is_err(), "97 > 96");
        s.camera.keyboard_pan = 6;
        assert!(s.validate().is_ok(), "6 is the low boundary");
        s.camera.keyboard_pan = 96;
        assert!(s.validate().is_ok(), "96 is the high boundary");
        s.camera.keyboard_pan = 48;

        s.camera.edge_pan = 100;
        assert!(s.validate().is_err(), "100 is not a multiple of 6 nor <=96");
        s.camera.edge_pan = 48;

        s.audio.master = 3;
        assert!(s.validate().is_err(), "3 is not a multiple of 5");
        s.audio.master = 101;
        assert!(s.validate().is_err(), "101 > 100");
        s.audio.master = 0;
        assert!(s.validate().is_ok(), "0 is the low boundary");
        s.audio.master = 100;
        assert!(s.validate().is_ok(), "100 is the high boundary");

        s.schema_version = 2;
        assert!(s.validate().is_err(), "only schema 1 is supported");
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SettingsStore::at(dir.path().join(SETTINGS_FILE));

        let loaded = store.load();

        assert_eq!(loaded.value, RtsSettings::default());
        assert!(loaded.warning.is_none(), "first run must not warn");
    }

    #[test]
    fn malformed_file_warns_and_uses_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(SETTINGS_FILE);
        fs::write(&path, "not json").expect("write malformed file");
        let store = SettingsStore::at(path.clone());

        let loaded = store.load();

        assert_eq!(loaded.value, RtsSettings::default());
        let warning = loaded.warning.expect("malformed file must warn");
        assert!(warning.contains(&path.display().to_string()));
    }

    #[test]
    fn unsupported_schema_warns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(SETTINGS_FILE);
        let bad = RtsSettings {
            schema_version: 2,
            ..RtsSettings::default()
        };
        fs::write(&path, serde_json::to_string(&bad).unwrap()).expect("write bad-schema file");
        let store = SettingsStore::at(path.clone());

        let loaded = store.load();

        assert_eq!(loaded.value, RtsSettings::default());
        let warning = loaded.warning.expect("unsupported schema must warn");
        assert!(warning.contains(&path.display().to_string()));
        assert!(warning.contains("schema_version"));
    }

    #[test]
    fn save_load_round_trip_is_canonical() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(SETTINGS_FILE);
        let store = SettingsStore::at(path.clone());
        let mut settings = RtsSettings::default();
        settings.audio.master = 55;
        settings.camera.keyboard_pan = 24;

        store.save(&settings).expect("first save");
        let bytes1 = fs::read(&path).expect("read after first save");
        assert!(
            bytes1.ends_with(b"\n"),
            "canonical JSON ends with a newline"
        );

        let loaded = store.load();
        assert_eq!(loaded.value, settings);
        assert!(loaded.warning.is_none());

        store
            .save(&settings)
            .expect("second save of an identical value");
        let bytes2 = fs::read(&path).expect("read after second save");
        assert_eq!(
            bytes1, bytes2,
            "identical value must serialize to stable bytes"
        );
    }

    #[test]
    fn failed_replace_restores_backup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(SETTINGS_FILE);
        let store = SettingsStore::at(path.clone());
        let original = RtsSettings::default();
        store.save(&original).expect("seed original settings");

        let mut changed = original.clone();
        changed.audio.master = 55;

        set_fail_final_rename(true);
        let result = store.save(&changed);
        set_fail_final_rename(false);

        assert!(result.is_err(), "injected failure must surface as an error");
        assert!(path.is_file(), "target must survive a failed replace");
        let loaded = store.load();
        assert_eq!(
            loaded.value, original,
            "old target content must survive a failed replace"
        );
        assert!(
            !store.bak_path().is_file(),
            "backup must not linger after a restore"
        );
    }

    #[test]
    fn offscreen_run_does_not_touch_settings() {
        // Module-level proof that nothing in this file resolves the real
        // pref path unless a caller explicitly asks: `SettingsStore::at`
        // never touches `pref_path`, and constructing/using a store here
        // creates files only under `dir`, a throwaway tempdir. The CLI-level
        // proof that `rts_run::run` itself skips settings lookup under
        // `SDL_VIDEODRIVER=offscreen` lives in
        // `tests/rts_cli_contract.rs::offscreen_settings_run_does_not_touch_settings`.
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SettingsStore::at(dir.path().join(SETTINGS_FILE));
        let _ = store.load();
        let mut entries = fs::read_dir(dir.path())
            .expect("read tempdir")
            .collect::<Vec<_>>();
        assert!(
            entries.is_empty(),
            "a load of a missing file must create nothing: {entries:?}"
        );
        entries.clear();
    }
}
