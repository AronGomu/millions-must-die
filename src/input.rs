//! SDL key → runtime bound actions.

use mmd_engine::runtime::{BoundKey, InputAction, action_for_key};
use sdl3::keyboard::Keycode;

/// The bound keys, their SDL keycode, and the name `--inject-input` accepts.
///
/// One table so the three views of a binding cannot drift: adding a key means
/// adding a row, not editing a match, an inverse match, and a help string.
const BINDINGS: &[(BoundKey, Keycode, &str)] = &[
    (BoundKey::Escape, Keycode::Escape, "esc"),
    (BoundKey::F1, Keycode::F1, "f1"),
    (BoundKey::Space, Keycode::Space, "space"),
    (BoundKey::H, Keycode::H, "h"),
];

/// Map SDL keycode to runtime action (stable Esc/F1/Space/H contract).
pub fn action_from_keycode(key: Keycode) -> Option<InputAction> {
    BINDINGS
        .iter()
        .find(|(_, keycode, _)| *keycode == key)
        .map(|(bound, _, _)| action_for_key(*bound))
}

/// Map a key *name* to the same bound key the live keycode path produces.
///
/// This is the headless half of the binding (`--inject-input 4:space`). It
/// resolves to [`BoundKey`] rather than straight to an [`InputAction`] on
/// purpose: a scripted press then travels through the exact
/// [`action_for_key`] mapping a real key press does.
///
/// Note what that does *not* cover: the `Keycode` column above is reachable
/// only from a live keyboard, so `keyboard_and_script_agree` below pins the two
/// columns to each other. Without it a swapped keycode would leave every
/// scripted test green while the real keyboard did the wrong thing.
pub fn bound_key_from_name(name: &str) -> Option<BoundKey> {
    let name = name.to_ascii_lowercase();
    BINDINGS
        .iter()
        .find(|(_, _, key_name)| *key_name == name)
        .map(|(bound, _, _)| *bound)
}

/// The name `--inject-input` uses for a bound key, for error messages.
pub fn name_of_bound_key(key: BoundKey) -> &'static str {
    BINDINGS
        .iter()
        .find(|(bound, _, _)| *bound == key)
        .map(|(_, _, name)| *name)
        .unwrap_or("?")
}

/// Key names accepted by `--inject-input`, for error messages and `--help`.
pub fn key_names() -> String {
    BINDINGS
        .iter()
        .map(|(_, _, name)| *name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keyboard and the injection script must resolve to the same action
    /// for the same binding.
    ///
    /// Only the script half is reachable from an integration test — nothing in
    /// this repo can synthesise a real key press — so a keycode swapped in the
    /// table would otherwise ship with the whole suite green: `run
    /// --inject-input 3:space` would still pause while pressing Space quit.
    #[test]
    fn keyboard_and_script_agree() {
        for (bound, keycode, name) in BINDINGS {
            assert_eq!(
                action_from_keycode(*keycode),
                bound_key_from_name(name).map(action_for_key),
                "the keyboard and --inject-input disagree on `{name}`"
            );
            assert_eq!(bound_key_from_name(name), Some(*bound));
            assert_eq!(name_of_bound_key(*bound), *name);
        }
    }

    #[test]
    fn each_binding_is_a_distinct_action() {
        // Two keys mapped to one action would make a test that presses the
        // "wrong" one still look like it worked.
        let actions: Vec<InputAction> = BINDINGS
            .iter()
            .map(|(bound, _, _)| action_for_key(*bound))
            .collect();
        for (i, a) in actions.iter().enumerate() {
            assert!(
                !actions[i + 1..].contains(a),
                "two bindings resolve to {a:?}"
            );
        }
    }

    #[test]
    fn unbound_keys_and_names_are_rejected() {
        assert_eq!(action_from_keycode(Keycode::A), None);
        assert_eq!(bound_key_from_name("f9"), None);
        assert_eq!(bound_key_from_name(""), None);
        // Names are case-insensitive; the table is the lowercase form.
        assert_eq!(bound_key_from_name("SPACE"), Some(BoundKey::Space));
        // The literal `--inject-input` spelling of the hitbox toggle. The
        // loop above proves the table is self-consistent whatever it says;
        // this pins what it actually says, so renaming the key would break
        // `--inject-input N:h` loudly instead of silently.
        assert_eq!(bound_key_from_name("h"), Some(BoundKey::H));
        assert_eq!(bound_key_from_name("H"), Some(BoundKey::H));
    }

    #[test]
    fn key_names_lists_every_binding() {
        let listed = key_names();
        for (_, _, name) in BINDINGS {
            assert!(listed.contains(name), "`{name}` missing from `{listed}`");
        }
    }
}
