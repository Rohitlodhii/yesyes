use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Registry::*;

pub const WM_SETTINGCHANGE: u32 = 0x001A;
pub const WM_THEMECHANGED: u32 = 0x031A;
pub const WM_DWMCOLORIZATIONCOLORCHANGED: u32 = 0x0320;

#[inline]
pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

#[derive(Clone, Copy, Debug)]
pub struct ThemeColors {
    pub is_dark: bool,
    pub window_bg: COLORREF,
    pub card_bg: COLORREF,
    pub card_border: COLORREF,
    pub card_hover: COLORREF,
    pub text_title: COLORREF,
    pub text_body: COLORREF,
    pub text_muted: COLORREF,
    pub btn_bg: COLORREF,
    pub btn_hover: COLORREF,
    pub btn_border: COLORREF,
    pub input_bg: COLORREF,
    pub input_border: COLORREF,
    pub accent: COLORREF,
    pub accent_hover: COLORREF,
    pub close_red: COLORREF,
    pub badge_bg: COLORREF,
    pub badge_text: COLORREF,
}

impl ThemeColors {
    pub fn current() -> Self {
        if is_windows_light_theme() {
            Self::light()
        } else {
            Self::dark()
        }
    }

    pub fn dark() -> Self {
        Self {
            is_dark: true,
            window_bg: rgb(24, 24, 27),     // #18181B
            card_bg: rgb(36, 36, 39),       // #242427
            card_border: rgb(56, 56, 63),   // #38383F
            card_hover: rgb(59, 130, 246),  // #3B82F6
            text_title: rgb(244, 244, 245), // #F4F4F5
            text_body: rgb(228, 228, 231),  // #E4E4E7
            text_muted: rgb(161, 161, 170), // #A1A1AA
            btn_bg: rgb(51, 46, 46),        // #332E2E
            btn_hover: rgb(68, 62, 62),     // #443E3E
            btn_border: rgb(68, 68, 79),    // #44444F
            input_bg: rgb(31, 28, 28),      // #1F1C1C
            input_border: rgb(56, 56, 63),
            accent: rgb(59, 130, 246),      // #3B82F6
            accent_hover: rgb(37, 99, 235), // #2563EB
            close_red: rgb(229, 51, 51),    // #E53333
            badge_bg: rgb(46, 42, 42),      // #2E2A2A
            badge_text: rgb(228, 228, 231),
        }
    }

    pub fn light() -> Self {
        Self {
            is_dark: false,
            window_bg: rgb(243, 243, 243),   // #F3F3F3 Win11 Fluent mica light surface
            card_bg: rgb(255, 255, 255),     // #FFFFFF crisp pure white card
            card_border: rgb(229, 231, 235), // #E5E7EB subtle border
            card_hover: rgb(0, 103, 192),    // #0067C0 Win11 Accent Blue
            text_title: rgb(24, 24, 27),     // #18181B dark title
            text_body: rgb(39, 39, 42),      // #27272A dark body
            text_muted: rgb(113, 113, 122),  // #71717A muted gray
            btn_bg: rgb(251, 251, 251),      // #FBFBFB
            btn_hover: rgb(240, 240, 242),   // #F0F0F2
            btn_border: rgb(218, 220, 224),  // #DADCE0
            input_bg: rgb(255, 255, 255),
            input_border: rgb(209, 213, 219),
            accent: rgb(0, 103, 192),        // #0067C0 Win11 Accent Blue
            accent_hover: rgb(0, 90, 170),
            close_red: rgb(196, 43, 28),     // #C42B1C Win11 Close Red
            badge_bg: rgb(243, 244, 246),    // #F3F4F6
            badge_text: rgb(31, 41, 55),
        }
    }
}

pub fn is_windows_light_theme() -> bool {
    unsafe {
        let subkey = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey,
            Some(0),
            KEY_READ,
            &mut hkey,
        ).is_err() {
            return false;
        }

        let mut data: u32 = 0;
        let mut data_len = std::mem::size_of::<u32>() as u32;
        let mut val_type = REG_VALUE_TYPE::default();

        // Check AppsUseLightTheme first
        let status = RegQueryValueExW(
            hkey,
            w!("AppsUseLightTheme"),
            None,
            Some(&mut val_type),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut data_len),
        );

        if status.is_ok() && val_type == REG_DWORD {
            let _ = RegCloseKey(hkey);
            return data == 1;
        }

        // Fall back to SystemUsesLightTheme if AppsUseLightTheme is not found
        data_len = std::mem::size_of::<u32>() as u32;
        let status_sys = RegQueryValueExW(
            hkey,
            w!("SystemUsesLightTheme"),
            None,
            Some(&mut val_type),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut data_len),
        );

        let _ = RegCloseKey(hkey);

        if status_sys.is_ok() && val_type == REG_DWORD {
            data == 1
        } else {
            false
        }
    }
}

pub fn apply_window_theme(hwnd: HWND, is_dark: bool) {
    unsafe {
        let dark_mode: i32 = if is_dark { 1 } else { 0 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

pub fn get_icon_font_name() -> &'static str {
    if std::path::Path::new("C:\\Windows\\Fonts\\SegoeIcons.ttf").exists() {
        "Segoe Fluent Icons"
    } else {
        "Segoe MDL2 Assets"
    }
}

pub unsafe fn create_icon_font(size_px: i32) -> HFONT {
    create_icon_font_px(size_px, 1.0)
}

pub unsafe fn create_icon_font_px(size_px: i32, scale: f32) -> HFONT {
    let font_name = get_icon_font_name();
    let name_w: Vec<u16> = font_name.encode_utf16().chain(std::iter::once(0)).collect();
    let scaled_height = (size_px as f32 * scale).round() as i32;
    CreateFontW(
        -scaled_height,
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        PCWSTR(name_w.as_ptr()),
    )
}

pub mod icons {
    pub const CLOSE: &str = "\u{E8BB}";      // ChromeClose '✕'
    pub const FOLDER: &str = "\u{ED25}";     // Folder
    pub const COPY: &str = "\u{E8C8}";       // Copy
    pub const DELETE: &str = "\u{E74D}";     // Delete / Trash
    pub const CHECKMARK: &str = "\u{E73E}";  // Checkmark
    pub const EXTERNAL: &str = "\u{E8A7}";   // Open in new / Launch
    pub const KEYBOARD: &str = "\u{E765}";   // Keyboard / Hotkey
    pub const TIMER: &str = "\u{E823}";      // Timer / Clock
    pub const SAVE: &str = "\u{E74E}";       // Save
    pub const CLEAR: &str = "\u{E894}";      // Clear / Trash
    pub const ARROW_UP: &str = "\u{E70E}";   // ChevronUp
    pub const ARROW_DOWN: &str = "\u{E70D}"; // ChevronDown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_query() {
        let is_light = is_windows_light_theme();
        println!("Detected Windows theme: is_light = {is_light}");
        let theme = ThemeColors::current();
        assert_eq!(theme.is_dark, !is_light);
    }

    #[test]
    fn test_icon_font_detection() {
        let font_name = get_icon_font_name();
        assert!(font_name == "Segoe Fluent Icons" || font_name == "Segoe MDL2 Assets");
    }
}
