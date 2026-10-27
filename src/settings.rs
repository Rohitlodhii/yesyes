use std::path::PathBuf;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::config::AppConfig;
use crate::hotkey::format_hotkey;

const WM_MOUSELEAVE: u32 = 0x02A3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonId {
    ChangeHotkey,
    LifespanInfinite,
    LifespanTimed,
    LifespanDisabled,
    LifespanDec,
    LifespanInc,
    BrowseFolder,
    OpenFolder,
    SaveApply,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LifespanMode {
    Infinite,
    Timed(u32),
    Disabled,
}

struct SettingsState {
    main_hwnd: HWND,
    _current_config: AppConfig,

    pending_modifiers: u32,
    pending_vk: u32,
    pending_lifespan: LifespanMode,
    pending_save_dir: PathBuf,

    recording_hotkey: bool,
    hovered_button: Option<ButtonId>,

    font_title: HFONT,
    font_section: HFONT,
    font_body: HFONT,
    font_badge: HFONT,
}

impl SettingsState {
    fn new(main_hwnd: HWND, cfg: &AppConfig) -> Self {
        let lifespan_mode = match cfg.thumbnail_lifespan_secs {
            None => LifespanMode::Infinite,
            Some(0) => LifespanMode::Disabled,
            Some(n) => LifespanMode::Timed(n),
        };

        unsafe {
            let font_title = create_font(18, 700);
            let font_section = create_font(14, 600);
            let font_body = create_font(13, 400);
            let font_badge = create_font(14, 600);

            Self {
                main_hwnd,
                _current_config: cfg.clone(),
                pending_modifiers: cfg.hotkey_modifiers,
                pending_vk: cfg.hotkey_vk,
                pending_lifespan: lifespan_mode,
                pending_save_dir: cfg.save_directory.clone(),
                recording_hotkey: false,
                hovered_button: None,
                font_title,
                font_section,
                font_body,
                font_badge,
            }
        }
    }
}

impl Drop for SettingsState {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.font_title.0));
            let _ = DeleteObject(HGDIOBJ(self.font_section.0));
            let _ = DeleteObject(HGDIOBJ(self.font_body.0));
            let _ = DeleteObject(HGDIOBJ(self.font_badge.0));
        }
    }
}

unsafe fn create_font(size: i32, weight: i32) -> HFONT {
    let font_name: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
    CreateFontW(
        -size,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        PCWSTR(font_name.as_ptr()),
    )
}

// BGR color constants for Win32 COLORREF
const COLOR_BG: COLORREF = COLORREF(0x001B1818);         // #18181B (Zinc 900)
const COLOR_CARD: COLORREF = COLORREF(0x002A2727);       // #27272A (Zinc 800)
const COLOR_CARD_BORDER: COLORREF = COLORREF(0x00463F3F);// #3F3F46 (Zinc 700)
const COLOR_TEXT_WHITE: COLORREF = COLORREF(0x00F5F4F4); // #F4F4F5
const COLOR_TEXT_MUTED: COLORREF = COLORREF(0x00AAA1A1); // #A1A1AA
const COLOR_PRIMARY: COLORREF = COLORREF(0x00EB6325);    // #2563EB (Blue 600)
const COLOR_PRIMARY_HOVER: COLORREF = COLORREF(0x00F6823B); // #3B82F6 (Blue 500)
const COLOR_ACTIVE_PILL: COLORREF = COLORREF(0x006E3B1E);   // Dark Blue highlight
const COLOR_BTN_BG: COLORREF = COLORREF(0x003F3737);     // #37373F
const COLOR_BTN_HOVER: COLORREF = COLORREF(0x00524C4C);  // #4C4C52
const COLOR_AMBER: COLORREF = COLORREF(0x000B9EF5);      // #F59E0B (Amber)

