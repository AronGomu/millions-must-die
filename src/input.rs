//! SDL key → runtime bound actions.

use mmd_engine::runtime::{BoundKey, InputAction, action_for_key};
use sdl3::keyboard::Keycode;

/// Map SDL keycode to runtime action (stable Esc/F1/Space contract).
pub fn action_from_keycode(key: Keycode) -> Option<InputAction> {
    let bound = match key {
        Keycode::Escape => BoundKey::Escape,
        Keycode::F1 => BoundKey::F1,
        Keycode::Space => BoundKey::Space,
        _ => return None,
    };
    Some(action_for_key(bound))
}
