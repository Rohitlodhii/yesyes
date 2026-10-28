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
const WM_DPICHANGED: u32 = 0x02E0;

pub const WM_SNAPDOCK_SETTINGS_SAVED: u32 = WM_APP + 5;

// Base unscaled layout dimensions (at 100% DPI, 96 DPI)
const BASE_WIN_W: i32 = 540;
const BASE_WIN_H: i32 = 592;

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

// Refined Palette (Win32 BGR COLORREF)
const COLOR_WINDOW_BG: COLORREF = COLORREF(0x001B1818);    // #18181B (Zinc 900)
const COLOR_SECTION_BG: COLORREF = COLORREF(0x00242020);   // #202024 (Card surface)
const COLOR_INPUT_BG: COLORREF = COLORREF(0x00191616);     // #161619 (Input surface)
const COLOR_BORDER: COLORREF = COLORREF(0x003F3838);       // #38383F (Restrained border)
const COLOR_BORDER_SUBTLE: COLORREF = COLORREF(0x00332D2D);// #2D2D33

const COLOR_TEXT_PRIMARY: COLORREF = COLORREF(0x00F6F4F3); // #F3F4F6 (Crisp light)
const COLOR_TEXT_SECONDARY: COLORREF = COLORREF(0x00AAA1A1);// #A1A1AA (Muted caption)
const COLOR_TEXT_MUTED: COLORREF = COLORREF(0x007A7171);   // #71717A (Subdued helper)

const COLOR_ACCENT_BLUE: COLORREF = COLORREF(0x00F67834);  // #3478F6 (Primary action)
const COLOR_ACCENT_HOVER: COLORREF = COLORREF(0x00F88747); // #4787F8
const COLOR_ACCENT_PILL_BG: COLORREF = COLORREF(0x0050301E);// #1E3050 (Subtle dark navy)

const COLOR_BTN_BG: COLORREF = COLORREF(0x003C3434);       // #34343C (Neutral button)
const COLOR_BTN_HOVER: COLORREF = COLORREF(0x004B4141);    // #41414B
const COLOR_AMBER_BG: COLORREF = COLORREF(0x000677D9);     // #D97706 (Recording)
const COLOR_AMBER_BORDER: COLORREF = COLORREF(0x000B9EF5); // #F59E0B

#[derive(Clone, Copy)]
struct LayoutRects {
    card_shortcut: RECT,
    rect_change_region: RECT,
    rect_change_fullscreen: RECT,
    rect_change_history: RECT,
    rect_badge_region: RECT,
    rect_badge_fullscreen: RECT,
    rect_badge_history: RECT,

    card_thumbnail: RECT,
    rect_lifespan_inf: RECT,
    rect_lifespan_timed: RECT,
    rect_lifespan_dis: RECT,
    rect_lifespan_dec: RECT,
    rect_lifespan_dur: RECT,
    rect_lifespan_inc: RECT,

    card_location: RECT,
    rect_path: RECT,
    rect_browse: RECT,
    rect_open_folder: RECT,

    rect_save_apply: RECT,
    rect_cancel: RECT,

    // Coordinates for text placement
    t1_y: i32,
    sub1_y: i32,
    in_l: i32,
    badge_l: i32,
    r1_y: i32,
    r2_y: i32,
    r3_y: i32,
    note1_y: i32,

    t2_y: i32,
    sub2_y: i32,

    t3_y: i32,
    sub3_y: i32,
}

