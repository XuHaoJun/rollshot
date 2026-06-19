use crate::daemon::config::{Modifier, Shortcut};
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
    let name = if key.starts_with('F') && key.len() >= 2 && key[1..].chars().all(|c| c.is_ascii_digit())
    {
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

use crate::daemon::core::DaemonEvent;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use winit::event_loop::EventLoopProxy;

pub(crate) struct ShortcutGuard {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
}

impl ShortcutGuard {
    pub(crate) fn start(
        proxy: EventLoopProxy<DaemonEvent>,
        shortcut: &Shortcut,
    ) -> Result<Self, String> {
        let hotkey = to_hotkey(shortcut)?;
        let manager = GlobalHotKeyManager::new()
            .map_err(|error| format!("failed to initialize global hotkey manager: {error}"))?;
        manager
            .register(hotkey)
            .map_err(|error| format!("failed to register capture hotkey: {error}"))?;

        let registered_id = hotkey.id();
        GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
            if event.id() == registered_id && event.state() == HotKeyState::Pressed {
                let _ = proxy.send_event(DaemonEvent::CaptureRegion);
            }
        }));

        Ok(Self { manager, hotkey })
    }
}

impl Drop for ShortcutGuard {
    fn drop(&mut self) {
        GlobalHotKeyEvent::set_event_handler(None::<fn(GlobalHotKeyEvent)>);
        let _ = self.manager.unregister(self.hotkey);
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
}
