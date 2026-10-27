use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::UI::Shell::*;

pub const WM_SNAPDOCK_TRAY: u32 = WM_APP + 1;
pub const IDM_SETTINGS: usize = 1000;
pub const IDM_QUIT: usize = 1001;

pub struct TrayIcon {
    nid: NOTIFYICONDATAW,
}

impl TrayIcon {
    pub fn new(hwnd: HWND) -> Self {
        let mut nid = NOTIFYICONDATAW::default();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_TIP | NIF_ICON;
        nid.uCallbackMessage = WM_SNAPDOCK_TRAY;

        // Try to load embedded icon from resource #1
        let h_instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default() };
        let icon = unsafe { LoadIconW(Some(h_instance.into()), PCWSTR(1 as *const u16)) }
            .unwrap_or_else(|_| unsafe { LoadIconW(None, IDI_APPLICATION).unwrap() });
        nid.hIcon = icon;

        let tip = "Snapdock (Ctrl+Shift+S)\0".encode_utf16().collect::<Vec<u16>>();
        let len = tip.len().min(nid.szTip.len());
        nid.szTip[..len].copy_from_slice(&tip[..len]);

        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        }

        Self { nid }
    }

    pub fn show_balloon(&self, title: &str, message: &str) {
        let mut nid = self.nid;
        nid.uFlags |= NIF_INFO;
        nid.dwInfoFlags = NIIF_WARNING;
        let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let msg_w: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();

        let t_len = title_w.len().min(nid.szInfoTitle.len());
        nid.szInfoTitle[..t_len].copy_from_slice(&title_w[..t_len]);

        let m_len = msg_w.len().min(nid.szInfo.len());
        nid.szInfo[..m_len].copy_from_slice(&msg_w[..m_len]);

        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    pub fn update_tip(&mut self, tip_str: &str) {
        let tip = tip_str.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let len = tip.len().min(self.nid.szTip.len());
        self.nid.szTip = [0; 128];
        self.nid.szTip[..len].copy_from_slice(&tip[..len]);
        self.nid.uFlags |= NIF_TIP;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &self.nid);
        }
    }

    pub fn show_menu(&self, hwnd: HWND) {
        unsafe {
            if let Ok(hmenu) = CreatePopupMenu() {
                let _ = AppendMenuW(hmenu, MF_STRING, IDM_SETTINGS, w!("Settings..."));
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                let _ = AppendMenuW(hmenu, MF_STRING, IDM_QUIT, w!("Quit Snapdock"));
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let _ = SetForegroundWindow(hwnd);
                let _ = TrackPopupMenu(
                    hmenu,
                    TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                    pt.x,
                    pt.y,
                    None,
                    hwnd,
                    None,
                );
                let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
                let _ = DestroyMenu(hmenu);
            }
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
        }
    }
}
