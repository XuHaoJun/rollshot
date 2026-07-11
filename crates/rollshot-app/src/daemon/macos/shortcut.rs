use crate::daemon::config::{Modifier, Shortcut};
use crate::daemon::core::CaptureKind;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};

pub(crate) fn to_hotkey(shortcut: &Shortcut) -> Result<HotKey, String> {
    let mut modifiers = Modifiers::empty();
    for modifier in shortcut.modifiers() {
        modifiers |= match modifier {
            Modifier::Control => Modifiers::CONTROL,
            Modifier::Alt => Modifiers::ALT,
            Modifier::Shift => Modifiers::SHIFT,
            Modifier::Command | Modifier::Super => Modifiers::SUPER,
        };
    }
    let code = key_to_code(shortcut.key())?;
    Ok(HotKey::new(Some(modifiers), code))
}

fn key_to_code(key: &str) -> Result<Code, String> {
    let name =
        if key.starts_with('F') && key.len() >= 2 && key[1..].chars().all(|c| c.is_ascii_digit()) {
            key.to_string()
        } else if key.len() == 1 {
            let ch = key.chars().next().expect("len == 1");
            if ch.is_ascii_digit() {
                format!("Digit{ch}")
            } else {
                format!("Key{}", ch.to_ascii_uppercase())
            }
        } else {
            return Err(format!("unsupported shortcut key: {key}"));
        };
    name.parse::<Code>()
        .map_err(|_| format!("unsupported shortcut key: {key}"))
}

pub(crate) fn hotkey_event_for_id(
    id: u32,
    registered: &[(u32, CaptureKind)],
) -> Option<crate::daemon::core::DaemonEvent> {
    registered
        .iter()
        .find(|(rid, _)| *rid == id)
        .map(|(_, kind)| match kind {
            CaptureKind::Region => crate::daemon::core::DaemonEvent::CaptureRegion,
            CaptureKind::Text => crate::daemon::core::DaemonEvent::CaptureText,
        })
}

#[cfg(feature = "ocr")]
#[allow(dead_code)]
pub(crate) fn is_valid_registration(registered: &[(u32, CaptureKind)]) -> bool {
    registered
        .iter()
        .any(|(_, kind)| *kind == CaptureKind::Region)
}

use crate::daemon::core::DaemonEvent;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use winit::event_loop::EventLoopProxy;

pub(crate) struct ShortcutGuard {
    manager: GlobalHotKeyManager,
    hotkeys: Vec<HotKey>,
}

impl ShortcutGuard {
    pub(crate) fn start(
        proxy: EventLoopProxy<DaemonEvent>,
        region_shortcut: &Shortcut,
        #[cfg(feature = "ocr")] text_shortcut: Option<&Shortcut>,
    ) -> Result<Self, String> {
        let manager = GlobalHotKeyManager::new()
            .map_err(|error| format!("failed to initialize global hotkey manager: {error}"))?;

        let region_hotkey = to_hotkey(region_shortcut)?;
        manager
            .register(region_hotkey)
            .map_err(|error| format!("failed to register capture hotkey: {error}"))?;

        #[allow(unused_mut)]
        let mut registered: Vec<(u32, CaptureKind)> =
            vec![(region_hotkey.id(), CaptureKind::Region)];

        #[cfg(feature = "ocr")]
        if let Some(text_shortcut) = text_shortcut {
            match to_hotkey(text_shortcut) {
                Ok(text_hotkey) => {
                    if manager.register(text_hotkey).is_ok() {
                        registered.push((text_hotkey.id(), CaptureKind::Text));
                    } else {
                        tracing::warn!(
                            target: "rollshot::daemon::shortcut",
                            "failed to register text hotkey; retaining region hotkey only"
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        target: "rollshot::daemon::shortcut",
                        %error,
                        "failed to convert text shortcut; retaining region hotkey only"
                    );
                }
            }
        }

        let hotkeys: Vec<HotKey> = registered
            .iter()
            .filter_map(|(id, _)| {
                if *id == region_hotkey.id() {
                    Some(region_hotkey)
                } else {
                    #[cfg(feature = "ocr")]
                    {
                        let text_shortcut = text_shortcut?;
                        let text_hotkey = to_hotkey(text_shortcut).ok()?;
                        if text_hotkey.id() == *id {
                            Some(text_hotkey)
                        } else {
                            None
                        }
                    }
                    #[cfg(not(feature = "ocr"))]
                    None
                }
            })
            .collect();

        let registered_clone = registered.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.state() == HotKeyState::Pressed {
                if let Some(daemon_event) = hotkey_event_for_id(event.id(), &registered_clone) {
                    let _ = proxy.send_event(daemon_event);
                }
            }
        }));

        Ok(Self { manager, hotkeys })
    }
}

