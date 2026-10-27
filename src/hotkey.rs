use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

pub const SNAPDOCK_HOTKEY_ID: i32 = 101;
pub const SNAPDOCK_HOTKEY_MODS: HOT_KEY_MODIFIERS = HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_NOREPEAT.0);
pub const SNAPDOCK_HOTKEY_VK: u32 = 0x53; // 'S'

pub struct HotkeyManager {
    hwnd: HWND,
    registered: bool,
}

impl HotkeyManager {
    pub fn new(hwnd: HWND) -> (Self, std::result::Result<(), String>) {
        let res = unsafe {
            RegisterHotKey(
                Some(hwnd),
                SNAPDOCK_HOTKEY_ID,
                SNAPDOCK_HOTKEY_MODS,
                SNAPDOCK_HOTKEY_VK,
            )
        };

        match res {
            Ok(_) => (Self { hwnd, registered: true }, Ok(())),
            Err(e) => (
                Self { hwnd, registered: false },
                Err(format!(
                    "Hotkey conflict: Ctrl+Shift+S could not be registered ({e}). It is likely in use by another application."
                )),
            ),
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
