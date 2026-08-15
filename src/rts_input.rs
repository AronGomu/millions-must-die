//! RTS keyboard + pan bindings: the live SDL path and the `--inject-input`
//! script path resolve through the same tables, so the two cannot drift.

use sdl3::keyboard::Keycode;

/// RTS commands, produced by both the live SDL path and the script path.
///
/// Deliberately a separate enum from `mmd_engine::runtime::InputAction`: that
/// one is pinned by `input_actions_are_stable` and belongs to the phase-0
/// horde viewer. Extending it would put RTS discriminants into a contract
/// that has nothing to do with them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RtsCommand {
    /// Script/CLI-only early termination (`quit` script token). No key binds
    /// this — `T13` repurposes Escape into the paused-menu FSM below.
    Quit,
    /// Gameplay: opens the paused one-button menu. Menu/Settings: navigates
    /// back one level. Never quits (`T13`).
    Escape,
    TogglePause,
    ToggleOverlay,
    /// Execute the command in a positional card slot (0‒8, row-major).
    /// Slot ≥ 9 is a no-op and never panics — validated in `execute_slot`.
    ExecuteSlot(u8),
    /// Begin holding a pan direction. Components in `-1..=1`, screen space.
    PanStart([f32; 2]),
    /// Stop holding it.
    PanStop([f32; 2]),
    /// Pointer moved to a screen position.
    Move([f32; 2]),
    /// Plain left click / release at a screen position.
    LeftClick([f32; 2]),
    /// Additive (shift) left click.
    ShiftClick([f32; 2]),
    /// Left drag from a to b.
    Drag([f32; 2], [f32; 2]),
    /// Right click — the context order.
    RightClick([f32; 2]),
    /// Mouse wheel scroll at logical `point` with `delta` notches (+up / -down).
    /// `point` has already been mapped through `viewport.map_pointer`.
    Wheel {
        point: [f32; 2],
        delta: i32,
    },
}

/// Bound key, its SDL keycode, the `--inject-input` name, and the command.
///
/// One table so the three views of a binding cannot drift — the same
/// discipline `src/input.rs` uses for the horde viewer.
const KEY_BINDINGS: &[(Keycode, &str, RtsCommand)] = &[
    (Keycode::Escape, "esc", RtsCommand::Escape),
    (Keycode::Space, "space", RtsCommand::TogglePause),
    (Keycode::F1, "f1", RtsCommand::ToggleOverlay),
    (Keycode::Q, "q", RtsCommand::ExecuteSlot(0)),
    (Keycode::W, "w", RtsCommand::ExecuteSlot(1)),
    (Keycode::E, "e", RtsCommand::ExecuteSlot(2)),
    (Keycode::A, "a", RtsCommand::ExecuteSlot(3)),
    (Keycode::S, "s", RtsCommand::ExecuteSlot(4)),
    (Keycode::D, "d", RtsCommand::ExecuteSlot(5)),
    (Keycode::Z, "z", RtsCommand::ExecuteSlot(6)),
    (Keycode::X, "x", RtsCommand::ExecuteSlot(7)),
    (Keycode::C, "c", RtsCommand::ExecuteSlot(8)),
];

/// Held pan keys. Arrow keys only — all letter keys are command-grid slots.
const PAN_BINDINGS: &[(Keycode, &str, [f32; 2])] = &[
    (Keycode::Left, "left", [-1.0, 0.0]),
    (Keycode::Right, "right", [1.0, 0.0]),
    (Keycode::Up, "up", [0.0, -1.0]),
    (Keycode::Down, "down", [0.0, 1.0]),
];

/// Map SDL keycode to an RTS command.
pub fn command_from_keycode(key: Keycode) -> Option<RtsCommand> {
    KEY_BINDINGS
        .iter()
        .find(|(keycode, _, _)| *keycode == key)
        .map(|(_, _, cmd)| *cmd)
}

/// Map a key *name* (the `--inject-input` spelling) to the same command the
/// live keycode path produces.
pub fn command_from_name(name: &str) -> Option<RtsCommand> {
    let name = name.to_ascii_lowercase();
    KEY_BINDINGS
        .iter()
        .find(|(_, key_name, _)| *key_name == name)
        .map(|(_, _, cmd)| *cmd)
}

/// Map SDL keycode to a held pan direction.
pub fn pan_from_keycode(key: Keycode) -> Option<[f32; 2]> {
    PAN_BINDINGS
        .iter()
        .find(|(keycode, _, _)| *keycode == key)
        .map(|(_, _, dir)| *dir)
}

