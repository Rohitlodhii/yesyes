use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

pub const SNAPDOCK_HOTKEY_ID: i32 = 101;

pub struct HotkeyManager {
    hwnd: HWND,
    pub modifiers: u32,
    pub vk: u32,
    pub registered: bool,
}

pub fn format_hotkey(modifiers: u32, vk: u32) -> String {
    let mut parts = Vec::new();
    if (modifiers & MOD_CONTROL.0) != 0 {
        parts.push("Ctrl");
    }
    if (modifiers & MOD_ALT.0) != 0 {
        parts.push("Alt");
    }
    if (modifiers & MOD_SHIFT.0) != 0 {
        parts.push("Shift");
    }
    if (modifiers & MOD_WIN.0) != 0 {
        parts.push("Win");
    }

    let vk_str = match vk {
        0x20 => "Space".to_string(),
        0x2C => "PrintScreen".to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x30..=0x39 => ((vk as u8) as char).to_string(),
        0x41..=0x5A => ((vk as u8) as char).to_string(),
        0x09 => "Tab".to_string(),
        0x0D => "Enter".to_string(),
        0x1B => "Esc".to_string(),
        _ => format!("Key(0x{vk:02X})"),
    };

    parts.push(&vk_str);
    parts.join(" + ")
}

impl HotkeyManager {
    pub fn new(hwnd: HWND, modifiers: u32, vk: u32) -> (Self, std::result::Result<(), String>) {
        let mut mgr = Self {
            hwnd,
            modifiers,
            vk,
            registered: false,
        };

        let res = mgr.try_register(modifiers, vk);
        (mgr, res)
    }

    fn try_register(&mut self, modifiers: u32, vk: u32) -> std::result::Result<(), String> {
        let mods = HOT_KEY_MODIFIERS(modifiers | MOD_NOREPEAT.0);
        let res = unsafe {
            RegisterHotKey(
                Some(self.hwnd),
                SNAPDOCK_HOTKEY_ID,
                mods,
                vk,
            )
        };

        match res {
            Ok(_) => {
                self.modifiers = modifiers;
                self.vk = vk;
                self.registered = true;
                Ok(())
            }
            Err(e) => {
                self.registered = false;
                let key_name = format_hotkey(modifiers, vk);
                Err(format!(
                    "Hotkey conflict: '{key_name}' could not be registered ({e}). It is likely already in use by Windows or another application."
                ))
            }
        }
    }

    pub fn rebind(&mut self, modifiers: u32, vk: u32) -> std::result::Result<(), String> {
        if self.registered {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), SNAPDOCK_HOTKEY_ID);
            }
            self.registered = false;
        }

        let old_mods = self.modifiers;
        let old_vk = self.vk;

        match self.try_register(modifiers, vk) {
            Ok(()) => Ok(()),
            Err(err) => {
                // Attempt to restore previous hotkey
                let _ = self.try_register(old_mods, old_vk);
                Err(err)
            }
        }
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        if self.registered {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), SNAPDOCK_HOTKEY_ID);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_hotkey() {
        assert_eq!(format_hotkey(0x0002 | 0x0004, 0x53), "Ctrl + Shift + S");
        assert_eq!(format_hotkey(0x0001, 0x70), "Alt + F1");
        assert_eq!(format_hotkey(0x0008, 0x2C), "Win + PrintScreen");
    }
}
