//! Scripted input for RTS runs with no keyboard or mouse.
//!
//! Grammar: `FRAME:KIND[:ARGS]` entries separated by **`;`**, because
//! coordinates already use `,`. Frames are 1-based and name the frame the
//! event lands *before*, matching `--inject-input` on the `run` command.
//!
//! | KIND      | ARGS            | meaning |
//! | --------- | --------------- | ------- |
//! | `key`     | `<name>`        | one press of a bound key |
//! | `pan`     | `<name>`        | press a pan key (hold until `panup`) |
//! | `panup`   | `<name>`        | release a pan key |
//! | `move`    | `X,Y`           | move the pointer |
//! | `lclick`  | `X,Y`           | left click |
//! | `sclick`  | `X,Y`           | additive (shift) left click |
//! | `rclick`  | `X,Y`           | right click |
//! | `drag`    | `X0,Y0,X1,Y1`   | left drag |
//!
//! Example:
//! `10:move:960,540;12:lclick:960,540;60:key:w;90:lclick:1000,600;200:key:esc`
//!
//! [`RtsScript::parse_file_text`] reads the same grammar from a file, where a
//! newline also separates entries and `#` starts a comment — the form the
//! tracked acceptance script is written in.

use crate::rts_input::{self, RtsCommand};

/// One scheduled command. `name` is the short label `unfired()` reports —
/// the bound key name for `key`, the pan direction name for `pan`/`panup`,
/// else the kind word.
#[derive(Debug, Clone)]
struct Entry {
    frame: u64,
    name: String,
    cmd: RtsCommand,
    fired: bool,
}

/// Scripted input for runs with no keyboard or mouse.
#[derive(Debug, Default)]
pub struct RtsScript {
    entries: Vec<Entry>,
}

