//! Keyboard accelerator parsing.
//!
//! Buttons in the config store a shortcut as a string, because that is what a
//! human types into the property panel: `"ctrl+shift+m"`. Turning that into
//! something executable is real logic, and it is where the interesting edge
//! cases live — so it is pure, and thoroughly tested.
//!
//! # Accepted forms
//!
//! | Input | Meaning |
//! | --- | --- |
//! | `ctrl+shift+m` | Ctrl, Shift and the `m` key |
//! | `ctrl++` | Ctrl and the `+` key |
//! | `f5` | the F5 key |
//! | `pgdn` | Page Down |
//! | `m` | the `m` key, no modifiers |
//!
//! Matching is case-insensitive: `Ctrl+Shift+M` is the same accelerator.
//! Aliases are accepted for readability: `control`, `option`, `cmd`, `win`,
//! `super`, `escape`, `del`, `ins`, `pgup`, `spacebar`.
//!
//! # Deliberately rejected
//!
//! * A bare modifier (`"ctrl"`) is an error, not a no-op. A user who wrote that
//!   meant to add a key; silently doing nothing would be worse than saying so.
//! * A duplicated modifier (`"ctrl+ctrl+m"`) is an error rather than being
//!   de-duplicated, because it is almost always a typo.
//! * Function keys above F24. enigo only supports F21–F24 on Windows and Linux,
//!   so accepting F25 would produce an accelerator that works on one platform
//!   and not another.

use std::fmt;

/// The largest function key accepted, because enigo only exposes F21–F24 on
/// Windows and Linux (F25+ are Unix-only).
///
/// `pub(crate)` so the enigo mapping table can size itself from this, and so a
/// test can assert the two have not drifted apart.
pub(crate) const MAX_FUNCTION_KEY: u8 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Meta,
}

/// A non-modifier key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyName {
    Char(char),
    Function(u8),
    Enter,
    Escape,
    Tab,
    Space,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    /// Media and volume keys. Sent as the keyboard's own media keys, so they
    /// reach whichever player the operating system routes media keys to.
    MediaPlayPause,
    MediaNext,
    MediaPrevious,
    MediaStop,
    VolumeUp,
    VolumeDown,
    VolumeMute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: Vec<Modifier>,
    pub key: KeyName,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HotkeyParseError {
    #[error("hotkey is empty")]
    Empty,

    #[error("'{0}' is not a recognised key")]
    UnknownKey(String),

    #[error("'{0}' is not a recognised modifier")]
    UnknownModifier(String),

    #[error("modifier '{0}' appears more than once")]
    DuplicateModifier(String),

    #[error("hotkey has an empty part (a stray '+')")]
    EmptyModifier,

    #[error("a hotkey needs a key, not just modifiers")]
    NoKey,
}

impl Hotkey {
    pub fn new(modifiers: Vec<Modifier>, key: KeyName) -> Self {
        Self { modifiers, key }
    }

    /// Parse an accelerator such as `"ctrl+shift+m"`.
    pub fn parse(input: &str) -> Result<Self, HotkeyParseError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(HotkeyParseError::Empty);
        }

        // `ctrl++` is the conventional spelling of "Ctrl plus the plus key".
        // Handling it before the split avoids a confusing empty segment.
        if let Some(prefix) = trimmed.strip_suffix("++") {
            return Ok(Self::new(
                parse_modifiers(prefix.split('+').map(str::trim))?,
                KeyName::Char('+'),
            ));
        }

        let mut parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
        // `split` on a non-empty string always yields at least one part.
        let Some(key_text) = parts.pop() else {
            return Err(HotkeyParseError::Empty);
        };

        if key_text.is_empty() {
            return Err(HotkeyParseError::NoKey);
        }

        let modifiers = parse_modifiers(parts.into_iter())?;
        let key = parse_key(key_text)?;
        Ok(Self::new(modifiers, key))
    }
}

fn parse_modifiers<'a>(
    parts: impl Iterator<Item = &'a str>,
) -> Result<Vec<Modifier>, HotkeyParseError> {
    let mut modifiers = Vec::new();
    for part in parts {
        let normalised = part.to_ascii_lowercase();
        if normalised.is_empty() {
            return Err(HotkeyParseError::EmptyModifier);
        }

        let modifier = match normalised.as_str() {
            "ctrl" | "control" => Modifier::Ctrl,
            "shift" => Modifier::Shift,
            "alt" | "option" => Modifier::Alt,
            "meta" | "super" | "win" | "cmd" | "command" => Modifier::Meta,
            _ => return Err(HotkeyParseError::UnknownModifier(part.to_string())),
        };

        if modifiers.contains(&modifier) {
            return Err(HotkeyParseError::DuplicateModifier(part.to_string()));
        }
        modifiers.push(modifier);
    }
    Ok(modifiers)
}