/// Map a pan key *name* to the same direction the live keycode path
/// produces.
pub fn pan_from_name(name: &str) -> Option<[f32; 2]> {
    let name = name.to_ascii_lowercase();
    PAN_BINDINGS
        .iter()
        .find(|(_, key_name, _)| *key_name == name)
        .map(|(_, _, dir)| *dir)
}

/// Key names accepted by `--inject-input`'s `key:` kind, for error messages.
pub fn key_names() -> String {
    KEY_BINDINGS
        .iter()
        .map(|(_, name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Pan key names accepted by `--inject-input`'s `pan:`/`panup:` kinds, for
/// error messages.
pub fn pan_key_names() -> String {
    PAN_BINDINGS
        .iter()
        .map(|(_, name, _)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The window banner string printed once a window is claimed. Lives here so
/// `the_window_banner_lists_every_binding` can assert against the same
/// table-driven text the app prints, instead of a copy that could drift.
pub fn window_banner() -> String {
    "Esc menu, Space pause, F1 overlay, QWE/ASD/ZXC card, right-click cancel, arrows pan"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keyboard and the injection script must resolve to the same
    /// command for the same binding.
    #[test]
    fn keyboard_and_script_agree() {
        for (keycode, name, cmd) in KEY_BINDINGS {
            assert_eq!(
                command_from_keycode(*keycode),
                command_from_name(name),
                "the keyboard and --inject-input disagree on `{name}`"
            );
            assert_eq!(command_from_name(name), Some(*cmd));
        }
    }

    /// Same agreement, for the pan table.
    #[test]
    fn pan_keyboard_and_script_agree() {
        for (keycode, name, dir) in PAN_BINDINGS {
            assert_eq!(
                pan_from_keycode(*keycode),
                pan_from_name(name),
                "the keyboard and --inject-input disagree on pan `{name}`"
            );
            assert_eq!(pan_from_name(name), Some(*dir));
        }
    }

    /// Two keys mapped to one command would make a test that presses the
    /// "wrong" one still look like it worked.
    #[test]
    fn each_key_binds_one_command() {
        let cmds: Vec<RtsCommand> = KEY_BINDINGS.iter().map(|(_, _, cmd)| *cmd).collect();
        for (i, cmd) in cmds.iter().enumerate() {
            assert!(
                !cmds[i + 1..].contains(cmd),
                "two bindings resolve to {cmd:?}"
            );
        }
    }

    /// A WASD pan would silently shadow half the build menu.
    #[test]
    fn no_key_is_both_a_command_and_a_pan() {
        for (keycode, name, _) in KEY_BINDINGS {
            assert!(
                !PAN_BINDINGS.iter().any(|(pk, _, _)| pk == keycode),
                "`{keycode:?}` is both a command and a pan key"
            );
            assert!(
                !PAN_BINDINGS.iter().any(|(_, pn, _)| pn == name),
                "`{name}` is both a command name and a pan name"
            );
        }
    }

    /// All nine card slots map row-major Q/W/E A/S/D Z/X/C.
    #[test]
    fn all_nine_command_keys_map_row_major() {
        let expected: &[(&str, u8)] = &[
            ("q", 0),
            ("w", 1),
            ("e", 2),
            ("a", 3),
            ("s", 4),
            ("d", 5),
            ("z", 6),
            ("x", 7),
            ("c", 8),
        ];
        for &(name, slot) in expected {
            assert_eq!(
                command_from_name(name),
                Some(RtsCommand::ExecuteSlot(slot)),
                "key `{name}` should map to slot {slot}"
            );
        }
    }

    /// X maps to slot 7 — it no longer cancels placement.
    #[test]
    fn x_executes_slot_seven() {
        assert_eq!(command_from_name("x"), Some(RtsCommand::ExecuteSlot(7)));
    }

    /// R is not bound to any command.
    #[test]
    fn r_is_unbound() {
        assert_eq!(command_from_keycode(Keycode::R), None);
        assert_eq!(command_from_name("r"), None);
    }

    #[test]
    fn unbound_keys_are_rejected() {
        assert_eq!(command_from_keycode(Keycode::R), None);
        assert_eq!(command_from_name("f9"), None);
        assert_eq!(command_from_name(""), None);
    }

    /// The banner printed on window claim is a human's only guide to the
    /// bindings; if it drifts from the table, the game lies about its own
    /// controls.
    #[test]
    fn the_window_banner_lists_every_binding() {
        let banner = window_banner().to_ascii_lowercase();
        for (_, name, _) in KEY_BINDINGS {
            assert!(
                banner.contains(name),
                "`{name}` missing from window banner: {banner}"
            );
        }
        assert!(
            banner.contains("arrows"),
            "window banner does not mention `arrows`: {banner}"
        );
    }
}