impl RtsScript {
    /// Every rejection names the offending entry — a script is typed by hand
    /// and a silent misparse would make the run prove nothing.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut entries = Vec::new();
        for raw in spec.split(';') {
            let entry = raw.trim();
            if entry.is_empty() {
                return Err(format!(
                    "--inject-input {spec:?} has an empty entry; expected \
                     FRAME:KIND[:ARGS];FRAME:KIND[:ARGS]..."
                ));
            }

            let mut parts = entry.splitn(3, ':');
            let frame_str = parts.next().unwrap_or("").trim();
            let kind_str = parts.next().ok_or_else(|| {
                format!(
                    "--inject-input entry {entry:?} is not FRAME:KIND[:ARGS] (e.g. 4:key:space)"
                )
            })?;
            let args = parts.next().ok_or_else(|| {
                format!(
                    "--inject-input entry {entry:?} is missing :ARGS (e.g. 4:key:space, \
                     4:lclick:960,540)"
                )
            })?;

            let frame: u64 = frame_str.parse().map_err(|_| {
                format!("--inject-input entry {entry:?}: frame {frame_str:?} is not a number")
            })?;
            if frame == 0 {
                return Err(format!(
                    "--inject-input entry {entry:?}: frames are 1-based, so the earliest \
                     event is frame 1"
                ));
            }

            let (name, cmd) = match kind_str.trim() {
                "key" => {
                    let key_name = args.trim().to_ascii_lowercase();
                    let cmd = rts_input::command_from_name(&key_name).ok_or_else(|| {
                        format!(
                            "--inject-input entry {entry:?}: unknown key {:?} (valid: {})",
                            args.trim(),
                            rts_input::key_names()
                        )
                    })?;
                    (key_name, cmd)
                }
                "pan" => {
                    let key_name = args.trim().to_ascii_lowercase();
                    let dir = rts_input::pan_from_name(&key_name).ok_or_else(|| {
                        format!(
                            "--inject-input entry {entry:?}: unknown pan key {:?} (valid: {})",
                            args.trim(),
                            rts_input::pan_key_names()
                        )
                    })?;
                    (key_name, RtsCommand::PanStart(dir))
                }
                "panup" => {
                    let key_name = args.trim().to_ascii_lowercase();
                    let dir = rts_input::pan_from_name(&key_name).ok_or_else(|| {
                        format!(
                            "--inject-input entry {entry:?}: unknown pan key {:?} (valid: {})",
                            args.trim(),
                            rts_input::pan_key_names()
                        )
                    })?;
                    (key_name, RtsCommand::PanStop(dir))
                }
                "move" => {
                    let c = parse_coords(entry, args, 2)?;
                    ("move".to_string(), RtsCommand::Move([c[0], c[1]]))
                }
                "lclick" => {
                    let c = parse_coords(entry, args, 2)?;
                    ("lclick".to_string(), RtsCommand::LeftClick([c[0], c[1]]))
                }
                "sclick" => {
                    let c = parse_coords(entry, args, 2)?;
                    ("sclick".to_string(), RtsCommand::ShiftClick([c[0], c[1]]))
                }
                "rclick" => {
                    let c = parse_coords(entry, args, 2)?;
                    ("rclick".to_string(), RtsCommand::RightClick([c[0], c[1]]))
                }
                "drag" => {
                    let c = parse_coords(entry, args, 4)?;
                    (
                        "drag".to_string(),
                        RtsCommand::Drag([c[0], c[1]], [c[2], c[3]]),
                    )
                }
                other => {
                    return Err(format!(
                        "--inject-input entry {entry:?}: unknown kind {other:?} (valid: key, \
                         pan, panup, move, lclick, sclick, rclick, drag)"
                    ));
                }
            };

            entries.push(Entry {
                frame,
                name,
                cmd,
                fired: false,
            });
        }
        Ok(Self { entries })
    }

    /// Strip comments, treat every newline as a `;`, then [`Self::parse`].
    ///
    /// The file form exists so an acceptance run can be *read* — a one-line
    /// `--inject-input` spec long enough to drive a whole economy loop is not
    /// reviewable. Everything from a `#` to the end of a line is a comment,
    /// blank lines are skipped, and `;` still separates entries on one line.
    ///
    /// A file with no entries at all is rejected: a script that schedules
    /// nothing would make the run it drives prove nothing.
    pub fn parse_file_text(text: &str) -> Result<Self, String> {
        let mut spec = String::with_capacity(text.len());
        for line in text.lines() {
            let code = line.split('#').next().unwrap_or("").trim();
            if code.is_empty() {
                continue;
            }
            if !spec.is_empty() {
                spec.push(';');
            }
            spec.push_str(code);
        }
        if spec.is_empty() {
            return Err(
                "the script file schedules no entries: every line is blank or a \
                 comment, so the run would prove nothing"
                    .to_string(),
            );
        }
        Self::parse(&spec)
    }

    /// Commands scheduled for `frame`, in script order, appended to `out`.
    /// Returns `true` when one of them was `Quit`; entries queued behind a
    /// `Quit` on the same frame stay unfired.
    pub fn drain_frame(&mut self, frame: u64, out: &mut Vec<RtsCommand>) -> bool {
        for entry in self
            .entries
            .iter_mut()
            .filter(|e| !e.fired && e.frame == frame)
        {
            entry.fired = true;
            out.push(entry.cmd);
            if entry.cmd == RtsCommand::Quit {
                return true;
            }
        }
        false
    }

    /// Entries the run never reached, in script order.
    pub fn unfired(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| !e.fired)
            .map(|e| format!("{}:{}", e.frame, e.name))
            .collect()
    }
}