fn parse_key(text: &str) -> Result<KeyName, HotkeyParseError> {
    let lower = text.to_ascii_lowercase();

    let named = match lower.as_str() {
        "enter" | "return" => Some(KeyName::Enter),
        "esc" | "escape" => Some(KeyName::Escape),
        "tab" => Some(KeyName::Tab),
        "space" | "spacebar" => Some(KeyName::Space),
        "backspace" | "bksp" => Some(KeyName::Backspace),
        "delete" | "del" => Some(KeyName::Delete),
        "insert" | "ins" => Some(KeyName::Insert),
        "home" => Some(KeyName::Home),
        "end" => Some(KeyName::End),
        "pageup" | "pgup" => Some(KeyName::PageUp),
        "pagedown" | "pgdn" | "pagedn" => Some(KeyName::PageDown),
        "up" | "arrowup" => Some(KeyName::Up),
        "down" | "arrowdown" => Some(KeyName::Down),
        "left" | "arrowleft" => Some(KeyName::Left),
        "right" | "arrowright" => Some(KeyName::Right),
        "plus" => Some(KeyName::Char('+')),
        "playpause" | "mediaplaypause" | "play" => Some(KeyName::MediaPlayPause),
        "nexttrack" | "medianext" | "next" => Some(KeyName::MediaNext),
        "prevtrack" | "mediaprevious" | "previous" | "prev" => Some(KeyName::MediaPrevious),
        "mediastop" | "stop" => Some(KeyName::MediaStop),
        "volumeup" | "volup" => Some(KeyName::VolumeUp),
        "volumedown" | "voldown" => Some(KeyName::VolumeDown),
        "volumemute" | "mute" => Some(KeyName::VolumeMute),
        _ => None,
    };
    if let Some(key) = named {
        return Ok(key);
    }

    // `f1`..`f24`. Note `"f"` on its own is NOT a function key — it falls
    // through to the single-character branch and becomes the letter F.
    if let Some(digits) = lower.strip_prefix('f') {
        if let Ok(number) = digits.parse::<u8>() {
            return if (1..=MAX_FUNCTION_KEY).contains(&number) {
                Ok(KeyName::Function(number))
            } else {
                Err(HotkeyParseError::UnknownKey(text.to_string()))
            };
        }
    }

    let mut characters = text.chars();
    match (characters.next(), characters.next()) {
        (Some(character), None) => Ok(KeyName::Char(character.to_ascii_lowercase())),
        _ => Err(HotkeyParseError::UnknownKey(text.to_string())),
    }
}

impl fmt::Display for Modifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Modifier::Ctrl => "ctrl",
            Modifier::Shift => "shift",
            Modifier::Alt => "alt",
            Modifier::Meta => "meta",
        })
    }
}