fn compute_layout(cr: &RECT, s: f32) -> LayoutRects {
    let scale = |v: i32| -> i32 { (v as f32 * s).round() as i32 };

    let m_x = scale(20);
    let card_l = m_x;
    let card_r = cr.right - m_x;

    let in_pad = scale(16);
    let in_l = card_l + in_pad;
    let in_r = card_r - in_pad;

    // --- CARD 1: KEYBOARD SHORTCUTS ---
    let c1_top = scale(18);
    let t1_y = c1_top + scale(16);
    let sub1_y = t1_y + scale(22);

    let row_h = scale(32);
    let row_gap = scale(8);
    let r1_y = sub1_y + scale(24);
    let r2_y = r1_y + row_h + row_gap;
    let r3_y = r2_y + row_h + row_gap;

    let btn_w = scale(92);
    let btn_l = in_r - btn_w;
    let btn_r = in_r;

    let badge_l = in_l + scale(134);
    let badge_r = btn_l - scale(10);

    let note1_y = r3_y + row_h + scale(10);
    let c1_bot = note1_y + scale(22);

    // --- CARD 2: FLOATING THUMBNAIL ---
    let c2_top = c1_bot + scale(12);
    let t2_y = c2_top + scale(16);
    let sub2_y = t2_y + scale(22);

    let opt_y = sub2_y + scale(22);
    let opt_h = scale(32);

    let pill_inf_w = scale(142);
    let pill_timed_w = scale(78);
    let pill_dis_w = scale(78);
    let pill_gap = scale(8);

    let p1_l = in_l;
    let p1_r = p1_l + pill_inf_w;
    let p2_l = p1_r + pill_gap;
    let p2_r = p2_l + pill_timed_w;
    let p3_l = p2_r + pill_gap;
    let p3_r = p3_l + pill_dis_w;

    // Stepper controls
    let step_dec_l = p3_r + scale(14);
    let step_dec_r = step_dec_l + scale(28);
    let step_dur_l = step_dec_r + scale(4);
    let step_dur_r = step_dur_l + scale(54);
    let step_inc_l = step_dur_r + scale(4);
    let step_inc_r = step_inc_l + scale(28);

    let c2_bot = opt_y + opt_h + scale(16);

    // --- CARD 3: SAVE LOCATION ---
    let c3_top = c2_bot + scale(12);
    let t3_y = c3_top + scale(16);
    let sub3_y = t3_y + scale(22);

    let path_y = sub3_y + scale(22);
    let path_h = scale(32);

    let sub_btn_y = path_y + path_h + scale(10);
    let sub_btn_h = scale(30);
    let browse_w = scale(90);
    let open_w = scale(106);

    let c3_bot = sub_btn_y + sub_btn_h + scale(16);

    // --- FOOTER ACTION BAR ---
    let foot_gap = scale(16);
    let foot_y = c3_bot + foot_gap;
    let foot_h = scale(32);

    let cancel_w = scale(80);
    let save_w = scale(118);
    let cancel_r = card_r;
    let cancel_l = cancel_r - cancel_w;
    let save_r = cancel_l - scale(10);
    let save_l = save_r - save_w;

    LayoutRects {
        card_shortcut: RECT { left: card_l, top: c1_top, right: card_r, bottom: c1_bot },
        rect_change_region: RECT { left: btn_l, top: r1_y, right: btn_r, bottom: r1_y + row_h },
        rect_change_fullscreen: RECT { left: btn_l, top: r2_y, right: btn_r, bottom: r2_y + row_h },
        rect_change_history: RECT { left: btn_l, top: r3_y, right: btn_r, bottom: r3_y + row_h },
        rect_badge_region: RECT { left: badge_l, top: r1_y, right: badge_r, bottom: r1_y + row_h },
        rect_badge_fullscreen: RECT { left: badge_l, top: r2_y, right: badge_r, bottom: r2_y + row_h },
        rect_badge_history: RECT { left: badge_l, top: r3_y, right: badge_r, bottom: r3_y + row_h },

        card_thumbnail: RECT { left: card_l, top: c2_top, right: card_r, bottom: c2_bot },
        rect_lifespan_inf: RECT { left: p1_l, top: opt_y, right: p1_r, bottom: opt_y + opt_h },
        rect_lifespan_timed: RECT { left: p2_l, top: opt_y, right: p2_r, bottom: opt_y + opt_h },
        rect_lifespan_dis: RECT { left: p3_l, top: opt_y, right: p3_r, bottom: opt_y + opt_h },
        rect_lifespan_dec: RECT { left: step_dec_l, top: opt_y, right: step_dec_r, bottom: opt_y + opt_h },
        rect_lifespan_dur: RECT { left: step_dur_l, top: opt_y, right: step_dur_r, bottom: opt_y + opt_h },
        rect_lifespan_inc: RECT { left: step_inc_l, top: opt_y, right: step_inc_r, bottom: opt_y + opt_h },

        card_location: RECT { left: card_l, top: c3_top, right: card_r, bottom: c3_bot },
        rect_path: RECT { left: in_l, top: path_y, right: in_r, bottom: path_y + path_h },
        rect_browse: RECT { left: in_l, top: sub_btn_y, right: in_l + browse_w, bottom: sub_btn_y + sub_btn_h },
        rect_open_folder: RECT { left: in_l + browse_w + scale(10), top: sub_btn_y, right: in_l + browse_w + scale(10) + open_w, bottom: sub_btn_y + sub_btn_h },

        rect_save_apply: RECT { left: save_l, top: foot_y, right: save_r, bottom: foot_y + foot_h },
        rect_cancel: RECT { left: cancel_l, top: foot_y, right: cancel_r, bottom: foot_y + foot_h },

        t1_y,
        sub1_y,
        in_l,
        badge_l,
        r1_y,
        r2_y,
        r3_y,
        note1_y,

        t2_y,
        sub2_y,

        t3_y,
        sub3_y,
    }
}