/// Parse `args` as exactly `n` comma-separated finite, non-negative `f32`s.
fn parse_coords(entry: &str, args: &str, n: usize) -> Result<Vec<f32>, String> {
    let parts: Vec<&str> = args.split(',').map(str::trim).collect();
    if parts.len() != n {
        return Err(format!(
            "--inject-input entry {entry:?}: expected {n} comma-separated coordinate(s), got {}",
            parts.len()
        ));
    }
    parts
        .into_iter()
        .map(|p| {
            let v: f32 = p.parse().map_err(|_| {
                format!("--inject-input entry {entry:?}: coordinate {p:?} is not a number")
            })?;
            if !v.is_finite() || v < 0.0 {
                return Err(format!(
                    "--inject-input entry {entry:?}: coordinate {p:?} must be a finite, \
                     non-negative number"
                ));
            }
            Ok(v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_parses_every_kind() {
        let spec = "1:key:esc;2:pan:left;3:panup:left;4:move:1,2;5:lclick:1,2;\
                     6:sclick:1,2;7:rclick:1,2;8:drag:1,2,3,4";
        let script = RtsScript::parse(spec).expect("valid script");
        assert_eq!(script.entries.len(), 8, "expected 8 entries: {spec}");
    }

    #[test]
    fn script_rejects_frame_zero() {
        let err = RtsScript::parse("0:key:esc").unwrap_err();
        assert!(err.contains("0:key:esc"), "{err}");
        assert!(err.contains("1-based"), "{err}");
    }

    #[test]
    fn script_rejects_an_unknown_kind() {
        let err = RtsScript::parse("1:jump:1,2").unwrap_err();
        assert!(err.contains("jump"), "{err}");
    }

    #[test]
    fn script_rejects_an_unknown_key() {
        let err = RtsScript::parse("1:key:f9").unwrap_err();
        assert!(err.contains("unknown key"), "{err}");
        assert!(err.contains("esc"), "{err}\nmust list valid names");
    }

    #[test]
    fn script_rejects_bad_coordinates() {
        for spec in ["1:lclick:a,2", "1:lclick:-1,2", "1:lclick:1"] {
            assert!(
                RtsScript::parse(spec).is_err(),
                "{spec} should have been rejected"
            );
        }
    }

    #[test]
    fn script_rejects_an_empty_entry() {
        let err = RtsScript::parse("1:key:esc;;2:key:esc").unwrap_err();
        assert!(err.contains("empty entry"), "{err}");
    }

    #[test]
    fn parse_file_text_strips_comments() {
        let script =
            RtsScript::parse_file_text("1:key:esc # go\n\n# nothing\n").expect("valid script file");
        assert_eq!(script.entries.len(), 1);
        assert_eq!(script.entries[0].name, "esc");
    }

    #[test]
    fn parse_file_text_accepts_semicolons_too() {
        let script = RtsScript::parse_file_text("1:key:esc;2:key:esc").expect("valid script file");
        assert_eq!(script.entries.len(), 2);
    }

    #[test]
    fn parse_file_text_rejects_a_bad_entry() {
        let err = RtsScript::parse_file_text("1:jump:1\n").unwrap_err();
        assert!(err.contains("jump"), "{err}");
    }

    #[test]
    fn parse_file_text_rejects_a_script_with_no_entries() {
        let err = RtsScript::parse_file_text("# only a comment\n\n").unwrap_err();
        assert!(err.contains("no entries"), "{err}");
    }

    /// The committed acceptance script is the one the merge gate runs; a typo
    /// in it must fail here, not three minutes into a GPU run.
    #[test]
    fn the_tracked_script_parses() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/scenarios/rts_acceptance_v1.script");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let script =
            RtsScript::parse_file_text(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            !script.entries.is_empty(),
            "{} parsed to nothing",
            path.display()
        );
    }

    /// A `Quit` scheduled behind another entry on the same frame stops the
    /// sweep: the later entry never fires.
    #[test]
    fn quit_stops_the_frames_sweep() {
        let mut script = RtsScript::parse("3:key:esc;3:key:space").expect("valid script");
        let mut out = Vec::new();
        let quit = script.drain_frame(3, &mut out);
        assert!(quit, "esc did not report a quit");
        assert_eq!(out, vec![RtsCommand::Quit]);
        assert_eq!(script.unfired(), vec!["3:space".to_string()]);
    }
}