impl fmt::Display for KeyName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyName::Char(character) => write!(formatter, "{character}"),
            KeyName::Function(number) => write!(formatter, "f{number}"),
            KeyName::Enter => formatter.write_str("enter"),
            KeyName::Escape => formatter.write_str("esc"),
            KeyName::Tab => formatter.write_str("tab"),
            KeyName::Space => formatter.write_str("space"),
            KeyName::Backspace => formatter.write_str("backspace"),
            KeyName::Delete => formatter.write_str("delete"),
            KeyName::Insert => formatter.write_str("insert"),
            KeyName::Home => formatter.write_str("home"),
            KeyName::End => formatter.write_str("end"),
            KeyName::PageUp => formatter.write_str("pgup"),
            KeyName::PageDown => formatter.write_str("pgdn"),
            KeyName::Up => formatter.write_str("up"),
            KeyName::Down => formatter.write_str("down"),
            KeyName::Left => formatter.write_str("left"),
            KeyName::Right => formatter.write_str("right"),
            KeyName::MediaPlayPause => formatter.write_str("playpause"),
            KeyName::MediaNext => formatter.write_str("nexttrack"),
            KeyName::MediaPrevious => formatter.write_str("prevtrack"),
            KeyName::MediaStop => formatter.write_str("mediastop"),
            KeyName::VolumeUp => formatter.write_str("volumeup"),
            KeyName::VolumeDown => formatter.write_str("volumedown"),
            KeyName::VolumeMute => formatter.write_str("volumemute"),
        }
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for modifier in &self.modifiers {
            write!(formatter, "{modifier}+")?;
        }
        write!(formatter, "{}", self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Hotkey {
        Hotkey::parse(input).unwrap_or_else(|error| panic!("{input:?} should parse, got {error}"))
    }

    #[test]
    fn parses_the_example_from_the_prd() {
        let hotkey = parse("ctrl+shift+m");
        assert_eq!(hotkey.modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
        assert_eq!(hotkey.key, KeyName::Char('m'));
    }

    #[test]
    fn a_bare_key_has_no_modifiers() {
        assert_eq!(parse("m").modifiers, Vec::new());
        assert_eq!(parse("m").key, KeyName::Char('m'));
    }

    #[test]
    fn parsing_is_case_and_whitespace_insensitive() {
        let expected = parse("ctrl+shift+m");
        assert_eq!(parse("Ctrl+Shift+M"), expected);
        assert_eq!(parse("  CTRL + SHIFT + M  "), expected);
        assert_eq!(parse("control+shift+m"), expected);
    }

    #[test]
    fn recognises_modifier_aliases() {
        assert_eq!(parse("option+a").modifiers, vec![Modifier::Alt]);
        assert_eq!(parse("cmd+a").modifiers, vec![Modifier::Meta]);
        assert_eq!(parse("win+a").modifiers, vec![Modifier::Meta]);
        assert_eq!(parse("super+a").modifiers, vec![Modifier::Meta]);
    }

    #[test]
    fn recognises_media_keys_and_round_trips_their_display() {
        for (text, key) in [
            ("playpause", KeyName::MediaPlayPause),
            ("nexttrack", KeyName::MediaNext),
            ("prevtrack", KeyName::MediaPrevious),
            ("mediastop", KeyName::MediaStop),
            ("volumeup", KeyName::VolumeUp),
            ("volumedown", KeyName::VolumeDown),
            ("volumemute", KeyName::VolumeMute),
        ] {
            let parsed = Hotkey::parse(text).expect(text);
            assert_eq!(parsed.key, key);
            assert_eq!(
                Hotkey::parse(&parsed.to_string()).expect("display parses"),
                parsed
            );
        }
    }

    #[test]
    fn recognises_named_keys_and_aliases() {
        assert_eq!(parse("enter").key, KeyName::Enter);
        assert_eq!(parse("return").key, KeyName::Enter);
        assert_eq!(parse("escape").key, KeyName::Escape);
        assert_eq!(parse("esc").key, KeyName::Escape);
        assert_eq!(parse("del").key, KeyName::Delete);
        assert_eq!(parse("ins").key, KeyName::Insert);
        assert_eq!(parse("pgup").key, KeyName::PageUp);
        assert_eq!(parse("pgdn").key, KeyName::PageDown);
        assert_eq!(parse("spacebar").key, KeyName::Space);
    }

    #[test]
    fn parses_function_keys() {
        assert_eq!(parse("f1").key, KeyName::Function(1));
        assert_eq!(parse("f12").key, KeyName::Function(12));
        assert_eq!(parse("ctrl+f24").key, KeyName::Function(24));
    }

    #[test]
    fn a_lone_f_is_the_letter_not_a_function_key() {
        assert_eq!(parse("f").key, KeyName::Char('f'));
        assert_eq!(parse("ctrl+f").key, KeyName::Char('f'));
    }

    #[test]
    fn function_keys_above_twenty_four_are_rejected() {
        // Accepting these would produce accelerators that work on one platform
        // and silently fail on another.
        assert_eq!(
            Hotkey::parse("f25"),
            Err(HotkeyParseError::UnknownKey("f25".to_string()))
        );
    }

    #[test]
    fn the_plus_key_can_be_written_as_a_double_plus_or_by_name() {
        for input in ["ctrl++", "ctrl+plus"] {
            let hotkey = parse(input);
            assert_eq!(hotkey.modifiers, vec![Modifier::Ctrl], "{input}");
            assert_eq!(hotkey.key, KeyName::Char('+'), "{input}");
        }
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(Hotkey::parse(""), Err(HotkeyParseError::Empty));
        assert_eq!(Hotkey::parse("   "), Err(HotkeyParseError::Empty));
    }

    #[test]
    fn a_trailing_plus_without_a_key_is_rejected() {
        assert_eq!(Hotkey::parse("ctrl+"), Err(HotkeyParseError::NoKey));
    }

    #[test]
    fn a_bare_modifier_is_rejected_rather_than_silently_doing_nothing() {
        // Reporting "shift is not a key" is far more useful than binding a
        // button that does nothing.
        assert_eq!(
            Hotkey::parse("shift"),
            Err(HotkeyParseError::UnknownKey("shift".to_string()))
        );
    }

    #[test]
    fn duplicate_modifiers_are_rejected() {
        assert_eq!(
            Hotkey::parse("ctrl+ctrl+m"),
            Err(HotkeyParseError::DuplicateModifier("ctrl".to_string()))
        );
    }

    #[test]
    fn an_unknown_modifier_is_named_in_the_error() {
        assert_eq!(
            Hotkey::parse("hyper+m"),
            Err(HotkeyParseError::UnknownModifier("hyper".to_string()))
        );
    }

    #[test]
    fn a_stray_plus_in_the_middle_is_rejected() {
        assert_eq!(
            Hotkey::parse("ctrl++m"),
            Err(HotkeyParseError::EmptyModifier)
        );
    }

    #[test]
    fn unknown_multi_character_keys_are_rejected() {
        assert_eq!(
            Hotkey::parse("ctrl+banana"),
            Err(HotkeyParseError::UnknownKey("banana".to_string()))
        );
    }

    #[test]
    fn display_round_trips_through_parse() {
        for input in [
            "ctrl+shift+m",
            "ctrl++",
            "f5",
            "ctrl+alt+delete",
            "meta+space",
            "pgdn",
            "m",
        ] {
            let parsed = parse(input);
            let rendered = parsed.to_string();
            let reparsed = Hotkey::parse(&rendered)
                .unwrap_or_else(|error| panic!("{rendered:?} should re-parse, got {error}"));
            assert_eq!(reparsed, parsed, "{input} rendered as {rendered}");
        }
    }
}
