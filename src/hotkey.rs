use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

pub const SNAPDOCK_HOTKEY_REGION_ID: i32 = 101;
pub const SNAPDOCK_HOTKEY_FULLSCREEN_ID: i32 = 102;
pub const SNAPDOCK_HOTKEY_HISTORY_ID: i32 = 103;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBinding {
    pub id: i32,
    pub name: &'static str,
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

pub struct HotkeyManager {
    hwnd: HWND,
    pub region: HotkeyBinding,
    pub fullscreen: HotkeyBinding,
    pub history: HotkeyBinding,
}

impl HotkeyManager {
    pub fn new(
        hwnd: HWND,
        reg_mods: u32,
        reg_vk: u32,
        fs_mods: u32,
        fs_vk: u32,
        hist_mods: u32,
        hist_vk: u32,
    ) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();

        let mut mgr = Self {
            hwnd,
            region: HotkeyBinding {
                id: SNAPDOCK_HOTKEY_REGION_ID,
                name: "Region Screenshot",
                modifiers: reg_mods,
                vk: reg_vk,
                registered: false,
            },
            fullscreen: HotkeyBinding {
                id: SNAPDOCK_HOTKEY_FULLSCREEN_ID,
                name: "Full Screen Screenshot",
                modifiers: fs_mods,
                vk: fs_vk,
                registered: false,
            },
            history: HotkeyBinding {
                id: SNAPDOCK_HOTKEY_HISTORY_ID,
                name: "Screenshot History",
                modifiers: hist_mods,
                vk: hist_vk,
                registered: false,
            },
        };

        if let Err(e) = mgr.register_binding(SNAPDOCK_HOTKEY_REGION_ID) {
            warnings.push(e);
        }
        if let Err(e) = mgr.register_binding(SNAPDOCK_HOTKEY_FULLSCREEN_ID) {
            warnings.push(e);
        }
        if let Err(e) = mgr.register_binding(SNAPDOCK_HOTKEY_HISTORY_ID) {
            warnings.push(e);
        }

        (mgr, warnings)
    }

    fn register_binding(&mut self, id: i32) -> Result<(), String> {
        let binding = match id {
            SNAPDOCK_HOTKEY_REGION_ID => &mut self.region,
            SNAPDOCK_HOTKEY_FULLSCREEN_ID => &mut self.fullscreen,
            SNAPDOCK_HOTKEY_HISTORY_ID => &mut self.history,
            _ => return Err("Invalid hotkey ID".to_string()),
        };

        let mods = HOT_KEY_MODIFIERS(binding.modifiers | MOD_NOREPEAT.0);
        let res = unsafe {
            RegisterHotKey(
                Some(self.hwnd),
                binding.id,
                mods,
                binding.vk,
            )
        };

        match res {
            Ok(_) => {
                binding.registered = true;
                Ok(())
            }
            Err(e) => {
                binding.registered = false;
                let key_name = format_hotkey(binding.modifiers, binding.vk);
                Err(format!(
                    "Hotkey conflict for {}: '{}' could not be registered ({e}). It may already be in use by another app.",
                    binding.name, key_name
                ))
            }
        }
    }

    fn unregister_binding(&mut self, id: i32) {
        let binding = match id {
            SNAPDOCK_HOTKEY_REGION_ID => &mut self.region,
            SNAPDOCK_HOTKEY_FULLSCREEN_ID => &mut self.fullscreen,
            SNAPDOCK_HOTKEY_HISTORY_ID => &mut self.history,
            _ => return,
        };

        if binding.registered {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), binding.id);
            }
            binding.registered = false;
        }
    }

    pub fn rebind_region(&mut self, modifiers: u32, vk: u32) -> Result<(), String> {
        self.rebind_by_id(SNAPDOCK_HOTKEY_REGION_ID, modifiers, vk)
    }

    pub fn rebind_fullscreen(&mut self, modifiers: u32, vk: u32) -> Result<(), String> {
        self.rebind_by_id(SNAPDOCK_HOTKEY_FULLSCREEN_ID, modifiers, vk)
    }

    pub fn rebind_history(&mut self, modifiers: u32, vk: u32) -> Result<(), String> {
        self.rebind_by_id(SNAPDOCK_HOTKEY_HISTORY_ID, modifiers, vk)
    }

    fn rebind_by_id(&mut self, id: i32, modifiers: u32, vk: u32) -> Result<(), String> {
        self.unregister_binding(id);

        let (old_mods, old_vk) = match id {
            SNAPDOCK_HOTKEY_REGION_ID => {
                let old = (self.region.modifiers, self.region.vk);
                self.region.modifiers = modifiers;
                self.region.vk = vk;
                old
            }
            SNAPDOCK_HOTKEY_FULLSCREEN_ID => {
                let old = (self.fullscreen.modifiers, self.fullscreen.vk);
                self.fullscreen.modifiers = modifiers;
                self.fullscreen.vk = vk;
                old
            }
            SNAPDOCK_HOTKEY_HISTORY_ID => {
                let old = (self.history.modifiers, self.history.vk);
                self.history.modifiers = modifiers;
                self.history.vk = vk;
                old
            }
            _ => return Err("Invalid hotkey ID".to_string()),
        };

        match self.register_binding(id) {
            Ok(()) => Ok(()),
            Err(err) => {
                // Restore previous
                match id {
                    SNAPDOCK_HOTKEY_REGION_ID => {
                        self.region.modifiers = old_mods;
                        self.region.vk = old_vk;
                    }
                    SNAPDOCK_HOTKEY_FULLSCREEN_ID => {
                        self.fullscreen.modifiers = old_mods;
                        self.fullscreen.vk = old_vk;
                    }
                    SNAPDOCK_HOTKEY_HISTORY_ID => {
                        self.history.modifiers = old_mods;
                        self.history.vk = old_vk;
                    }
                    _ => {}
                }
                let _ = self.register_binding(id);
                Err(err)
            }
        }
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        self.unregister_binding(SNAPDOCK_HOTKEY_REGION_ID);
        self.unregister_binding(SNAPDOCK_HOTKEY_FULLSCREEN_ID);
        self.unregister_binding(SNAPDOCK_HOTKEY_HISTORY_ID);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_hotkey() {
        assert_eq!(format_hotkey(0x0002 | 0x0004, 0x53), "Ctrl + Shift + S");
        assert_eq!(format_hotkey(0x0002 | 0x0004, 0x46), "Ctrl + Shift + F");
        assert_eq!(format_hotkey(0x0002 | 0x0004, 0x20), "Ctrl + Shift + Space");
        assert_eq!(format_hotkey(0x0001, 0x70), "Alt + F1");
    }
}
