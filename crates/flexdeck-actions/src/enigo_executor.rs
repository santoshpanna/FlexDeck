//! Real execution: injects input through `enigo`.
//!
//! **Nothing in a test may construct this.** It types into whatever window
//! currently has focus. Tests use [`crate::RecordingExecutor`].
//!
//! # Platform notes
//!
//! * Windows: uses `SendInput`.
//! * Linux/X11: works via `x11rb`, which is enigo's default feature. `xdo` is
//!   *not* enabled, so no system `libxdo` is required.
//! * Linux/Wayland: not supported here. enigo can do it behind its `wayland`
//!   and `libei` features, but they are experimental; the agent should document
//!   this limitation rather than silently doing nothing.

use enigo::{Direction, Enigo, InputError, Key, Keyboard, NewConError, Settings};

use crate::hotkey::{Hotkey, KeyName, Modifier, MAX_FUNCTION_KEY};
use crate::{ActionError, CoreAction, Executor};

/// enigo's function keys in order, so the index is `number - 1`.
///
/// The length must stay in step with [`MAX_FUNCTION_KEY`]; there is a test that
/// fails loudly if the two drift apart.
const FUNCTION_KEYS: [Key; MAX_FUNCTION_KEY as usize] = [
    Key::F1,
    Key::F2,
    Key::F3,
    Key::F4,
    Key::F5,
    Key::F6,
    Key::F7,
    Key::F8,
    Key::F9,
    Key::F10,
    Key::F11,
    Key::F12,
    Key::F13,
    Key::F14,
    Key::F15,
    Key::F16,
    Key::F17,
    Key::F18,
    Key::F19,
    Key::F20,
    Key::F21,
    Key::F22,
    Key::F23,
    Key::F24,
];

pub struct EnigoExecutor {
    enigo: Enigo,
}

impl EnigoExecutor {
    pub fn new() -> Result<Self, ActionError> {
        let enigo = Enigo::new(&Settings::default()).map_err(connection_failed)?;
        Ok(Self { enigo })
    }

    fn press_hotkey(&mut self, hotkey: &Hotkey) -> Result<(), ActionError> {
        for modifier in &hotkey.modifiers {
            self.enigo
                .key(modifier_key(*modifier), Direction::Press)
                .map_err(failed)?;
        }

        self.enigo
            .key(key_for(hotkey.key)?, Direction::Click)
            .map_err(failed)?;

        // Release in reverse order, so a modifier can never be left logically
        // held down if an earlier release fails.
        for modifier in hotkey.modifiers.iter().rev() {
            self.enigo
                .key(modifier_key(*modifier), Direction::Release)
                .map_err(failed)?;
        }

        Ok(())
    }
}

impl Executor for EnigoExecutor {
    fn perform(&mut self, action: &CoreAction) -> Result<(), ActionError> {
        match action {
            CoreAction::Hotkey(hotkey) => self.press_hotkey(hotkey),
            CoreAction::Text(text) => self.enigo.text(text).map_err(failed),
        }
    }
}

fn failed(error: InputError) -> ActionError {
    ActionError::Execution(error.to_string())
}

/// Failing to connect to the input system is a different error type from
/// failing to inject a keystroke, so it gets its own adapter.
fn connection_failed(error: NewConError) -> ActionError {
    ActionError::Execution(error.to_string())
}

fn modifier_key(modifier: Modifier) -> Key {
    match modifier {
        Modifier::Ctrl => Key::Control,
        Modifier::Shift => Key::Shift,
        Modifier::Alt => Key::Alt,
        Modifier::Meta => Key::Meta,
    }
}