fn hit_test(pt: POINT, layout: &LayoutRects, is_timed: bool) -> Option<ButtonId> {
    unsafe {
        if PtInRect(&layout.rect_change_region, pt).as_bool() {
            return Some(ButtonId::ChangeRegionHotkey);
        }
        if PtInRect(&layout.rect_change_fullscreen, pt).as_bool() {
            return Some(ButtonId::ChangeFullscreenHotkey);
        }
        if PtInRect(&layout.rect_change_history, pt).as_bool() {
            return Some(ButtonId::ChangeHistoryHotkey);
        }
        if PtInRect(&layout.rect_lifespan_inf, pt).as_bool() {
            return Some(ButtonId::LifespanInfinite);
        }
        if PtInRect(&layout.rect_lifespan_timed, pt).as_bool() {
            return Some(ButtonId::LifespanTimed);
        }
        if PtInRect(&layout.rect_lifespan_dis, pt).as_bool() {
            return Some(ButtonId::LifespanDisabled);
        }
        if is_timed {
            if PtInRect(&layout.rect_lifespan_dec, pt).as_bool() {
                return Some(ButtonId::LifespanDec);
            }
            if PtInRect(&layout.rect_lifespan_inc, pt).as_bool() {
                return Some(ButtonId::LifespanInc);
            }
        }
        if PtInRect(&layout.rect_browse, pt).as_bool() {
            return Some(ButtonId::BrowseFolder);
        }
        if PtInRect(&layout.rect_open_folder, pt).as_bool() {
            return Some(ButtonId::OpenFolder);
        }
        if PtInRect(&layout.rect_save_apply, pt).as_bool() {
            return Some(ButtonId::SaveApply);
        }
        if PtInRect(&layout.rect_cancel, pt).as_bool() {
            return Some(ButtonId::Cancel);
        }
    }
    None
}

struct SettingsState {
    main_hwnd: HWND,
    dpi_scale: f32,

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

    font_heading: HFONT,
    font_body_semibold: HFONT,
    font_body: HFONT,
    font_caption: HFONT,
}

impl SettingsState {
    fn new(main_hwnd: HWND, cfg: &AppConfig, dpi_scale: f32) -> Self {
        let lifespan_mode = match cfg.thumbnail_lifespan_secs {
            None => LifespanMode::Infinite,
            Some(0) => LifespanMode::Disabled,
            Some(n) => LifespanMode::Timed(n),
        };

        unsafe {
            let font_heading = create_font_px(14, 600, dpi_scale);
            let font_body_semibold = create_font_px(12, 600, dpi_scale);
            let font_body = create_font_px(12, 400, dpi_scale);
            let font_caption = create_font_px(11, 400, dpi_scale);

            Self {
                main_hwnd,
                dpi_scale,
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
                font_heading,
                font_body_semibold,
                font_body,
                font_caption,
            }
        }
    }

    fn reload_fonts(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.font_heading.0));
            let _ = DeleteObject(HGDIOBJ(self.font_body_semibold.0));
            let _ = DeleteObject(HGDIOBJ(self.font_body.0));
            let _ = DeleteObject(HGDIOBJ(self.font_caption.0));

            self.font_heading = create_font_px(14, 600, self.dpi_scale);
            self.font_body_semibold = create_font_px(12, 600, self.dpi_scale);
            self.font_body = create_font_px(12, 400, self.dpi_scale);
            self.font_caption = create_font_px(11, 400, self.dpi_scale);
        }
    }
}