impl Drop for ShortcutGuard {
    fn drop(&mut self) {
        GlobalHotKeyEvent::set_event_handler(None::<fn(GlobalHotKeyEvent)>);
        for hotkey in &self.hotkeys {
            let _ = self.manager.unregister(*hotkey);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_macos_default_to_command_shift_digit6() {
        let shortcut: Shortcut = "Command+Shift+6".parse().unwrap();
        let hotkey = to_hotkey(&shortcut).unwrap();
        assert_eq!(hotkey.mods, Modifiers::SUPER | Modifiers::SHIFT);
        assert_eq!(hotkey.key, Code::Digit6);
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn translates_command_shift_seven_to_digit7() {
        let shortcut: Shortcut = "Command+Shift+7".parse().unwrap();
        let hotkey = to_hotkey(&shortcut).unwrap();
        assert_eq!(hotkey.mods, Modifiers::SUPER | Modifiers::SHIFT);
        assert_eq!(hotkey.key, Code::Digit7);
    }

    #[test]
    fn translates_letter_and_function_keys() {
        assert_eq!(
            to_hotkey(&"Command+A".parse().unwrap()).unwrap().key,
            Code::KeyA
        );
        assert_eq!(
            to_hotkey(&"Command+F6".parse().unwrap()).unwrap().key,
            Code::F6
        );
    }

    #[test]
    fn super_maps_to_command_meta() {
        let hotkey = to_hotkey(&"Super+Shift+6".parse().unwrap()).unwrap();
        assert_eq!(hotkey.mods, Modifiers::SUPER | Modifiers::SHIFT);
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn hotkey_event_for_id_routes_region_and_text() {
        let region_hotkey = to_hotkey(&"Command+Shift+6".parse().unwrap()).unwrap();
        let text_hotkey = to_hotkey(&"Command+Shift+7".parse().unwrap()).unwrap();
        let registered: Vec<(u32, CaptureKind)> = vec![
            (region_hotkey.id(), CaptureKind::Region),
            (text_hotkey.id(), CaptureKind::Text),
        ];
        assert_eq!(
            hotkey_event_for_id(region_hotkey.id(), &registered),
            Some(DaemonEvent::CaptureRegion)
        );
        assert_eq!(
            hotkey_event_for_id(text_hotkey.id(), &registered),
            Some(DaemonEvent::CaptureText)
        );
        assert_eq!(hotkey_event_for_id(999, &registered), None);
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn region_only_registration_is_valid() {
        let region_hotkey = to_hotkey(&"Command+Shift+6".parse().unwrap()).unwrap();
        let registered: Vec<(u32, CaptureKind)> = vec![(region_hotkey.id(), CaptureKind::Region)];
        assert!(is_valid_registration(&registered));
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn empty_registration_is_invalid() {
        let registered: Vec<(u32, CaptureKind)> = vec![];
        assert!(!is_valid_registration(&registered));
    }

    #[cfg(feature = "ocr")]
    #[test]
    fn text_only_registration_is_invalid() {
        let text_hotkey = to_hotkey(&"Command+Shift+7".parse().unwrap()).unwrap();
        let registered: Vec<(u32, CaptureKind)> = vec![(text_hotkey.id(), CaptureKind::Text)];
        assert!(!is_valid_registration(&registered));
    }
}
