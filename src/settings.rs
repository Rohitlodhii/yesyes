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
enum RecordingTarget {
    Region,
    Fullscreen,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonId {
    ChangeRegionHotkey,
    ChangeFullscreenHotkey,
    ChangeHistoryHotkey,
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

    pending_region_mods: u32,
    pending_region_vk: u32,

    pending_fullscreen_mods: u32,
    pending_fullscreen_vk: u32,

    pending_history_mods: u32,
    pending_history_vk: u32,

    pending_lifespan: LifespanMode,
    pending_save_dir: PathBuf,

    recording_target: Option<RecordingTarget>,
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
            let font_badge = create_font(13, 600);

            Self {
                main_hwnd,
                _current_config: cfg.clone(),
                pending_region_mods: cfg.region_hotkey_modifiers,
                pending_region_vk: cfg.region_hotkey_vk,
                pending_fullscreen_mods: cfg.fullscreen_hotkey_modifiers,
                pending_fullscreen_vk: cfg.fullscreen_hotkey_vk,
                pending_history_mods: cfg.history_hotkey_modifiers,
                pending_history_vk: cfg.history_hotkey_vk,
                pending_lifespan: lifespan_mode,
                pending_save_dir: cfg.save_directory.clone(),
                recording_target: None,
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

// Layout coordinate constants (Window width = 560, height = 680)
const WIN_W: i32 = 560;
const WIN_H: i32 = 680;

// Hitbox Rectangles
const RECT_CHANGE_REGION: RECT = RECT { left: 405, top: 76, right: 520, bottom: 108 };
const RECT_CHANGE_FULLSCREEN: RECT = RECT { left: 405, top: 121, right: 520, bottom: 153 };
const RECT_CHANGE_HISTORY: RECT = RECT { left: 405, top: 166, right: 520, bottom: 198 };

const RECT_LIFESPAN_INF: RECT = RECT { left: 40, top: 310, right: 180, bottom: 345 };
const RECT_LIFESPAN_TIMED: RECT = RECT { left: 195, top: 310, right: 305, bottom: 345 };
const RECT_LIFESPAN_DIS: RECT = RECT { left: 320, top: 310, right: 440, bottom: 345 };

const RECT_LIFESPAN_DEC: RECT = RECT { left: 195, top: 355, right: 230, bottom: 385 };
const RECT_LIFESPAN_INC: RECT = RECT { left: 330, top: 355, right: 365, bottom: 385 };

const RECT_BROWSE_FOLDER: RECT = RECT { left: 40, top: 512, right: 150, bottom: 544 };
const RECT_OPEN_FOLDER: RECT = RECT { left: 160, top: 512, right: 270, bottom: 544 };

const RECT_SAVE_APPLY: RECT = RECT { left: 280, top: 590, right: 425, bottom: 630 };
const RECT_CANCEL: RECT = RECT { left: 440, top: 590, right: 520, bottom: 630 };

fn hit_test(pt: POINT) -> Option<ButtonId> {
    unsafe {
        if PtInRect(&RECT_CHANGE_REGION, pt).as_bool() {
            return Some(ButtonId::ChangeRegionHotkey);
        }
        if PtInRect(&RECT_CHANGE_FULLSCREEN, pt).as_bool() {
            return Some(ButtonId::ChangeFullscreenHotkey);
        }
        if PtInRect(&RECT_CHANGE_HISTORY, pt).as_bool() {
            return Some(ButtonId::ChangeHistoryHotkey);
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

    // Fill background
    let bg_brush = CreateSolidBrush(COLOR_BG);
    FillRect(mem_dc, &cr, bg_brush);
    let _ = DeleteObject(HGDIOBJ(bg_brush.0));

    let _ = SetBkMode(mem_dc, TRANSPARENT);

    // --- CARD 1: KEYBOARD SHORTCUTS ---
    let card1 = RECT { left: 24, top: 16, right: cr.right - 24, bottom: 250 };
    draw_rounded_card(mem_dc, &card1, COLOR_CARD, COLOR_CARD_BORDER);

    let old_font = SelectObject(mem_dc, HGDIOBJ(state.font_section.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut t1: Vec<u16> = "KEYBOARD SHORTCUTS\0".encode_utf16().collect();
    let mut r1 = RECT { left: 40, top: 28, right: 300, bottom: 48 };
    DrawTextW(mem_dc, &mut t1, &mut r1, DT_LEFT | DT_SINGLELINE);

    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_MUTED);
    let mut sub1: Vec<u16> = "Global hotkeys to trigger capture actions and history\0".encode_utf16().collect();
    let mut r_sub1 = RECT { left: 40, top: 48, right: 460, bottom: 68 };
    DrawTextW(mem_dc, &mut sub1, &mut r_sub1, DT_LEFT | DT_SINGLELINE);

    // Row 1: Region Capture
    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut lbl1: Vec<u16> = "Region Capture\0".encode_utf16().collect();
    let mut r_lbl1 = RECT { left: 40, top: 82, right: 200, bottom: 104 };
    DrawTextW(mem_dc, &mut lbl1, &mut r_lbl1, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    let reg_badge_rect = RECT { left: 200, top: 76, right: 395, bottom: 108 };
    draw_rounded_card(mem_dc, &reg_badge_rect, COLOR_BG, COLOR_PRIMARY);
    SelectObject(mem_dc, HGDIOBJ(state.font_badge.0));
    let reg_hk_str = format_hotkey(state.pending_region_mods, state.pending_region_vk);
    let mut reg_hk_w: Vec<u16> = format!("{reg_hk_str}\0").encode_utf16().collect();
    let mut r_rhk = reg_badge_rect;
    DrawTextW(mem_dc, &mut reg_hk_w, &mut r_rhk, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    let is_rec_reg = state.recording_target == Some(RecordingTarget::Region);
    let (reg_btn_txt, reg_btn_bg) = if is_rec_reg { ("Press keys...", COLOR_AMBER) } else if state.hovered_button == Some(ButtonId::ChangeRegionHotkey) { ("Change...", COLOR_BTN_HOVER) } else { ("Change...", COLOR_BTN_BG) };
    draw_button(mem_dc, &RECT_CHANGE_REGION, reg_btn_txt, state.font_section, reg_btn_bg, COLOR_TEXT_WHITE, if is_rec_reg { COLOR_AMBER } else { COLOR_CARD_BORDER });

    // Row 2: Full Screen Capture
    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut lbl2: Vec<u16> = "Full Screen\0".encode_utf16().collect();
    let mut r_lbl2 = RECT { left: 40, top: 127, right: 200, bottom: 149 };
    DrawTextW(mem_dc, &mut lbl2, &mut r_lbl2, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    let fs_badge_rect = RECT { left: 200, top: 121, right: 395, bottom: 153 };
    draw_rounded_card(mem_dc, &fs_badge_rect, COLOR_BG, COLOR_PRIMARY);
    SelectObject(mem_dc, HGDIOBJ(state.font_badge.0));
    let fs_hk_str = format_hotkey(state.pending_fullscreen_mods, state.pending_fullscreen_vk);
    let mut fs_hk_w: Vec<u16> = format!("{fs_hk_str}\0").encode_utf16().collect();
    let mut r_fshk = fs_badge_rect;
    DrawTextW(mem_dc, &mut fs_hk_w, &mut r_fshk, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    let is_rec_fs = state.recording_target == Some(RecordingTarget::Fullscreen);
    let (fs_btn_txt, fs_btn_bg) = if is_rec_fs { ("Press keys...", COLOR_AMBER) } else if state.hovered_button == Some(ButtonId::ChangeFullscreenHotkey) { ("Change...", COLOR_BTN_HOVER) } else { ("Change...", COLOR_BTN_BG) };
    draw_button(mem_dc, &RECT_CHANGE_FULLSCREEN, fs_btn_txt, state.font_section, fs_btn_bg, COLOR_TEXT_WHITE, if is_rec_fs { COLOR_AMBER } else { COLOR_CARD_BORDER });

    // Row 3: History Sidebar
    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut lbl3: Vec<u16> = "History Sidebar\0".encode_utf16().collect();
    let mut r_lbl3 = RECT { left: 40, top: 172, right: 200, bottom: 194 };
    DrawTextW(mem_dc, &mut lbl3, &mut r_lbl3, DT_LEFT | DT_VCENTER | DT_SINGLELINE);

    let hist_badge_rect = RECT { left: 200, top: 166, right: 395, bottom: 198 };
    draw_rounded_card(mem_dc, &hist_badge_rect, COLOR_BG, COLOR_PRIMARY);
    SelectObject(mem_dc, HGDIOBJ(state.font_badge.0));
    let hist_hk_str = format_hotkey(state.pending_history_mods, state.pending_history_vk);
    let mut hist_hk_w: Vec<u16> = format!("{hist_hk_str}\0").encode_utf16().collect();
    let mut r_histhk = hist_badge_rect;
    DrawTextW(mem_dc, &mut hist_hk_w, &mut r_histhk, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

    let is_rec_hist = state.recording_target == Some(RecordingTarget::History);
    let (hist_btn_txt, hist_btn_bg) = if is_rec_hist { ("Press keys...", COLOR_AMBER) } else if state.hovered_button == Some(ButtonId::ChangeHistoryHotkey) { ("Change...", COLOR_BTN_HOVER) } else { ("Change...", COLOR_BTN_BG) };
    draw_button(mem_dc, &RECT_CHANGE_HISTORY, hist_btn_txt, state.font_section, hist_btn_bg, COLOR_TEXT_WHITE, if is_rec_hist { COLOR_AMBER } else { COLOR_CARD_BORDER });

    // Bottom note in Card 1
    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_MUTED);
    let mut note1: Vec<u16> = "Press any key combination while recording. Esc cancels.\0".encode_utf16().collect();
    let mut r_note1 = RECT { left: 40, top: 218, right: 500, bottom: 238 };
    DrawTextW(mem_dc, &mut note1, &mut r_note1, DT_LEFT | DT_SINGLELINE);

    // --- CARD 2: FLOATING THUMBNAIL LIFESPAN ---
    let card2 = RECT { left: 24, top: 265, right: cr.right - 24, bottom: 405 };
    draw_rounded_card(mem_dc, &card2, COLOR_CARD, COLOR_CARD_BORDER);

    SelectObject(mem_dc, HGDIOBJ(state.font_section.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut t2: Vec<u16> = "FLOATING THUMBNAIL\0".encode_utf16().collect();
    let mut r2 = RECT { left: 40, top: 277, right: 300, bottom: 297 };
    DrawTextW(mem_dc, &mut t2, &mut r2, DT_LEFT | DT_SINGLELINE);

    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_MUTED);
    let mut sub2: Vec<u16> = "Preview popup at bottom-right corner after capture\0".encode_utf16().collect();
    let mut r_sub2 = RECT { left: 40, top: 295, right: 460, bottom: 315 };
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

    // Timed stepper controls
    if let LifespanMode::Timed(secs) = state.pending_lifespan {
        let dec_bg = if state.hovered_button == Some(ButtonId::LifespanDec) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
        draw_button(mem_dc, &RECT_LIFESPAN_DEC, "-", state.font_section, dec_bg, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);

        let dur_rect = RECT { left: 235, top: 355, right: 325, bottom: 385 };
        SelectObject(mem_dc, HGDIOBJ(state.font_section.0));
        SetTextColor(mem_dc, COLOR_TEXT_WHITE);
        let mut dur_w: Vec<u16> = format!("{secs} seconds\0").encode_utf16().collect();
        let mut r_dur = dur_rect;
        DrawTextW(mem_dc, &mut dur_w, &mut r_dur, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        let inc_bg = if state.hovered_button == Some(ButtonId::LifespanInc) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
        draw_button(mem_dc, &RECT_LIFESPAN_INC, "+", state.font_section, inc_bg, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);
    }

    // --- CARD 3: SAVE LOCATION ---
    let card3 = RECT { left: 24, top: 420, right: cr.right - 24, bottom: 560 };
    draw_rounded_card(mem_dc, &card3, COLOR_CARD, COLOR_CARD_BORDER);

    SelectObject(mem_dc, HGDIOBJ(state.font_section.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut t3: Vec<u16> = "SAVE LOCATION\0".encode_utf16().collect();
    let mut r3 = RECT { left: 40, top: 432, right: 300, bottom: 452 };
    DrawTextW(mem_dc, &mut t3, &mut r3, DT_LEFT | DT_SINGLELINE);

    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_MUTED);
    let mut sub3: Vec<u16> = "Folder where full-resolution screenshots are saved\0".encode_utf16().collect();
    let mut r_sub3 = RECT { left: 40, top: 452, right: 460, bottom: 472 };
    DrawTextW(mem_dc, &mut sub3, &mut r_sub3, DT_LEFT | DT_SINGLELINE);

    // Path display box
    let path_rect = RECT { left: 40, top: 474, right: cr.right - 40, bottom: 502 };
    draw_rounded_card(mem_dc, &path_rect, COLOR_BG, COLOR_CARD_BORDER);
    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let path_str = state.pending_save_dir.to_string_lossy();
    let mut path_w: Vec<u16> = format!("{path_str}\0").encode_utf16().collect();
    let mut r_path = RECT { left: 50, top: 474, right: cr.right - 50, bottom: 502 };
    DrawTextW(mem_dc, &mut path_w, &mut r_path, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS);

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
            if let Some(target) = state.recording_target {
                let vk = wparam.0 as u32;

                if vk == 0x1B { // Esc cancels recording
                    state.recording_target = None;
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

                    let is_f_key = (0x70..=0x87).contains(&vk);
                    let is_prt_scr = vk == 0x2C;
                    let is_space = vk == 0x20;
                    if mods != 0 || is_f_key || is_prt_scr || is_space {
                        match target {
                            RecordingTarget::Region => {
                                state.pending_region_mods = mods;
                                state.pending_region_vk = vk;
                            }
                            RecordingTarget::Fullscreen => {
                                state.pending_fullscreen_mods = mods;
                                state.pending_fullscreen_vk = vk;
                            }
                            RecordingTarget::History => {
                                state.pending_history_mods = mods;
                                state.pending_history_vk = vk;
                            }
                        }
                        state.recording_target = None;
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
                Some(ButtonId::ChangeRegionHotkey) => {
                    state.recording_target = if state.recording_target == Some(RecordingTarget::Region) { None } else { Some(RecordingTarget::Region) };
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                Some(ButtonId::ChangeFullscreenHotkey) => {
                    state.recording_target = if state.recording_target == Some(RecordingTarget::Fullscreen) { None } else { Some(RecordingTarget::Fullscreen) };
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                Some(ButtonId::ChangeHistoryHotkey) => {
                    state.recording_target = if state.recording_target == Some(RecordingTarget::History) { None } else { Some(RecordingTarget::History) };
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
                    let lifespan_opt = match state.pending_lifespan {
                        LifespanMode::Infinite => None,
                        LifespanMode::Disabled => Some(0),
                        LifespanMode::Timed(s) => Some(s),
                    };

                    let new_cfg = AppConfig {
                        region_hotkey_modifiers: state.pending_region_mods,
                        region_hotkey_vk: state.pending_region_vk,
                        fullscreen_hotkey_modifiers: state.pending_fullscreen_mods,
                        fullscreen_hotkey_vk: state.pending_fullscreen_vk,
                        history_hotkey_modifiers: state.pending_history_mods,
                        history_hotkey_vk: state.pending_history_vk,
                        thumbnail_lifespan_secs: lifespan_opt,
                        save_directory: state.pending_save_dir.clone(),
                    };

                    if let Err(e) = new_cfg.save() {
                        let msg: Vec<u16> = format!("Failed to save config: {e}\0").encode_utf16().collect();
                        MessageBoxW(Some(hwnd), PCWSTR(msg.as_ptr()), w!("Error"), MB_ICONERROR);
                        return LRESULT(0);
                    }

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
                    if state.recording_target.is_some() {
                        state.recording_target = None;
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
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