impl Drop for SettingsState {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.font_heading.0));
            let _ = DeleteObject(HGDIOBJ(self.font_body_semibold.0));
            let _ = DeleteObject(HGDIOBJ(self.font_body.0));
            let _ = DeleteObject(HGDIOBJ(self.font_caption.0));
        }
    }
}

unsafe fn create_font_px(size_px: i32, weight: i32, scale: f32) -> HFONT {
    let font_name: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
    let scaled_height = (size_px as f32 * scale).round() as i32;

    CreateFontW(
        -scaled_height,
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

unsafe fn draw_rounded_box(hdc: HDC, rect: &RECT, bg: COLORREF, border: COLORREF, corner_radius: i32) {
    let brush = CreateSolidBrush(bg);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));

    let _ = RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, corner_radius, corner_radius);

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
    corner_radius: i32,
) {
    draw_rounded_box(hdc, rect, bg, border, corner_radius);

    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, text_color);

    let mut text_w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let mut r = *rect;
    DrawTextW(hdc, &mut text_w, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

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
    let bg_brush = CreateSolidBrush(COLOR_WINDOW_BG);
    FillRect(mem_dc, &cr, bg_brush);
    let _ = DeleteObject(HGDIOBJ(bg_brush.0));

    let _ = SetBkMode(mem_dc, TRANSPARENT);

    let l = compute_layout(&cr, state.dpi_scale);
    let is_timed = matches!(state.pending_lifespan, LifespanMode::Timed(_));

    // --- CARD 1: KEYBOARD SHORTCUTS ---
    draw_rounded_box(mem_dc, &l.card_shortcut, COLOR_SECTION_BG, COLOR_BORDER, 8);

    // Section title & description
    let old_font = SelectObject(mem_dc, HGDIOBJ(state.font_heading.0));
    SetTextColor(mem_dc, COLOR_TEXT_PRIMARY);
    let mut t1: Vec<u16> = "Keyboard Shortcuts\0".encode_utf16().collect();
    let mut r_t1 = RECT { left: l.in_l, top: l.t1_y, right: l.card_shortcut.right - 20, bottom: l.t1_y + 20 };
    DrawTextW(mem_dc, &mut t1, &mut r_t1, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);

    SelectObject(mem_dc, HGDIOBJ(state.font_caption.0));
    SetTextColor(mem_dc, COLOR_TEXT_SECONDARY);
    let mut sub1: Vec<u16> = "Global hotkeys to trigger capture actions and history\0".encode_utf16().collect();
    let mut r_sub1 = RECT { left: l.in_l, top: l.sub1_y, right: l.card_shortcut.right - 20, bottom: l.sub1_y + 18 };
    DrawTextW(mem_dc, &mut sub1, &mut r_sub1, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);

    let row_h = l.rect_change_region.bottom - l.rect_change_region.top;

    let draw_shortcut_row = |mem_dc: HDC,
                             lbl_text: &str,
                             lbl_y: i32,
                             badge_rect: &RECT,
                             btn_rect: &RECT,
                             hk_str: &str,
                             is_recording: bool,
                             btn_hovered: bool| {
        // Label
        SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
        SetTextColor(mem_dc, COLOR_TEXT_PRIMARY);
        let mut lw: Vec<u16> = format!("{lbl_text}\0").encode_utf16().collect();
        let mut r_lbl = RECT { left: l.in_l, top: lbl_y, right: l.badge_l - 10, bottom: lbl_y + row_h };
        DrawTextW(mem_dc, &mut lw, &mut r_lbl, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

        // Badge
        let badge_border = if is_recording { COLOR_AMBER_BORDER } else { COLOR_BORDER };
        let badge_bg = if is_recording { COLOR_AMBER_BG } else { COLOR_INPUT_BG };
        draw_rounded_box(mem_dc, badge_rect, badge_bg, badge_border, 6);

        SelectObject(mem_dc, HGDIOBJ(state.font_body_semibold.0));
        SetTextColor(mem_dc, COLOR_TEXT_PRIMARY);
        let mut bw: Vec<u16> = format!("{hk_str}\0").encode_utf16().collect();
        let mut r_b = *badge_rect;
        DrawTextW(mem_dc, &mut bw, &mut r_b, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

        // Button
        let (btn_txt, btn_bg, btn_border) = if is_recording {
            ("Recording...", COLOR_AMBER_BG, COLOR_AMBER_BORDER)
        } else if btn_hovered {
            ("Change", COLOR_BTN_HOVER, COLOR_BORDER)
        } else {
            ("Change", COLOR_BTN_BG, COLOR_BORDER)
        };
        draw_button(mem_dc, btn_rect, btn_txt, state.font_body_semibold, btn_bg, COLOR_TEXT_PRIMARY, btn_border, 6);
    };

    // Row 1: Region Capture
    let is_rec_reg = state.recording_target == Some(RecordingTarget::Region);
    let reg_hk_str = format_hotkey(state.pending_region_mods, state.pending_region_vk);
    draw_shortcut_row(
        mem_dc,
        "Region Capture",
        l.r1_y,
        &l.rect_badge_region,
        &l.rect_change_region,
        &reg_hk_str,
        is_rec_reg,
        state.hovered_button == Some(ButtonId::ChangeRegionHotkey),
    );

    // Row 2: Full Screen
    let is_rec_fs = state.recording_target == Some(RecordingTarget::Fullscreen);
    let fs_hk_str = format_hotkey(state.pending_fullscreen_mods, state.pending_fullscreen_vk);
    draw_shortcut_row(
        mem_dc,
        "Full Screen",
        l.r2_y,
        &l.rect_badge_fullscreen,
        &l.rect_change_fullscreen,
        &fs_hk_str,
        is_rec_fs,
        state.hovered_button == Some(ButtonId::ChangeFullscreenHotkey),
    );

    // Row 3: History Sidebar
    let is_rec_hist = state.recording_target == Some(RecordingTarget::History);
    let hist_hk_str = format_hotkey(state.pending_history_mods, state.pending_history_vk);
    draw_shortcut_row(
        mem_dc,
        "History Sidebar",
        l.r3_y,
        &l.rect_badge_history,
        &l.rect_change_history,
        &hist_hk_str,
        is_rec_hist,
        state.hovered_button == Some(ButtonId::ChangeHistoryHotkey),
    );

    // Card 1 Footnote
    SelectObject(mem_dc, HGDIOBJ(state.font_caption.0));
    SetTextColor(mem_dc, COLOR_TEXT_MUTED);
    let mut note1: Vec<u16> = "Press any key combination while recording. Esc cancels.\0".encode_utf16().collect();
    let mut r_note1 = RECT { left: l.in_l, top: l.note1_y, right: l.card_shortcut.right - 20, bottom: l.note1_y + 18 };
    DrawTextW(mem_dc, &mut note1, &mut r_note1, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);

    // --- CARD 2: FLOATING THUMBNAIL ---
    draw_rounded_box(mem_dc, &l.card_thumbnail, COLOR_SECTION_BG, COLOR_BORDER, 8);

    SelectObject(mem_dc, HGDIOBJ(state.font_heading.0));
    SetTextColor(mem_dc, COLOR_TEXT_PRIMARY);
    let mut t2: Vec<u16> = "Floating Thumbnail\0".encode_utf16().collect();
    let mut r_t2 = RECT { left: l.in_l, top: l.t2_y, right: l.card_thumbnail.right - 20, bottom: l.t2_y + 20 };
    DrawTextW(mem_dc, &mut t2, &mut r_t2, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);

    SelectObject(mem_dc, HGDIOBJ(state.font_caption.0));
    SetTextColor(mem_dc, COLOR_TEXT_SECONDARY);
    let mut sub2: Vec<u16> = "Choose how long preview thumbnails stay on screen\0".encode_utf16().collect();
    let mut r_sub2 = RECT { left: l.in_l, top: l.sub2_y, right: l.card_thumbnail.right - 20, bottom: l.sub2_y + 18 };
    DrawTextW(mem_dc, &mut sub2, &mut r_sub2, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);

    let draw_pill = |mem_dc: HDC, rect: &RECT, text: &str, is_selected: bool, is_hover: bool| {
        let (bg, border, txt_color) = if is_selected {
            (COLOR_ACCENT_PILL_BG, COLOR_ACCENT_BLUE, COLOR_TEXT_PRIMARY)
        } else if is_hover {
            (COLOR_BTN_HOVER, COLOR_BORDER, COLOR_TEXT_PRIMARY)
        } else {
            (COLOR_BTN_BG, COLOR_BORDER_SUBTLE, COLOR_TEXT_SECONDARY)
        };
        draw_button(mem_dc, rect, text, state.font_body_semibold, bg, txt_color, border, 6);
    };

    draw_pill(mem_dc, &l.rect_lifespan_inf, "Keep until closed", state.pending_lifespan == LifespanMode::Infinite, state.hovered_button == Some(ButtonId::LifespanInfinite));
    draw_pill(mem_dc, &l.rect_lifespan_timed, "Timed", is_timed, state.hovered_button == Some(ButtonId::LifespanTimed));
    draw_pill(mem_dc, &l.rect_lifespan_dis, "Disable", state.pending_lifespan == LifespanMode::Disabled, state.hovered_button == Some(ButtonId::LifespanDisabled));

    // Stepper (active & visible when Timed is selected)
    if let LifespanMode::Timed(secs) = state.pending_lifespan {
        let dec_bg = if state.hovered_button == Some(ButtonId::LifespanDec) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
        draw_button(mem_dc, &l.rect_lifespan_dec, "-", state.font_heading, dec_bg, COLOR_TEXT_PRIMARY, COLOR_BORDER, 6);

        draw_rounded_box(mem_dc, &l.rect_lifespan_dur, COLOR_INPUT_BG, COLOR_BORDER, 6);
        SelectObject(mem_dc, HGDIOBJ(state.font_body_semibold.0));
        SetTextColor(mem_dc, COLOR_TEXT_PRIMARY);
        let mut dur_w: Vec<u16> = format!("{secs}s\0").encode_utf16().collect();
        let mut r_dur = l.rect_lifespan_dur;
        DrawTextW(mem_dc, &mut dur_w, &mut r_dur, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

        let inc_bg = if state.hovered_button == Some(ButtonId::LifespanInc) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
        draw_button(mem_dc, &l.rect_lifespan_inc, "+", state.font_heading, inc_bg, COLOR_TEXT_PRIMARY, COLOR_BORDER, 6);
    }

    // --- CARD 3: SAVE LOCATION ---
    draw_rounded_box(mem_dc, &l.card_location, COLOR_SECTION_BG, COLOR_BORDER, 8);

    SelectObject(mem_dc, HGDIOBJ(state.font_heading.0));
    SetTextColor(mem_dc, COLOR_TEXT_PRIMARY);
    let mut t3: Vec<u16> = "Save Location\0".encode_utf16().collect();
    let mut r_t3 = RECT { left: l.in_l, top: l.t3_y, right: l.card_location.right - 20, bottom: l.t3_y + 20 };
    DrawTextW(mem_dc, &mut t3, &mut r_t3, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);

    SelectObject(mem_dc, HGDIOBJ(state.font_caption.0));
    SetTextColor(mem_dc, COLOR_TEXT_SECONDARY);
    let mut sub3: Vec<u16> = "Folder where full-resolution screenshots are stored\0".encode_utf16().collect();
    let mut r_sub3 = RECT { left: l.in_l, top: l.sub3_y, right: l.card_location.right - 20, bottom: l.sub3_y + 18 };
    DrawTextW(mem_dc, &mut sub3, &mut r_sub3, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);

    // Path Box
    draw_rounded_box(mem_dc, &l.rect_path, COLOR_INPUT_BG, COLOR_BORDER, 6);
    SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
    SetTextColor(mem_dc, COLOR_TEXT_PRIMARY);
    let path_str = state.pending_save_dir.to_string_lossy();
    let mut path_w: Vec<u16> = format!("{path_str}\0").encode_utf16().collect();
    let mut r_path = RECT {
        left: l.rect_path.left + 10,
        top: l.rect_path.top,
        right: l.rect_path.right - 10,
        bottom: l.rect_path.bottom,
    };
    DrawTextW(mem_dc, &mut path_w, &mut r_path, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_PATH_ELLIPSIS | DT_NOPREFIX);

    // Action buttons
    let browse_bg = if state.hovered_button == Some(ButtonId::BrowseFolder) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
    draw_button(mem_dc, &l.rect_browse, "Browse...", state.font_body_semibold, browse_bg, COLOR_TEXT_PRIMARY, COLOR_BORDER, 6);

    let open_bg = if state.hovered_button == Some(ButtonId::OpenFolder) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
    draw_button(mem_dc, &l.rect_open_folder, "Open Folder", state.font_body_semibold, open_bg, COLOR_TEXT_PRIMARY, COLOR_BORDER, 6);

    // --- FOOTER BUTTONS ---
    let save_bg = if state.hovered_button == Some(ButtonId::SaveApply) { COLOR_ACCENT_HOVER } else { COLOR_ACCENT_BLUE };
    draw_button(mem_dc, &l.rect_save_apply, "Save & Apply", state.font_body_semibold, save_bg, COLOR_TEXT_PRIMARY, COLOR_ACCENT_BLUE, 6);

    let cancel_bg = if state.hovered_button == Some(ButtonId::Cancel) { COLOR_BTN_HOVER } else { COLOR_BTN_BG };
    draw_button(mem_dc, &l.rect_cancel, "Cancel", state.font_body_semibold, cancel_bg, COLOR_TEXT_SECONDARY, COLOR_BORDER, 6);

    SelectObject(mem_dc, old_font);

    // Blit from backbuffer to screen
    let _ = BitBlt(hdc, 0, 0, cr.right, cr.bottom, Some(mem_dc), 0, 0, SRCCOPY);

    SelectObject(mem_dc, old_bmp);
    let _ = DeleteObject(HGDIOBJ(mem_bmp.0));
    let _ = DeleteDC(mem_dc);

    let _ = EndPaint(hwnd, &ps);
}

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
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xffff) as u32;
            state.dpi_scale = new_dpi as f32 / 96.0;
            state.reload_fonts();

            let rect = *(lparam.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(hwnd, &mut pt);

            let mut cr = RECT::default();
            let _ = GetClientRect(hwnd, &mut cr);
            let layout = compute_layout(&cr, state.dpi_scale);
            let is_timed = matches!(state.pending_lifespan, LifespanMode::Timed(_));

            if hit_test(pt, &layout, is_timed).is_some() {
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

            let mut cr = RECT::default();
            let _ = GetClientRect(hwnd, &mut cr);
            let layout = compute_layout(&cr, state.dpi_scale);
            let is_timed = matches!(state.pending_lifespan, LifespanMode::Timed(_));

            let hit = hit_test(pt, &layout, is_timed);
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

            let mut cr = RECT::default();
            let _ = GetClientRect(hwnd, &mut cr);
            let layout = compute_layout(&cr, state.dpi_scale);
            let is_timed = matches!(state.pending_lifespan, LifespanMode::Timed(_));

            match hit_test(pt, &layout, is_timed) {
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

pub fn get_system_dpi_scale() -> f32 {
    unsafe {
        let dc = GetDC(None);
        let dpi = GetDeviceCaps(Some(dc), LOGPIXELSX);
        ReleaseDC(None, dc);
        if dpi > 0 {
            dpi as f32 / 96.0
        } else {
            1.0
        }
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

        let dpi_scale = get_system_dpi_scale();
        let client_w = (BASE_WIN_W as f32 * dpi_scale).round() as i32;
        let client_h = (BASE_WIN_H as f32 * dpi_scale).round() as i32;

        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut wr = RECT {
            left: 0,
            top: 0,
            right: client_w,
            bottom: client_h,
        };
        let _ = AdjustWindowRectEx(&mut wr, style, false, WINDOW_EX_STYLE::default());

        let total_w = wr.right - wr.left;
        let total_h = wr.bottom - wr.top;

        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let x = (screen_w - total_w) / 2;
        let y = (screen_h - total_h) / 2;

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Snapdock Settings"),
            style,
            x,
            y,
            total_w,
            total_h,
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

        let state = Box::new(SettingsState::new(main_hwnd, cfg, dpi_scale));
        let state_raw = Box::into_raw(state);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_raw as isize);

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);

        Some(hwnd)
    }
}
