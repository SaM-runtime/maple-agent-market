//! Floating-window keyboard input — the winit KEY→action map for the hidden
//! close gesture and audio runtime controls (#633 close-out). The audio state
//! TRANSITION is shared with the
//! TUI in `crate::audio` ([`crate::audio::apply_audio_action`]); only this
//! key-decoding half is painter-specific (winit here, crossterm in the TUI),
//! so the two surfaces can't drift on feel. The winit glue in `window.rs`
//! stays thin (codecov-ignored, like the TUI event loop).

use crate::audio::AudioAction;
use winit::keyboard::{Key, NamedKey};

pub(crate) use super::offscreen::MapleViewSelection as MapViewSelection;

/// `Escape` closes the focused frameless window without adding visible chrome.
/// Swallow repeats so holding the key cannot dispatch duplicate close requests.
pub(crate) fn close_requested(key: &Key, repeat: bool) -> bool {
    !repeat && matches!(key, Key::Named(NamedKey::Escape))
}

/// `Tab` flips between the two Maple maps.  One press equals one camera move;
/// swallowing repeat prevents a held key from strobing between scenes.
pub(crate) fn map_switch_requested(key: &Key, repeat: bool) -> bool {
    !repeat && matches!(key, Key::Named(NamedKey::Tab))
}

/// `z` cycles the compact/medium/large floating-window presets.  It is kept
/// lowercase and non-repeating like the discrete `m` and `Tab` actions.
pub(crate) fn size_switch_requested(key: &Key, repeat: bool) -> bool {
    !repeat && matches!(key, Key::Character(value) if value.as_str() == "z")
}

/// Direct map selection: `1` = both maps, `2` = Free Market, `3` = training.
pub(crate) fn map_view_selection(key: &Key, repeat: bool) -> Option<MapViewSelection> {
    if repeat {
        return None;
    }
    let Key::Character(value) = key else {
        return None;
    };
    match value.as_str() {
        "1" => Some(MapViewSelection::Dual),
        "2" => Some(MapViewSelection::FreeMarket),
        "3" => Some(MapViewSelection::ForestTraining),
        _ => None,
    }
}

/// Map a winit logical key to an [`AudioAction`] — the TUI's `m` / `+`(`=`) /
/// `-`(`_`) vocabulary (lowercase `m` only, matching the TUI's
/// `KeyCode::Char('m')`; no Shift+M).
///
/// winit delivers an explicit `repeat` flag, which the TUI's crossterm path
/// LACKS (crossterm surfaces OS autorepeat as ordinary `Press` events unless
/// the never-enabled kitty protocol is on, so a held `m` there re-dispatches).
/// We use it as floating-only hardening: volume keys accept repeats (holding
/// `-` slides the volume), the mute TOGGLE swallows them (a held `m` must not
/// oscillate).
pub(crate) fn audio_action(key: &Key, repeat: bool) -> Option<AudioAction> {
    let Key::Character(s) = key else {
        return None;
    };
    match s.as_str() {
        "m" if !repeat => Some(AudioAction::ToggleMute),
        "+" | "=" => Some(AudioAction::Volume(true)),
        "-" | "_" => Some(AudioAction::Volume(false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> Key {
        Key::Character(s.into())
    }

    #[test]
    fn key_map_is_the_tui_vocabulary_and_swallows_mute_repeats() {
        assert_eq!(
            audio_action(&key("m"), false),
            Some(AudioAction::ToggleMute)
        );
        assert_eq!(
            audio_action(&key("m"), true),
            None,
            "held m must not oscillate"
        );
        assert_eq!(
            audio_action(&key("M"), false),
            None,
            "Shift+M is not mute — parity with the TUI's Char('m')"
        );
        for k in ["+", "="] {
            assert_eq!(
                audio_action(&key(k), false),
                Some(AudioAction::Volume(true))
            );
            assert_eq!(
                audio_action(&key(k), true),
                Some(AudioAction::Volume(true)),
                "up-volume keys autorepeat"
            );
        }
        for k in ["-", "_"] {
            assert_eq!(
                audio_action(&key(k), false),
                Some(AudioAction::Volume(false))
            );
            assert_eq!(
                audio_action(&key(k), true),
                Some(AudioAction::Volume(false)),
                "down-volume keys autorepeat too (symmetry with up)"
            );
        }
        assert_eq!(audio_action(&key("q"), false), None);
        assert_eq!(
            audio_action(&Key::Named(winit::keyboard::NamedKey::Enter), false),
            None
        );
    }

    #[test]
    fn escape_requests_a_single_close_without_claiming_other_keys() {
        let escape = Key::Named(winit::keyboard::NamedKey::Escape);
        assert!(close_requested(&escape, false));
        assert!(!close_requested(&escape, true), "held Escape closes once");
        assert!(!close_requested(&key("q"), false));
        assert!(!close_requested(
            &Key::Named(winit::keyboard::NamedKey::Enter),
            false
        ));
    }

    #[test]
    fn tab_requests_one_map_switch_without_claiming_enter() {
        let tab = Key::Named(NamedKey::Tab);
        assert!(map_switch_requested(&tab, false));
        assert!(!map_switch_requested(&tab, true));
        assert!(!map_switch_requested(&Key::Named(NamedKey::Enter), false));
    }

    #[test]
    fn z_requests_one_window_size_step_without_claiming_audio_or_repeat() {
        assert!(size_switch_requested(&key("z"), false));
        assert!(!size_switch_requested(&key("z"), true));
        assert!(!size_switch_requested(&key("Z"), false));
        assert!(!size_switch_requested(&key("m"), false));
    }

    #[test]
    fn number_keys_select_dual_market_and_training_without_autorepeat() {
        assert_eq!(
            map_view_selection(&key("1"), false),
            Some(super::MapViewSelection::Dual)
        );
        assert_eq!(
            map_view_selection(&key("2"), false),
            Some(super::MapViewSelection::FreeMarket)
        );
        assert_eq!(
            map_view_selection(&key("3"), false),
            Some(super::MapViewSelection::ForestTraining)
        );
        assert_eq!(map_view_selection(&key("2"), true), None);
        assert_eq!(map_view_selection(&key("4"), false), None);
    }
}