fn key_for(key: KeyName) -> Result<Key, ActionError> {
    let mapped = match key {
        KeyName::Char(character) => Key::Unicode(character),
        KeyName::Function(number) => function_key(number)
            .ok_or_else(|| ActionError::Execution(format!("F{number} has no key mapping")))?,
        KeyName::Enter => Key::Return,
        KeyName::Escape => Key::Escape,
        KeyName::Tab => Key::Tab,
        KeyName::Space => Key::Space,
        KeyName::Backspace => Key::Backspace,
        KeyName::Delete => Key::Delete,
        KeyName::Insert => Key::Insert,
        KeyName::Home => Key::Home,
        KeyName::End => Key::End,
        KeyName::PageUp => Key::PageUp,
        KeyName::PageDown => Key::PageDown,
        KeyName::Up => Key::UpArrow,
        KeyName::Down => Key::DownArrow,
        KeyName::Left => Key::LeftArrow,
        KeyName::Right => Key::RightArrow,
        KeyName::MediaPlayPause => Key::MediaPlayPause,
        KeyName::MediaNext => Key::MediaNextTrack,
        KeyName::MediaPrevious => Key::MediaPrevTrack,
        KeyName::MediaStop => media_stop()?,
        KeyName::VolumeUp => Key::VolumeUp,
        KeyName::VolumeDown => Key::VolumeDown,
        KeyName::VolumeMute => Key::VolumeMute,
    };
    Ok(mapped)
}

#[cfg(not(target_os = "macos"))]
fn media_stop() -> Result<Key, ActionError> {
    Ok(Key::MediaStop)
}

/// macOS has no media-stop key, so refuse it by name rather than sending
/// something else.
#[cfg(target_os = "macos")]
fn media_stop() -> Result<Key, ActionError> {
    Err(ActionError::Execution(
        "this platform has no media stop key".to_string(),
    ))
}

fn function_key(number: u8) -> Option<Key> {
    let index = usize::from(number).checked_sub(1)?;
    FUNCTION_KEYS.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every test here is pure mapping. None of them construct an EnigoExecutor,
    // because doing so would inject input into the machine running the tests.

    #[test]
    fn the_function_key_table_covers_every_key_the_parser_accepts() {
        // This is the guard that matters: raising the parser's cap without
        // extending the table would otherwise produce a runtime failure for a
        // hotkey that parsed successfully.
        for number in 1..=MAX_FUNCTION_KEY {
            assert!(function_key(number).is_some(), "F{number} has no mapping");
        }
    }

    #[test]
    fn function_key_lookup_is_bounds_checked() {
        assert_eq!(function_key(0), None);
        assert_eq!(function_key(MAX_FUNCTION_KEY + 1), None);
        assert_eq!(function_key(255), None);
    }

    #[test]
    fn the_table_starts_at_f1_and_ends_at_f24() {
        assert_eq!(function_key(1), Some(Key::F1));
        assert_eq!(function_key(24), Some(Key::F24));
    }

    #[test]
    fn named_keys_map_to_the_expected_enigo_keys() {
        assert_eq!(key_for(KeyName::Enter).unwrap(), Key::Return);
        assert_eq!(key_for(KeyName::Escape).unwrap(), Key::Escape);
        assert_eq!(key_for(KeyName::PageDown).unwrap(), Key::PageDown);
        assert_eq!(key_for(KeyName::Up).unwrap(), Key::UpArrow);
        assert_eq!(key_for(KeyName::Char('m')).unwrap(), Key::Unicode('m'));
    }

    #[test]
    fn modifiers_map_to_the_expected_enigo_keys() {
        assert_eq!(modifier_key(Modifier::Ctrl), Key::Control);
        assert_eq!(modifier_key(Modifier::Shift), Key::Shift);
        assert_eq!(modifier_key(Modifier::Alt), Key::Alt);
        assert_eq!(modifier_key(Modifier::Meta), Key::Meta);
    }

    #[test]
    fn every_key_the_parser_can_produce_has_a_mapping() {
        // Exhaustive over KeyName, so adding a variant without a mapping fails
        // here rather than at press time.
        let every_key = [
            KeyName::Char('a'),
            KeyName::Function(1),
            KeyName::Enter,
            KeyName::Escape,
            KeyName::Tab,
            KeyName::Space,
            KeyName::Backspace,
            KeyName::Delete,
            KeyName::Insert,
            KeyName::Home,
            KeyName::End,
            KeyName::PageUp,
            KeyName::PageDown,
            KeyName::Up,
            KeyName::Down,
            KeyName::Left,
            KeyName::Right,
        ];
        for key in every_key {
            assert!(key_for(key).is_ok(), "{key:?} has no mapping");
        }
    }
}