pub fn pick_folder(parent: HWND) -> Option<PathBuf> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let options = dialog.GetOptions().ok()?;
        let _ = dialog.SetOptions(options | FOS_PICKFOLDERS);
        let _ = dialog.SetTitle(w!("Select Screenshots Folder"));

        if dialog.Show(Some(parent)).is_ok() {
            let item: IShellItem = dialog.GetResult().ok()?;
            let display_name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            let path_str = display_name.to_string().ok()?;
            CoTaskMemFree(Some(display_name.as_ptr() as *const _));
            Some(PathBuf::from(path_str))
        } else {
            None
        }
    }
}

// Layout coordinate constants (Window width = 520, height = 540)
const WIN_W: i32 = 520;
const WIN_H: i32 = 540;

// Button Hitbox Rectangles
const RECT_CHANGE_HOTKEY: RECT = RECT { left: 340, top: 62, right: 470, bottom: 98 };

const RECT_LIFESPAN_INF: RECT = RECT { left: 40, top: 180, right: 180, bottom: 215 };
const RECT_LIFESPAN_TIMED: RECT = RECT { left: 190, top: 180, right: 300, bottom: 215 };
const RECT_LIFESPAN_DIS: RECT = RECT { left: 310, top: 180, right: 430, bottom: 215 };

const RECT_LIFESPAN_DEC: RECT = RECT { left: 190, top: 225, right: 225, bottom: 255 };
const RECT_LIFESPAN_INC: RECT = RECT { left: 315, top: 225, right: 350, bottom: 255 };

const RECT_BROWSE_FOLDER: RECT = RECT { left: 40, top: 380, right: 150, bottom: 412 };
const RECT_OPEN_FOLDER: RECT = RECT { left: 160, top: 380, right: 270, bottom: 412 };

const RECT_SAVE_APPLY: RECT = RECT { left: 260, top: 460, right: 390, bottom: 498 };
const RECT_CANCEL: RECT = RECT { left: 405, top: 460, right: 480, bottom: 498 };

fn hit_test(pt: POINT) -> Option<ButtonId> {
    unsafe {
        if PtInRect(&RECT_CHANGE_HOTKEY, pt).as_bool() {
            return Some(ButtonId::ChangeHotkey);
        }
        if PtInRect(&RECT_LIFESPAN_INF, pt).as_bool() {
            return Some(ButtonId::LifespanInfinite);
        }
        if PtInRect(&RECT_LIFESPAN_TIMED, pt).as_bool() {
            return Some(ButtonId::LifespanTimed);
        }
        if PtInRect(&RECT_LIFESPAN_DIS, pt).as_bool() {
            return Some(ButtonId::LifespanDisabled);
        }
        if PtInRect(&RECT_LIFESPAN_DEC, pt).as_bool() {
            return Some(ButtonId::LifespanDec);
        }
        if PtInRect(&RECT_LIFESPAN_INC, pt).as_bool() {
            return Some(ButtonId::LifespanInc);
        }
        if PtInRect(&RECT_BROWSE_FOLDER, pt).as_bool() {
            return Some(ButtonId::BrowseFolder);
        }
        if PtInRect(&RECT_OPEN_FOLDER, pt).as_bool() {
            return Some(ButtonId::OpenFolder);
        }
        if PtInRect(&RECT_SAVE_APPLY, pt).as_bool() {
            return Some(ButtonId::SaveApply);
        }
        if PtInRect(&RECT_CANCEL, pt).as_bool() {
            return Some(ButtonId::Cancel);
        }
    }
    None
}

unsafe fn draw_rounded_card(hdc: HDC, rect: &RECT, bg: COLORREF, border: COLORREF) {
    let brush = CreateSolidBrush(bg);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));

    let _ = RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, 12, 12);

    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

unsafe fn draw_button(
    hdc: HDC,
    rect: &RECT,
    text: &str,
    font: HFONT,
    bg: COLORREF,
    text_color: COLORREF,
    border: COLORREF,
) {
    draw_rounded_card(hdc, rect, bg, border);

    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, text_color);

    let mut text_w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let mut r = *rect;
    DrawTextW(hdc, &mut text_w, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    SelectObject(hdc, old_font);
}

