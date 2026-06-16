//! Pure, host-agnostic classification of a reduced CoreGraphics event into a
//! privacy-filtered semantic action. No Unicode text is ever read; ordinary
//! key-downs collapse to `TypingActivity`; only Return/Tab survive as semantic
//! keys. Tested on every CI host.

use rollshot_action::{MouseButton, SemanticAction, SemanticKey};

/// macOS virtual keycodes (Carbon `kVK_*`).
const KEYCODE_RETURN: i64 = 0x24;
const KEYCODE_TAB: i64 = 0x30;

/// The subset of `CGEventType` that produces a semantic action. The tap
/// callback (source.rs) reduces every `CGEvent` to one of these; everything
/// else (KeyUp, FlagsChanged, mouse-move, ScrollWheel deltas aside, tap-
/// disabled) becomes `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawCgKind {
    LeftMouseDown,
    RightMouseDown,
    OtherMouseDown,
    ScrollWheel,
    KeyDown,
    Other,
}

/// A native CGEvent reduced to the fields classification needs. `button_number`
/// is meaningful only for `OtherMouseDown`; `keycode` only for `KeyDown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawCgEvent {
    pub kind: RawCgKind,
    pub button_number: i64,
    pub keycode: i64,
}

/// Map a reduced CoreGraphics event to a semantic action, or `None` to ignore.
pub fn classify_cg(ev: RawCgEvent) -> Option<SemanticAction> {
    match ev.kind {
        RawCgKind::LeftMouseDown => Some(SemanticAction::Click {
            button: MouseButton::Left,
            position: None,
        }),
        RawCgKind::RightMouseDown => Some(SemanticAction::Click {
            button: MouseButton::Right,
            position: None,
        }),
        RawCgKind::OtherMouseDown => {
            let button = if ev.button_number == 2 {
                MouseButton::Middle
            } else {
                MouseButton::Other
            };
            Some(SemanticAction::Click {
                button,
                position: None,
            })
        }
        RawCgKind::ScrollWheel => Some(SemanticAction::ScrollActivity),
        RawCgKind::KeyDown => match ev.keycode {
            KEYCODE_RETURN => Some(SemanticAction::SemanticKey(SemanticKey::Enter)),
            KEYCODE_TAB => Some(SemanticAction::SemanticKey(SemanticKey::Tab)),
            _ => Some(SemanticAction::TypingActivity),
        },
        RawCgKind::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{MouseButton, SemanticAction, SemanticKey};

    const KEYCODE_RETURN: i64 = 0x24;
    const KEYCODE_TAB: i64 = 0x30;
    const KEYCODE_A: i64 = 0x00;

    fn ev(kind: RawCgKind, button_number: i64, keycode: i64) -> RawCgEvent {
        RawCgEvent {
            kind,
            button_number,
            keycode,
        }
    }

    #[test]
    fn mouse_downs_map_to_their_buttons_without_position() {
        assert_eq!(
            classify_cg(ev(RawCgKind::LeftMouseDown, 0, 0)),
            Some(SemanticAction::Click {
                button: MouseButton::Left,
                position: None
            })
        );
        assert_eq!(
            classify_cg(ev(RawCgKind::RightMouseDown, 1, 0)),
            Some(SemanticAction::Click {
                button: MouseButton::Right,
                position: None
            })
        );
    }

    #[test]
    fn other_mouse_button_two_is_middle_others_are_other() {
        assert_eq!(
            classify_cg(ev(RawCgKind::OtherMouseDown, 2, 0)),
            Some(SemanticAction::Click {
                button: MouseButton::Middle,
                position: None
            })
        );
        assert_eq!(
            classify_cg(ev(RawCgKind::OtherMouseDown, 3, 0)),
            Some(SemanticAction::Click {
                button: MouseButton::Other,
                position: None
            })
        );
    }

    #[test]
    fn scroll_wheel_is_scroll_activity() {
        assert_eq!(
            classify_cg(ev(RawCgKind::ScrollWheel, 0, 0)),
            Some(SemanticAction::ScrollActivity)
        );
    }

    #[test]
    fn return_and_tab_keydowns_are_semantic_keys() {
        assert_eq!(
            classify_cg(ev(RawCgKind::KeyDown, 0, KEYCODE_RETURN)),
            Some(SemanticAction::SemanticKey(SemanticKey::Enter))
        );
        assert_eq!(
            classify_cg(ev(RawCgKind::KeyDown, 0, KEYCODE_TAB)),
            Some(SemanticAction::SemanticKey(SemanticKey::Tab))
        );
    }

    #[test]
    fn ordinary_keydown_collapses_to_typing_activity_never_a_keycode() {
        assert_eq!(
            classify_cg(ev(RawCgKind::KeyDown, 0, KEYCODE_A)),
            Some(SemanticAction::TypingActivity)
        );
    }

    #[test]
    fn other_kinds_are_ignored() {
        // KeyUp, FlagsChanged, mouse-move, tap-disabled all reduce to Other.
        assert_eq!(classify_cg(ev(RawCgKind::Other, 0, KEYCODE_A)), None);
    }
}
