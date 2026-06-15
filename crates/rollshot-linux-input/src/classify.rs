//! Pure, host-agnostic classification of raw evdev events into privacy-filtered
//! semantic actions. No device identity, no raw key code, and no typed text
//! ever leaves this module — ordinary keys collapse to `TypingActivity`; only
//! Enter/Tab survive as semantic keys. Tested on every CI host.

use rollshot_action::{MouseButton, SemanticAction, SemanticKey};

// Linux input-event-codes.h ABI constants (stable kernel UAPI).
pub(crate) const EV_SYN: u16 = 0x00;
pub(crate) const EV_KEY: u16 = 0x01;
pub(crate) const EV_REL: u16 = 0x02;

pub(crate) const REL_X: u16 = 0x00;
pub(crate) const REL_Y: u16 = 0x01;
pub(crate) const REL_HWHEEL: u16 = 0x06;
pub(crate) const REL_WHEEL: u16 = 0x08;

pub(crate) const BTN_LEFT: u16 = 0x110;
pub(crate) const BTN_RIGHT: u16 = 0x111;
pub(crate) const BTN_MIDDLE: u16 = 0x112;

pub(crate) const KEY_TAB: u16 = 15;
pub(crate) const KEY_ENTER: u16 = 28;
// Codes used only by tests to represent "some ordinary key".
#[cfg(test)]
pub(crate) const KEY_A: u16 = 30;
#[cfg(test)]
pub(crate) const KEY_1: u16 = 2;
#[cfg(test)]
pub(crate) const KEY_SPACE: u16 = 57;

/// A native evdev event reduced to the three fields classification needs.
/// Deliberately minimal: no timestamp, no device handle, no name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawEvdevEvent {
    pub ev_type: u16,
    pub code: u16,
    pub value: i32,
}

/// Stateless classifier (kept as a struct for symmetry with the macOS side and
/// to leave room for future stateful rules without an API break).
#[derive(Debug, Default)]
pub struct EvdevClassifier {
    _private: (),
}

impl EvdevClassifier {
    pub fn new() -> Self {
        Self::default()
    }

    /// Map one raw evdev event to a semantic action, or `None` to ignore it.
    /// Only key/button *presses* (`value == 1`) and wheel motion produce
    /// actions; releases (`0`), autorepeat (`2`), pointer motion, and sync are
    /// ignored.
    pub fn classify(&mut self, ev: RawEvdevEvent) -> Option<SemanticAction> {
        match ev.ev_type {
            EV_KEY if ev.value == 1 => match ev.code {
                BTN_LEFT => Some(SemanticAction::Click {
                    button: MouseButton::Left,
                    position: None,
                }),
                BTN_RIGHT => Some(SemanticAction::Click {
                    button: MouseButton::Right,
                    position: None,
                }),
                BTN_MIDDLE => Some(SemanticAction::Click {
                    button: MouseButton::Middle,
                    position: None,
                }),
                // Any other BTN_* in the mouse range -> Other button click.
                c if (0x110..0x118).contains(&c) => Some(SemanticAction::Click {
                    button: MouseButton::Other,
                    position: None,
                }),
                KEY_ENTER => Some(SemanticAction::SemanticKey(SemanticKey::Enter)),
                KEY_TAB => Some(SemanticAction::SemanticKey(SemanticKey::Tab)),
                // Every other key press is ordinary typing — the code is dropped.
                _ => Some(SemanticAction::TypingActivity),
            },
            EV_REL if ev.code == REL_WHEEL || ev.code == REL_HWHEEL => {
                Some(SemanticAction::ScrollActivity)
            }
            // REL_X/REL_Y pointer motion, EV_SYN, releases, autorepeat: ignored.
            EV_REL if ev.code == REL_X || ev.code == REL_Y => None,
            EV_SYN => None,
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rollshot_action::{MouseButton, SemanticAction, SemanticKey};

    fn ev(t: u16, c: u16, v: i32) -> RawEvdevEvent {
        RawEvdevEvent {
            ev_type: t,
            code: c,
            value: v,
        }
    }

    #[test]
    fn left_button_press_is_a_left_click_with_no_position() {
        let mut c = EvdevClassifier::new();
        assert_eq!(
            c.classify(ev(EV_KEY, BTN_LEFT, 1)),
            Some(SemanticAction::Click {
                button: MouseButton::Left,
                position: None
            })
        );
    }

    #[test]
    fn right_and_middle_buttons_map_to_their_buttons() {
        let mut c = EvdevClassifier::new();
        assert_eq!(
            c.classify(ev(EV_KEY, BTN_RIGHT, 1)),
            Some(SemanticAction::Click {
                button: MouseButton::Right,
                position: None
            })
        );
        assert_eq!(
            c.classify(ev(EV_KEY, BTN_MIDDLE, 1)),
            Some(SemanticAction::Click {
                button: MouseButton::Middle,
                position: None
            })
        );
    }

    #[test]
    fn other_buttons_in_mouse_range_map_to_other() {
        let mut c = EvdevClassifier::new();
        assert_eq!(
            c.classify(ev(EV_KEY, 0x113, 1)),
            Some(SemanticAction::Click {
                button: MouseButton::Other,
                position: None
            })
        );
    }

    #[test]
    fn button_release_and_autorepeat_are_ignored() {
        let mut c = EvdevClassifier::new();
        assert_eq!(c.classify(ev(EV_KEY, BTN_LEFT, 0)), None); // release
        assert_eq!(c.classify(ev(EV_KEY, KEY_A, 2)), None); // autorepeat
    }

    #[test]
    fn wheel_and_hwheel_map_to_scroll_activity() {
        let mut c = EvdevClassifier::new();
        assert_eq!(
            c.classify(ev(EV_REL, REL_WHEEL, 1)),
            Some(SemanticAction::ScrollActivity)
        );
        assert_eq!(
            c.classify(ev(EV_REL, REL_HWHEEL, -1)),
            Some(SemanticAction::ScrollActivity)
        );
    }

    #[test]
    fn pointer_motion_and_sync_never_create_actions() {
        let mut c = EvdevClassifier::new();
        assert_eq!(c.classify(ev(EV_REL, REL_X, 5)), None);
        assert_eq!(c.classify(ev(EV_REL, REL_Y, -3)), None);
        assert_eq!(c.classify(ev(EV_SYN, 0, 0)), None);
    }

    #[test]
    fn enter_and_tab_press_are_semantic_keys() {
        let mut c = EvdevClassifier::new();
        assert_eq!(
            c.classify(ev(EV_KEY, KEY_ENTER, 1)),
            Some(SemanticAction::SemanticKey(SemanticKey::Enter))
        );
        assert_eq!(
            c.classify(ev(EV_KEY, KEY_TAB, 1)),
            Some(SemanticAction::SemanticKey(SemanticKey::Tab))
        );
    }

    #[test]
    fn ordinary_key_press_collapses_to_typing_activity_never_a_code() {
        let mut c = EvdevClassifier::new();
        // A letter, a digit, and space all collapse to TypingActivity — the
        // raw code is never surfaced (privacy by construction).
        for code in [KEY_A, KEY_1, KEY_SPACE] {
            assert_eq!(
                c.classify(ev(EV_KEY, code, 1)),
                Some(SemanticAction::TypingActivity)
            );
        }
    }

    #[test]
    fn key_release_is_ignored_so_only_presses_drive_typing() {
        let mut c = EvdevClassifier::new();
        assert_eq!(c.classify(ev(EV_KEY, KEY_A, 0)), None);
    }
}