unsafe fn paint_settings(hwnd: HWND, state: &SettingsState) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let mut cr = RECT::default();
    let _ = GetClientRect(hwnd, &mut cr);

    let mem_dc = CreateCompatibleDC(Some(hdc));
    let mem_bmp = CreateCompatibleBitmap(hdc, cr.right, cr.bottom);
    let old_bmp = SelectObject(mem_dc, HGDIOBJ(mem_bmp.0));

    // Fill main dark background
    let bg_brush = CreateSolidBrush(COLOR_BG);
    FillRect(mem_dc, &cr, bg_brush);
    let _ = DeleteObject(HGDIOBJ(bg_brush.0));

    let _ = SetBkMode(mem_dc, TRANSPARENT);

    // --- CARD 1: HOTKEY ---
    let card1 = RECT { left: 24, top: 20, right: cr.right - 24, bottom: 120 };
    draw_rounded_card(mem_dc, &card1, COLOR_CARD, COLOR_CARD_BORDER);

    let old_font = SelectObject(mem_dc, HGDIOBJ(state.font_section.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut t1: Vec<u16> = "CAPTURE SHORTCUT\0".encode_utf16().collect();
    let mut r1 = RECT { left: 40, top: 32, right: 300, bottom: 52 };
    DrawTextW(mem_dc, &mut t1, &mut r1, DT_LEFT | DT_SINGLELINE);

    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_MUTED);
    let mut sub1: Vec<u16> = "Global key combination to freeze and crop screen\0".encode_utf16().collect();
    let mut r_sub1 = RECT { left: 40, top: 54, right: 330, bottom: 74 };
    DrawTextW(mem_dc, &mut sub1, &mut r_sub1, DT_LEFT | DT_SINGLELINE);

    // Hotkey display badge
    let hotkey_str = format_hotkey(state.pending_modifiers, state.pending_vk);
    let badge_rect = RECT { left: 40, top: 76, right: 240, bottom: 108 };
    draw_rounded_card(mem_dc, &badge_rect, COLOR_BG, COLOR_PRIMARY);
    SelectObject(mem_dc, HGDIOBJ(state.font_badge.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut hk_w: Vec<u16> = format!("{hotkey_str}\0").encode_utf16().collect();
    let mut r_hk = badge_rect;
    DrawTextW(mem_dc, &mut hk_w, &mut r_hk, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    // Change Button
    let (chg_text, chg_bg, chg_border) = if state.recording_hotkey {
        ("Press keys...", COLOR_AMBER, COLOR_AMBER)
    } else if state.hovered_button == Some(ButtonId::ChangeHotkey) {
        ("Change...", COLOR_BTN_HOVER, COLOR_PRIMARY_HOVER)
    } else {
        ("Change...", COLOR_BTN_BG, COLOR_CARD_BORDER)
    };
    draw_button(mem_dc, &RECT_CHANGE_HOTKEY, chg_text, state.font_section, chg_bg, COLOR_TEXT_WHITE, chg_border);

    // --- CARD 2: FLOATING THUMBNAIL LIFESPAN ---
    let card2 = RECT { left: 24, top: 135, right: cr.right - 24, bottom: 275 };
    draw_rounded_card(mem_dc, &card2, COLOR_CARD, COLOR_CARD_BORDER);

    SelectObject(mem_dc, HGDIOBJ(state.font_section.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut t2: Vec<u16> = "FLOATING THUMBNAIL\0".encode_utf16().collect();
    let mut r2 = RECT { left: 40, top: 147, right: 300, bottom: 167 };
    DrawTextW(mem_dc, &mut t2, &mut r2, DT_LEFT | DT_SINGLELINE);

    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_MUTED);
    let mut sub2: Vec<u16> = "Preview popup at bottom-right corner after capture\0".encode_utf16().collect();
    let mut r_sub2 = RECT { left: 40, top: 167, right: 460, bottom: 187 };
    DrawTextW(mem_dc, &mut sub2, &mut r_sub2, DT_LEFT | DT_SINGLELINE);

    // Pill: Infinite
    let is_inf = state.pending_lifespan == LifespanMode::Infinite;
    let inf_bg = if is_inf { COLOR_ACTIVE_PILL } else if state.hovered_button == Some(ButtonId::LifespanInfinite) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
    let inf_border = if is_inf { COLOR_PRIMARY_HOVER } else { COLOR_CARD_BORDER };
    let inf_txt_color = if is_inf { COLOR_TEXT_WHITE } else { COLOR_TEXT_MUTED };
    draw_button(mem_dc, &RECT_LIFESPAN_INF, "Keep until closed", state.font_body, inf_bg, inf_txt_color, inf_border);

    // Pill: Timed
    let is_timed = matches!(state.pending_lifespan, LifespanMode::Timed(_));
    let timed_bg = if is_timed { COLOR_ACTIVE_PILL } else if state.hovered_button == Some(ButtonId::LifespanTimed) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
    let timed_border = if is_timed { COLOR_PRIMARY_HOVER } else { COLOR_CARD_BORDER };
    let timed_txt_color = if is_timed { COLOR_TEXT_WHITE } else { COLOR_TEXT_MUTED };
    draw_button(mem_dc, &RECT_LIFESPAN_TIMED, "Timed", state.font_body, timed_bg, timed_txt_color, timed_border);

    // Pill: Disabled
    let is_dis = state.pending_lifespan == LifespanMode::Disabled;
    let dis_bg = if is_dis { COLOR_ACTIVE_PILL } else if state.hovered_button == Some(ButtonId::LifespanDisabled) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
    let dis_border = if is_dis { COLOR_PRIMARY_HOVER } else { COLOR_CARD_BORDER };
    let dis_txt_color = if is_dis { COLOR_TEXT_WHITE } else { COLOR_TEXT_MUTED };
    draw_button(mem_dc, &RECT_LIFESPAN_DIS, "Disable", state.font_body, dis_bg, dis_txt_color, dis_border);

    // Timed stepper controls (visible/active when Timed mode is selected)
    if let LifespanMode::Timed(secs) = state.pending_lifespan {
        let dec_bg = if state.hovered_button == Some(ButtonId::LifespanDec) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
        draw_button(mem_dc, &RECT_LIFESPAN_DEC, "-", state.font_section, dec_bg, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);

        let dur_rect = RECT { left: 230, top: 225, right: 310, bottom: 255 };
        SelectObject(mem_dc, HGDIOBJ(state.font_section.0));
        SetTextColor(mem_dc, COLOR_TEXT_WHITE);
        let mut dur_w: Vec<u16> = format!("{secs} seconds\0").encode_utf16().collect();
        let mut r_dur = dur_rect;
        DrawTextW(mem_dc, &mut dur_w, &mut r_dur, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        let inc_bg = if state.hovered_button == Some(ButtonId::LifespanInc) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
        draw_button(mem_dc, &RECT_LIFESPAN_INC, "+", state.font_section, inc_bg, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);
    }

    // --- CARD 3: SAVE LOCATION ---
    let card3 = RECT { left: 24, top: 290, right: cr.right - 24, bottom: 430 };
    draw_rounded_card(mem_dc, &card3, COLOR_CARD, COLOR_CARD_BORDER);

    SelectObject(mem_dc, HGDIOBJ(state.font_section.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut t3: Vec<u16> = "SAVE LOCATION\0".encode_utf16().collect();
    let mut r3 = RECT { left: 40, top: 302, right: 300, bottom: 322 };
    DrawTextW(mem_dc, &mut t3, &mut r3, DT_LEFT | DT_SINGLELINE);

    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_MUTED);
    let mut sub3: Vec<u16> = "Folder where full-resolution screenshots are saved\0".encode_utf16().collect();
    let mut r_sub3 = RECT { left: 40, top: 322, right: 460, bottom: 342 };
    DrawTextW(mem_dc, &mut sub3, &mut r_sub3, DT_LEFT | DT_SINGLELINE);

    // Path display box
    let path_rect = RECT { left: 40, top: 344, right: cr.right - 40, bottom: 372 };
    draw_rounded_card(mem_dc, &path_rect, COLOR_BG, COLOR_CARD_BORDER);
    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let path_str = state.pending_save_dir.to_string_lossy();
    let mut path_w: Vec<u16> = format!("{path_str}\0").encode_utf16().collect();
    let mut r_path = RECT { left: 50, top: 344, right: cr.right - 50, bottom: 372 };
    DrawTextW(mem_dc, &mut path_w, &mut r_path, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS);

    // Browse and Open buttons
    let browse_bg = if state.hovered_button == Some(ButtonId::BrowseFolder) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
    draw_button(mem_dc, &RECT_BROWSE_FOLDER, "Browse...", state.font_body, browse_bg, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);

    let open_bg = if state.hovered_button == Some(ButtonId::OpenFolder) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
    draw_button(mem_dc, &RECT_OPEN_FOLDER, "Open Folder", state.font_body, open_bg, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);

    // --- FOOTER BUTTONS ---
    let save_bg = if state.hovered_button == Some(ButtonId::SaveApply) { COLOR_PRIMARY_HOVER } else { COLOR_PRIMARY };
    draw_button(mem_dc, &RECT_SAVE_APPLY, "Save & Apply", state.font_section, save_bg, COLOR_TEXT_WHITE, COLOR_PRIMARY_HOVER);

    let cancel_bg = if state.hovered_button == Some(ButtonId::Cancel) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
    draw_button(mem_dc, &RECT_CANCEL, "Cancel", state.font_section, cancel_bg, COLOR_TEXT_MUTED, COLOR_CARD_BORDER);

    SelectObject(mem_dc, old_font);

    // Blit from backbuffer to screen
    let _ = BitBlt(hdc, 0, 0, cr.right, cr.bottom, Some(mem_dc), 0, 0, SRCCOPY);

    SelectObject(mem_dc, old_bmp);
    let _ = DeleteObject(HGDIOBJ(mem_bmp.0));
    let _ = DeleteDC(mem_dc);

    let _ = EndPaint(hwnd, &ps);
}

pub const WM_SNAPDOCK_SETTINGS_SAVED: u32 = WM_APP + 5;

unsafe extern "system" fn settings_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *state_ptr;

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint_settings(hwnd, state);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(hwnd, &mut pt);
            if hit_test(pt).is_some() {
                let cursor = LoadCursorW(None, IDC_HAND).unwrap_or_default();
                SetCursor(Some(cursor));
                return LRESULT(1);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_MOUSEMOVE => {
            let x = (lparam.0 & 0xffff) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let pt = POINT { x, y };

            let hit = hit_test(pt);
            if hit != state.hovered_button {
                state.hovered_button = hit;
                let _ = InvalidateRect(Some(hwnd), None, false);

                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if state.hovered_button.is_some() {
                state.hovered_button = None;
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            if state.recording_hotkey {
                let vk = wparam.0 as u32;

                // Escape cancels recording
                if vk == 0x1B {
                    state.recording_hotkey = false;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    return LRESULT(0);
                }

                // Ignore lone modifier presses
                if vk != 0x10 && vk != 0x11 && vk != 0x12 && vk != 0x5B && vk != 0x5C {
                    let mut mods = 0u32;
                    if (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 {
                        mods |= MOD_CONTROL.0;
                    }
                    if (GetKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 {
                        mods |= MOD_ALT.0;
                    }
                    if (GetKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 {
                        mods |= MOD_SHIFT.0;
                    }
                    let lwin = (GetKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0;
                    let rwin = (GetKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0;
                    if lwin || rwin {
                        mods |= MOD_WIN.0;
                    }

                    // Must have at least one modifier unless it's a function key (F1-F24) or PrintScreen
                    let is_f_key = (0x70..=0x87).contains(&vk);
                    let is_prt_scr = vk == 0x2C;
                    if mods != 0 || is_f_key || is_prt_scr {
                        state.pending_modifiers = mods;
                        state.pending_vk = vk;
                        state.recording_hotkey = false;
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xffff) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let pt = POINT { x, y };

            match hit_test(pt) {
                Some(ButtonId::ChangeHotkey) => {
                    state.recording_hotkey = !state.recording_hotkey;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                Some(ButtonId::LifespanInfinite) => {
                    state.pending_lifespan = LifespanMode::Infinite;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                Some(ButtonId::LifespanTimed) => {
                    state.pending_lifespan = LifespanMode::Timed(8);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                Some(ButtonId::LifespanDisabled) => {
                    state.pending_lifespan = LifespanMode::Disabled;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                Some(ButtonId::LifespanDec) => {
                    if let LifespanMode::Timed(secs) = state.pending_lifespan {
                        if secs > 2 {
                            state.pending_lifespan = LifespanMode::Timed(secs - 1);
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                }
                Some(ButtonId::LifespanInc) => {
                    if let LifespanMode::Timed(secs) = state.pending_lifespan {
                        if secs < 60 {
                            state.pending_lifespan = LifespanMode::Timed(secs + 1);
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                }
                Some(ButtonId::BrowseFolder) => {
                    if let Some(folder) = pick_folder(hwnd) {
                        state.pending_save_dir = folder;
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
                Some(ButtonId::OpenFolder) => {
                    let dir_w: Vec<u16> = state.pending_save_dir.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
                    let _ = ShellExecuteW(
                        Some(hwnd),
                        w!("open"),
                        PCWSTR(dir_w.as_ptr()),
                        None,
                        None,
                        SW_SHOWNORMAL,
                    );
                }
                Some(ButtonId::SaveApply) => {
                    // Update AppConfig
                    let lifespan_opt = match state.pending_lifespan {
                        LifespanMode::Infinite => None,
                        LifespanMode::Disabled => Some(0),
                        LifespanMode::Timed(s) => Some(s),
                    };

                    let new_cfg = AppConfig {
                        hotkey_modifiers: state.pending_modifiers,
                        hotkey_vk: state.pending_vk,
                        thumbnail_lifespan_secs: lifespan_opt,
                        save_directory: state.pending_save_dir.clone(),
                    };

                    // Save to %APPDATA%\Snapdock\config.json
                    if let Err(e) = new_cfg.save() {
                        let msg: Vec<u16> = format!("Failed to save config: {e}\0").encode_utf16().collect();
                        MessageBoxW(Some(hwnd), PCWSTR(msg.as_ptr()), w!("Error"), MB_ICONERROR);
                        return LRESULT(0);
                    }

                    // Notify main window to update its active config and rebind hotkey
                    let boxed = Box::new(new_cfg);
                    let ptr = Box::into_raw(boxed);
                    let _ = SendMessageW(
                        state.main_hwnd,
                        WM_SNAPDOCK_SETTINGS_SAVED,
                        Some(WPARAM(ptr as usize)),
                        Some(LPARAM(0)),
                    );

                    let _ = DestroyWindow(hwnd);
                }
                Some(ButtonId::Cancel) => {
                    let _ = DestroyWindow(hwnd);
                }
                None => {
                    if state.recording_hotkey {
                        state.recording_hotkey = false;
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // Reclaim SettingsState Box
            let _ = Box::from_raw(state_ptr);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn open_settings_window(main_hwnd: HWND, cfg: &AppConfig) -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class_name = w!("SnapdockSettingsWindow");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(settings_wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        // Center on screen
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let x = (screen_w - WIN_W) / 2;
        let y = (screen_h - WIN_H) / 2;

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Snapdock Settings"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            x,
            y,
            WIN_W,
            WIN_H,
            None,
            None,
            Some(instance.into()),
            None,
        ).ok()?;

        // Enable Windows 11 Immersive Dark Mode on titlebar & borders
        let dark_mode: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );

        let state = Box::new(SettingsState::new(main_hwnd, cfg));
        let state_raw = Box::into_raw(state);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_raw as isize);

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);

        Some(hwnd)
    }
}
