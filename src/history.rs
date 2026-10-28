use std::collections::HashSet;
use std::path::{Path, PathBuf};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::theme::{self, icons, ThemeColors};

const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_ANIM_ID: usize = 99;
const WM_REQUEST_TOGGLE: u32 = WM_USER + 102;
const WM_DOCK_UPDATE: u32 = WM_USER + 103;

pub const DOCK_WIDTH: i32 = 156;
pub const DOCK_RIGHT_MARGIN: i32 = 16;
pub const DOCK_CORNER_RADIUS: i32 = 12;
pub const HEADER_HEIGHT: i32 = 44;
pub const CARD_HEIGHT: i32 = 114;
pub const CARD_GAP: i32 = 8;

#[derive(Clone)]
pub struct HistoryEntry {
    pub id: usize,
    pub file_path: PathBuf,
    #[allow(dead_code)]
    pub timestamp_str: String,
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
    pub thumb_hbitmap: HBITMAP,
    pub thumb_w: i32,
    pub thumb_h: i32,
}

pub struct HistoryManager {
    next_id: usize,
    pub entries: Vec<HistoryEntry>,
    pub sidebar_hwnd: Option<HWND>,
}

impl HistoryManager {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            entries: Vec::new(),
            sidebar_hwnd: None,
        }
    }

    pub fn push(
        &mut self,
        file_path: PathBuf,
        rgba: &[u8],
        orig_w: u32,
        orig_h: u32,
    ) {
        if orig_w == 0 || orig_h == 0 {
            return;
        }

        let st = unsafe { GetLocalTime() };
        let timestamp_str = format!(
            "{:02}:{:02}:{:02}",
            st.wHour, st.wMinute, st.wSecond
        );

        // Thumbnail: max 110 px wide, proportional height max 68 px to fit card upper section
        let thumb_w = 110.min(orig_w).max(40);
        let thumb_h = (((orig_h as f32 / orig_w as f32) * thumb_w as f32).round() as u32).clamp(24, 68);

        let downscaled = downscale_rgba(rgba, orig_w, orig_h, thumb_w, thumb_h);

        let hbitmap = create_thumb_hbitmap(&downscaled, thumb_w, thumb_h);
        if let Some(bmp) = hbitmap {
            let id = self.next_id;
            self.next_id += 1;

            let entry = HistoryEntry {
                id,
                file_path,
                timestamp_str,
                width: orig_w,
                height: orig_h,
                thumb_hbitmap: bmp,
                thumb_w: thumb_w as i32,
                thumb_h: thumb_h as i32,
            };

            self.entries.insert(0, entry);

            // Limit history to 50 items
            if self.entries.len() > 50 {
                if let Some(removed) = self.entries.pop() {
                    unsafe {
                        let _ = DeleteObject(HGDIOBJ(removed.thumb_hbitmap.0));
                    }
                }
            }

            if let Some(hwnd) = self.sidebar_hwnd {
                unsafe {
                    if IsWindow(Some(hwnd)).as_bool() {
                        let _ = PostMessageW(Some(hwnd), WM_DOCK_UPDATE, WPARAM(0), LPARAM(0));
                    }
                }
            }
        }
    }

    pub fn delete_by_id(&mut self, id: usize) {
        if let Some(pos) = self.entries.iter().position(|e| e.id == id) {
            let removed = self.entries.remove(pos);
            unsafe {
                let _ = DeleteObject(HGDIOBJ(removed.thumb_hbitmap.0));
            }
            if let Some(hwnd) = self.sidebar_hwnd {
                unsafe {
                    if IsWindow(Some(hwnd)).as_bool() {
                        let _ = PostMessageW(Some(hwnd), WM_DOCK_UPDATE, WPARAM(0), LPARAM(0));
                    }
                }
            }
        }
    }

    pub fn clear(&mut self) {
        for entry in self.entries.drain(..) {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(entry.thumb_hbitmap.0));
            }
        }
        if let Some(hwnd) = self.sidebar_hwnd {
            unsafe {
                if IsWindow(Some(hwnd)).as_bool() {
                    let _ = PostMessageW(Some(hwnd), WM_DOCK_UPDATE, WPARAM(0), LPARAM(0));
                }
            }
        }
    }

    pub fn toggle_sidebar(&mut self, main_hwnd: HWND) {
        unsafe {
            if let Some(hwnd) = self.sidebar_hwnd {
                if IsWindow(Some(hwnd)).as_bool() {
                    let _ = PostMessageW(Some(hwnd), WM_REQUEST_TOGGLE, WPARAM(0), LPARAM(0));
                    return;
                }
            }
            self.sidebar_hwnd = open_sidebar_window(main_hwnd, self as *mut HistoryManager);
        }
    }
}

impl Drop for HistoryManager {
    fn drop(&mut self) {
        for entry in self.entries.drain(..) {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(entry.thumb_hbitmap.0));
            }
        }
    }
}

fn create_thumb_hbitmap(rgba: &[u8], w: u32, h: u32) -> Option<HBITMAP> {
    unsafe {
        let screen_dc = GetDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: w * h * 4,
                ..Default::default()
            },
            bmiColors: [RGBQUAD::default(); 1],
        };

        let mut bits_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbitmap = CreateDIBSection(
            Some(screen_dc),
            &bmi,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        ).ok()?;
        ReleaseDC(None, screen_dc);

        let total = (w * h) as usize;
        let dst_slice = std::slice::from_raw_parts_mut(bits_ptr as *mut u8, total * 4);
        for (src_chunk, dst_chunk) in rgba.chunks_exact(4).zip(dst_slice.chunks_exact_mut(4)) {
            dst_chunk[0] = src_chunk[2]; // B
            dst_chunk[1] = src_chunk[1]; // G
            dst_chunk[2] = src_chunk[0]; // R
            dst_chunk[3] = 255;          // A
        }

        Some(hbitmap)
    }
}

fn downscale_rgba(src: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];
    let x_ratio = src_w as f32 / dst_w as f32;
    let y_ratio = src_h as f32 / dst_h as f32;

    for y in 0..dst_h {
        let src_y = ((y as f32 + 0.5) * y_ratio) as u32;
        let src_y = src_y.min(src_h - 1);
        for x in 0..dst_w {
            let src_x = ((x as f32 + 0.5) * x_ratio) as u32;
            let src_x = src_x.min(src_w - 1);

            let src_idx = ((src_y * src_w + src_x) * 4) as usize;
            let dst_idx = ((y * dst_w + x) * 4) as usize;

            dst[dst_idx..dst_idx + 4].copy_from_slice(&src[src_idx..src_idx + 4]);
        }
    }
    dst
}

pub fn calculate_dock_dimensions(count: usize, work_h: i32) -> (i32, i32) {
    let content_h = if count == 0 {
        HEADER_HEIGHT + 70 + 8
    } else {
        HEADER_HEIGHT + (count as i32 * CARD_HEIGHT) + ((count as i32 - 1).max(0) * CARD_GAP) + 10
    };
    let max_h = (work_h - 48).max(180);
    let dock_h = content_h.min(max_h);
    (DOCK_WIDTH, dock_h)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarHit {
    SelectAllBtn,
    ClearBtn,
    CloseBtn,
    Card(usize),
    CardOpen(usize),
    CardCopy(usize),
    CardPath(usize),
    CardDelete(usize),
}

struct SidebarAnim {
    is_closing: bool,
    start_time: std::time::Instant,
    duration: std::time::Duration,
    start_x: i32,
    target_x: i32,
    y: i32,
    w: i32,
    h: i32,
}

struct SidebarState {
    _main_hwnd: HWND,
    history_ptr: *mut HistoryManager,
    scroll_y: i32,
    hovered: Option<SidebarHit>,
    selected_ids: HashSet<usize>,
    drag_start: Option<(i32, i32, usize)>, // x, y, card index
    anim: Option<SidebarAnim>,
    pub theme: ThemeColors,

    font_title: HFONT,
    font_bold: HFONT,
    font_body: HFONT,
    font_small: HFONT,
    font_icon: HFONT,
    font_icon_btn: HFONT,
}

impl SidebarState {
    fn new(main_hwnd: HWND, history_ptr: *mut HistoryManager) -> Self {
        unsafe {
            Self {
                _main_hwnd: main_hwnd,
                history_ptr,
                scroll_y: 0,
                hovered: None,
                selected_ids: HashSet::new(),
                drag_start: None,
                anim: None,
                theme: ThemeColors::current(),
                font_title: create_font(15, 700),
                font_bold: create_font(13, 600),
                font_body: create_font(12, 400),
                font_small: create_font(11, 400),
                font_icon: theme::create_icon_font(14),
                font_icon_btn: theme::create_icon_font(12),
            }
        }
    }
}

unsafe fn get_monitor_work_rect(hwnd: HWND) -> RECT {
    let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let _ = GetMonitorInfoW(hmon, &mut mi);
    mi.rcWork
}

unsafe fn relayout_dock(hwnd: HWND, state: &mut SidebarState) {
    let history = &*state.history_ptr;
    let count = history.entries.len();
    let work = get_monitor_work_rect(hwnd);
    let work_h = work.bottom - work.top;

    let (dock_w, dock_h) = calculate_dock_dimensions(count, work_h);
    let target_x = work.right - DOCK_RIGHT_MARGIN - dock_w;
    let y = work.top + (work_h - dock_h) / 2;

    // Clamp scroll
    let total_content_h = count as i32 * (CARD_HEIGHT + CARD_GAP);
    let visible_h = dock_h - HEADER_HEIGHT;
    let max_scroll = (total_content_h - visible_h).max(0);
    state.scroll_y = state.scroll_y.clamp(0, max_scroll);

    // Apply rounded window region
    let rgn = CreateRoundRectRgn(
        0,
        0,
        dock_w + 1,
        dock_h + 1,
        DOCK_CORNER_RADIUS * 2,
        DOCK_CORNER_RADIUS * 2,
    );
    let _ = SetWindowRgn(hwnd, Some(rgn), true);

    let _ = SetWindowPos(
        hwnd,
        None,
        target_x,
        y,
        dock_w,
        dock_h,
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    let _ = InvalidateRect(Some(hwnd), None, true);
}

unsafe fn start_close_animation(hwnd: HWND, state: &mut SidebarState) {
    if state.anim.as_ref().map_or(false, |a| a.is_closing) {
        return;
    }
    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    let work = get_monitor_work_rect(hwnd);
    let cur_x = rect.left;
    let target_x = work.right + 20;

    state.anim = Some(SidebarAnim {
        is_closing: true,
        start_time: std::time::Instant::now(),
        duration: std::time::Duration::from_millis(140),
        start_x: cur_x,
        target_x,
        y: rect.top,
        w: rect.right - rect.left,
        h: rect.bottom - rect.top,
    });
    let _ = SetTimer(Some(hwnd), TIMER_ANIM_ID, 16, None);
}

impl Drop for SidebarState {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.font_title.0));
            let _ = DeleteObject(HGDIOBJ(self.font_bold.0));
            let _ = DeleteObject(HGDIOBJ(self.font_body.0));
            let _ = DeleteObject(HGDIOBJ(self.font_small.0));
            let _ = DeleteObject(HGDIOBJ(self.font_icon.0));
            let _ = DeleteObject(HGDIOBJ(self.font_icon_btn.0));
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

fn hit_test(pt: POINT, state: &SidebarState, count: usize) -> Option<SidebarHit> {
    // Header controls: Select All, Clear, Close
    let select_all_rect = RECT { left: 8, top: 9, right: 86, bottom: 35 };
    let clear_rect = RECT { left: 92, top: 9, right: 118, bottom: 35 };
    let close_rect = RECT { left: 122, top: 9, right: 148, bottom: 35 };

    unsafe {
        if PtInRect(&select_all_rect, pt).as_bool() {
            return Some(SidebarHit::SelectAllBtn);
        }
        if PtInRect(&clear_rect, pt).as_bool() {
            return Some(SidebarHit::ClearBtn);
        }
        if PtInRect(&close_rect, pt).as_bool() {
            return Some(SidebarHit::CloseBtn);
        }
    }

    if pt.y < HEADER_HEIGHT {
        return None;
    }

    let y_in_cards = pt.y - HEADER_HEIGHT + state.scroll_y;
    let card_step = CARD_HEIGHT + CARD_GAP;
    let idx = (y_in_cards / card_step) as usize;

    if idx < count {
        let card_top = HEADER_HEIGHT + (idx as i32 * card_step) - state.scroll_y;
        let card_bottom = card_top + CARD_HEIGHT;

        if pt.y >= card_top && pt.y <= card_bottom && pt.x >= 8 && pt.x <= DOCK_WIDTH - 8 {
            // Check button hits within card: y in [card_top + 82, card_top + 106]
            let btn_top = card_top + 82;
            let btn_bottom = card_top + 106;

            if pt.y >= btn_top && pt.y <= btn_bottom {
                // Button 1: Open (x: 20..46)
                if pt.x >= 20 && pt.x <= 46 {
                    return Some(SidebarHit::CardOpen(idx));
                }
                // Button 2: Copy (x: 50..76)
                if pt.x >= 50 && pt.x <= 76 {
                    return Some(SidebarHit::CardCopy(idx));
                }
                // Button 3: Path (x: 80..106)
                if pt.x >= 80 && pt.x <= 106 {
                    return Some(SidebarHit::CardPath(idx));
                }
                // Button 4: Delete (x: 110..136)
                if pt.x >= 110 && pt.x <= 136 {
                    return Some(SidebarHit::CardDelete(idx));
                }
            }

            return Some(SidebarHit::Card(idx));
        }
    }

    None
}

unsafe fn draw_rounded_card(hdc: HDC, rect: &RECT, bg: COLORREF, border: COLORREF, radius: i32) {
    let brush = CreateSolidBrush(bg);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));

    let _ = RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, radius * 2, radius * 2);

    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

unsafe fn draw_icon_btn(
    hdc: HDC,
    rect: &RECT,
    icon: &str,
    font: HFONT,
    bg: COLORREF,
    icon_color: COLORREF,
    border: COLORREF,
) {
    draw_rounded_card(hdc, rect, bg, border, 4);

    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, icon_color);

    let mut icon_w: Vec<u16> = icon.encode_utf16().chain(std::iter::once(0)).collect();
    let mut r = *rect;
    DrawTextW(hdc, &mut icon_w, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    SelectObject(hdc, old_font);
}

unsafe fn draw_icon_and_text_btn(
    hdc: HDC,
    rect: &RECT,
    icon: &str,
    text: &str,
    font_icon: HFONT,
    font_text: HFONT,
    bg: COLORREF,
    color: COLORREF,
    border: COLORREF,
) {
    draw_rounded_card(hdc, rect, bg, border, 5);

    let _ = SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, color);

    let old_font = SelectObject(hdc, HGDIOBJ(font_icon.0));
    let mut icon_w: Vec<u16> = icon.encode_utf16().chain(std::iter::once(0)).collect();
    let mut icon_size = SIZE::default();
    let _ = GetTextExtentPoint32W(hdc, &icon_w[..icon_w.len() - 1], &mut icon_size);

    SelectObject(hdc, HGDIOBJ(font_text.0));
    let mut text_w: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let mut text_size = SIZE::default();
    let _ = GetTextExtentPoint32W(hdc, &text_w[..text_w.len() - 1], &mut text_size);

    let gap = 4;
    let total_w = icon_size.cx + gap + text_size.cx;
    let start_x = rect.left + ((rect.right - rect.left) - total_w).max(0) / 2;

    SelectObject(hdc, HGDIOBJ(font_icon.0));
    let mut r_icon = RECT {
        left: start_x,
        top: rect.top,
        right: start_x + icon_size.cx,
        bottom: rect.bottom,
    };
    DrawTextW(hdc, &mut icon_w, &mut r_icon, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    SelectObject(hdc, HGDIOBJ(font_text.0));
    let text_x = start_x + icon_size.cx + gap;
    let mut r_text = RECT {
        left: text_x,
        top: rect.top,
        right: text_x + text_size.cx,
        bottom: rect.bottom,
    };
    DrawTextW(hdc, &mut text_w, &mut r_text, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    SelectObject(hdc, old_font);
}

unsafe fn paint_sidebar(hwnd: HWND, state: &SidebarState) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let mut cr = RECT::default();
    let _ = GetClientRect(hwnd, &mut cr);

    let mem_dc = CreateCompatibleDC(Some(hdc));
    let mem_bmp = CreateCompatibleBitmap(hdc, cr.right, cr.bottom);
    let old_bmp = SelectObject(mem_dc, HGDIOBJ(mem_bmp.0));

    // Fill background
    let bg_brush = CreateSolidBrush(state.theme.window_bg);
    FillRect(mem_dc, &cr, bg_brush);
    let _ = DeleteObject(HGDIOBJ(bg_brush.0));

    let history = &*state.history_ptr;
    let count = history.entries.len();

    // --- CARDS LIST (Clipped to below header) ---
    let card_area = RECT { left: 0, top: HEADER_HEIGHT, right: cr.right, bottom: cr.bottom };
    let card_rgn = CreateRectRgn(card_area.left, card_area.top, card_area.right, card_area.bottom);
    SelectClipRgn(mem_dc, Some(card_rgn));

    let card_step = CARD_HEIGHT + CARD_GAP;
    for (idx, entry) in history.entries.iter().enumerate() {
        let card_top = HEADER_HEIGHT + (idx as i32 * card_step) - state.scroll_y;
        let card_bottom = card_top + CARD_HEIGHT;

        if card_bottom < HEADER_HEIGHT || card_top > cr.bottom {
            continue; // off-screen
        }

        let is_selected = state.selected_ids.contains(&entry.id);
        let is_hovered = matches!(
            state.hovered,
            Some(SidebarHit::Card(i))
                | Some(SidebarHit::CardCopy(i))
                | Some(SidebarHit::CardOpen(i))
                | Some(SidebarHit::CardPath(i))
                | Some(SidebarHit::CardDelete(i))
            if i == idx
        );

        let border_color = if is_selected {
            state.theme.accent
        } else if is_hovered {
            state.theme.card_hover
        } else {
            state.theme.card_border
        };

        let card_rect = RECT {
            left: 8,
            top: card_top,
            right: cr.right - 8,
            bottom: card_bottom,
        };
        draw_rounded_card(mem_dc, &card_rect, state.theme.card_bg, border_color, 8);

        // Selection badge in top-left corner
        if is_selected {
            let badge_rect = RECT {
                left: 12,
                top: card_top + 6,
                right: 28,
                bottom: card_top + 22,
            };
            draw_rounded_card(mem_dc, &badge_rect, state.theme.accent, state.theme.accent, 4);

            let old_font = SelectObject(mem_dc, HGDIOBJ(state.font_icon_btn.0));
            let _ = SetBkMode(mem_dc, TRANSPARENT);
            SetTextColor(mem_dc, theme::rgb(255, 255, 255));
            let mut chk_w: Vec<u16> = icons::CHECKMARK.encode_utf16().chain(std::iter::once(0)).collect();
            let mut r_chk = badge_rect;
            DrawTextW(mem_dc, &mut chk_w, &mut r_chk, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
            SelectObject(mem_dc, old_font);
        }

        // Center thumbnail in upper card region
        let thumb_x = 8 + (140 - entry.thumb_w) / 2;
        let thumb_y = card_top + 6 + (72 - entry.thumb_h).max(0) / 2;

        let thumb_rgn = CreateRoundRectRgn(
            thumb_x,
            thumb_y,
            thumb_x + entry.thumb_w + 1,
            thumb_y + entry.thumb_h + 1,
            6,
            6,
        );
        SelectClipRgn(mem_dc, Some(thumb_rgn));

        let thumb_dc = CreateCompatibleDC(Some(mem_dc));
        let old_thumb_bmp = SelectObject(thumb_dc, HGDIOBJ(entry.thumb_hbitmap.0));
        let _ = BitBlt(mem_dc, thumb_x, thumb_y, entry.thumb_w, entry.thumb_h, Some(thumb_dc), 0, 0, SRCCOPY);
        SelectObject(thumb_dc, old_thumb_bmp);
        let _ = DeleteDC(thumb_dc);

        SelectClipRgn(mem_dc, Some(card_rgn));
        let _ = DeleteObject(HGDIOBJ(thumb_rgn.0));

        // Thumbnail subtle border
        let t_pen = CreatePen(PS_SOLID, 1, state.theme.card_border);
        let old_pen = SelectObject(mem_dc, HGDIOBJ(t_pen.0));
        let old_brush = SelectObject(mem_dc, GetStockObject(HOLLOW_BRUSH));
        let _ = RoundRect(mem_dc, thumb_x, thumb_y, thumb_x + entry.thumb_w, thumb_y + entry.thumb_h, 6, 6);
        SelectObject(mem_dc, old_pen);
        SelectObject(mem_dc, old_brush);
        let _ = DeleteObject(HGDIOBJ(t_pen.0));

        // Action buttons row (Open, Copy, Path, Delete)
        let btn_top = card_top + 82;
        let btn_bottom = card_top + 106;

        let open_hover = state.hovered == Some(SidebarHit::CardOpen(idx));
        let copy_hover = state.hovered == Some(SidebarHit::CardCopy(idx));
        let path_hover = state.hovered == Some(SidebarHit::CardPath(idx));
        let del_hover = state.hovered == Some(SidebarHit::CardDelete(idx));

        // 1. Open Button
        let open_bg = if open_hover { state.theme.btn_hover } else { state.theme.btn_bg };
        let open_fg = if open_hover { state.theme.accent } else { state.theme.text_title };
        draw_icon_btn(mem_dc, &RECT { left: 20, top: btn_top, right: 46, bottom: btn_bottom }, icons::EXTERNAL, state.font_icon_btn, open_bg, open_fg, state.theme.card_border);

        // 2. Copy Image Button
        let copy_bg = if copy_hover { state.theme.btn_hover } else { state.theme.btn_bg };
        let copy_fg = if copy_hover { state.theme.accent } else { state.theme.text_title };
        draw_icon_btn(mem_dc, &RECT { left: 50, top: btn_top, right: 76, bottom: btn_bottom }, icons::COPY, state.font_icon_btn, copy_bg, copy_fg, state.theme.card_border);

        // 3. Copy Path Button
        let path_bg = if path_hover { state.theme.btn_hover } else { state.theme.btn_bg };
        let path_fg = if path_hover { state.theme.accent } else { state.theme.text_title };
        draw_icon_btn(mem_dc, &RECT { left: 80, top: btn_top, right: 106, bottom: btn_bottom }, icons::FOLDER, state.font_icon_btn, path_bg, path_fg, state.theme.card_border);

        // 4. Delete Button
        let del_bg = if del_hover { state.theme.close_red } else { state.theme.btn_bg };
        let del_fg = if del_hover { theme::rgb(255, 255, 255) } else { state.theme.text_muted };
        draw_icon_btn(mem_dc, &RECT { left: 110, top: btn_top, right: 136, bottom: btn_bottom }, icons::DELETE, state.font_icon_btn, del_bg, del_fg, state.theme.card_border);
    }

    SelectClipRgn(mem_dc, None);
    let _ = DeleteObject(HGDIOBJ(card_rgn.0));

    if count == 0 {
        let old_font = SelectObject(mem_dc, HGDIOBJ(state.font_small.0));
        let _ = SetBkMode(mem_dc, TRANSPARENT);
        SetTextColor(mem_dc, state.theme.text_muted);
        let mut empty_msg: Vec<u16> = "No captures\nTake screenshot\0".encode_utf16().collect();
        let mut r_empty = RECT { left: 8, top: HEADER_HEIGHT + 14, right: cr.right - 8, bottom: cr.bottom - 8 };
        DrawTextW(mem_dc, &mut empty_msg, &mut r_empty, DT_CENTER | DT_WORDBREAK | DT_NOPREFIX);
        SelectObject(mem_dc, old_font);
    }

    // --- HEADER (Fixed at top) ---
    let header_rect = RECT { left: 0, top: 0, right: cr.right, bottom: HEADER_HEIGHT };
    let h_brush = CreateSolidBrush(state.theme.window_bg);
    FillRect(mem_dc, &header_rect, h_brush);
    let _ = DeleteObject(HGDIOBJ(h_brush.0));

    // Bottom border of header
    let line_pen = CreatePen(PS_SOLID, 1, state.theme.card_border);
    let old_pen = SelectObject(mem_dc, HGDIOBJ(line_pen.0));
    let _ = MoveToEx(mem_dc, 8, HEADER_HEIGHT - 1, None);
    let _ = LineTo(mem_dc, cr.right - 8, HEADER_HEIGHT - 1);
    SelectObject(mem_dc, old_pen);
    let _ = DeleteObject(HGDIOBJ(line_pen.0));

    // 1. Select All Button
    let all_selected = count > 0 && state.selected_ids.len() == count;
    let some_selected = !state.selected_ids.is_empty();
    let sel_hover = state.hovered == Some(SidebarHit::SelectAllBtn);

    let (sel_bg, sel_border, sel_fg, sel_text) = if all_selected {
        (state.theme.accent, state.theme.accent, theme::rgb(255, 255, 255), "Deselect")
    } else if some_selected {
        (
            if sel_hover { state.theme.btn_hover } else { state.theme.btn_bg },
            state.theme.accent,
            state.theme.accent,
            "Select All",
        )
    } else {
        (
            if sel_hover { state.theme.btn_hover } else { state.theme.btn_bg },
            state.theme.card_border,
            state.theme.text_title,
            "Select All",
        )
    };

    let select_all_rect = RECT { left: 8, top: 9, right: 86, bottom: 35 };
    draw_icon_and_text_btn(
        mem_dc,
        &select_all_rect,
        icons::CHECKMARK,
        sel_text,
        state.font_icon_btn,
        state.font_small,
        sel_bg,
        sel_fg,
        sel_border,
    );

    // 2. Clear Button (Trash)
    let clear_hover = state.hovered == Some(SidebarHit::ClearBtn);
    let clear_rect = RECT { left: 92, top: 9, right: 118, bottom: 35 };
    let clear_bg = if clear_hover { state.theme.btn_hover } else { state.theme.btn_bg };
    let clear_fg = if clear_hover {
        if some_selected { state.theme.close_red } else { state.theme.accent }
    } else {
        state.theme.text_title
    };
    draw_icon_btn(mem_dc, &clear_rect, icons::DELETE, state.font_icon_btn, clear_bg, clear_fg, state.theme.card_border);

    // 3. Close Button (✕)
    let close_hover = state.hovered == Some(SidebarHit::CloseBtn);
    let close_rect = RECT { left: 122, top: 9, right: 148, bottom: 35 };
    let close_bg = if close_hover { state.theme.close_red } else { state.theme.btn_bg };
    let close_fg = if close_hover { theme::rgb(255, 255, 255) } else { state.theme.text_title };
    draw_icon_btn(mem_dc, &close_rect, icons::CLOSE, state.font_icon_btn, close_bg, close_fg, state.theme.card_border);

    // Outer Dock Perimeter Rounded Border
    let dock_border_pen = CreatePen(PS_SOLID, 1, state.theme.card_border);
    let old_pen = SelectObject(mem_dc, HGDIOBJ(dock_border_pen.0));
    let old_brush = SelectObject(mem_dc, GetStockObject(HOLLOW_BRUSH));
    let _ = RoundRect(mem_dc, 0, 0, cr.right, cr.bottom, DOCK_CORNER_RADIUS * 2, DOCK_CORNER_RADIUS * 2);
    SelectObject(mem_dc, old_pen);
    SelectObject(mem_dc, old_brush);
    let _ = DeleteObject(HGDIOBJ(dock_border_pen.0));

    // Blit from backbuffer to screen
    let _ = BitBlt(hdc, 0, 0, cr.right, cr.bottom, Some(mem_dc), 0, 0, SRCCOPY);

    SelectObject(mem_dc, old_bmp);
    let _ = DeleteObject(HGDIOBJ(mem_bmp.0));
    let _ = DeleteDC(mem_dc);

    let _ = EndPaint(hwnd, &ps);
}

unsafe extern "system" fn sidebar_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SidebarState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *state_ptr;
    let history = &mut *state.history_ptr;

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        theme::WM_SETTINGCHANGE | theme::WM_THEMECHANGED | theme::WM_DWMCOLORIZATIONCOLORCHANGED => {
            let new_theme = theme::ThemeColors::current();
            state.theme = new_theme;
            theme::apply_window_theme(hwnd, state.theme.is_dark);
            let _ = InvalidateRect(Some(hwnd), None, true);
            LRESULT(0)
        }
        WM_DOCK_UPDATE => {
            relayout_dock(hwnd, state);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_ANIM_ID {
                if let Some(anim) = &mut state.anim {
                    let elapsed = anim.start_time.elapsed().as_secs_f32() * 1000.0;
                    let total = anim.duration.as_secs_f32() * 1000.0;
                    let t = (elapsed / total).clamp(0.0, 1.0);

                    let ease = if anim.is_closing {
                        t * t * t
                    } else {
                        1.0 - (1.0 - t).powi(3)
                    };

                    let cur_x = anim.start_x + ((anim.target_x - anim.start_x) as f32 * ease).round() as i32;

                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        cur_x,
                        anim.y,
                        anim.w,
                        anim.h,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );

                    if t >= 1.0 {
                        let is_closing = anim.is_closing;
                        state.anim = None;
                        let _ = KillTimer(Some(hwnd), TIMER_ANIM_ID);
                        if is_closing {
                            let _ = ShowWindow(hwnd, SW_HIDE);
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            paint_sidebar(hwnd, state);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = ScreenToClient(hwnd, &mut pt);
            let count = history.entries.len();
            if hit_test(pt, state, count).is_some() {
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

            let count = history.entries.len();
            let hit = hit_test(pt, state, count);

            if hit != state.hovered {
                state.hovered = hit;
                let _ = InvalidateRect(Some(hwnd), None, false);

                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
            }

            // Drag detection: if left button held and moved > 4px from click
            if let Some((sx, sy, card_idx)) = state.drag_start {
                let dist = (x - sx).abs() + (y - sy).abs();
                if dist > 4 {
                    state.drag_start = None;
                    if let Some(entry) = history.entries.get(card_idx) {
                        let path = entry.file_path.clone();
                        let bmp = entry.thumb_hbitmap;
                        let w = entry.thumb_w;
                        let h = entry.thumb_h;
                        crate::dragout::execute_drag_drop(&path, bmp, w, h);
                    }
                }
            }

            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if state.hovered.is_some() {
                state.hovered = None;
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xffff) as i16 as i32;
            let count = history.entries.len();
            let total_content_h = count as i32 * (CARD_HEIGHT + CARD_GAP);

            let mut cr = RECT::default();
            let _ = GetClientRect(hwnd, &mut cr);
            let visible_h = cr.bottom - HEADER_HEIGHT;
            let max_scroll = (total_content_h - visible_h).max(0);

            // Negative delta = scroll down
            state.scroll_y = (state.scroll_y - (delta / 2)).clamp(0, max_scroll);
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_REQUEST_TOGGLE => {
            if let Some(anim) = &state.anim {
                if anim.is_closing {
                    let mut rect = RECT::default();
                    let _ = GetWindowRect(hwnd, &mut rect);
                    let work = get_monitor_work_rect(hwnd);
                    let work_h = work.bottom - work.top;
                    let (dock_w, dock_h) = calculate_dock_dimensions(history.entries.len(), work_h);
                    let target_x = work.right - DOCK_RIGHT_MARGIN - dock_w;
                    let y = work.top + (work_h - dock_h) / 2;
                    state.anim = Some(SidebarAnim {
                        is_closing: false,
                        start_time: std::time::Instant::now(),
                        duration: std::time::Duration::from_millis(160),
                        start_x: rect.left,
                        target_x,
                        y,
                        w: dock_w,
                        h: dock_h,
                    });
                    let _ = SetTimer(Some(hwnd), TIMER_ANIM_ID, 16, None);
                    return LRESULT(0);
                }
            }
            start_close_animation(hwnd, state);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if state.anim.as_ref().map_or(false, |a| a.is_closing) {
                return LRESULT(0);
            }

            let x = (lparam.0 & 0xffff) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            let pt = POINT { x, y };

            let count = history.entries.len();
            match hit_test(pt, state, count) {
                Some(SidebarHit::SelectAllBtn) => {
                    if !history.entries.is_empty() && state.selected_ids.len() == history.entries.len() {
                        state.selected_ids.clear();
                    } else {
                        state.selected_ids = history.entries.iter().map(|e| e.id).collect();
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                Some(SidebarHit::ClearBtn) => {
                    if !state.selected_ids.is_empty() {
                        let ids_to_delete: Vec<usize> = state.selected_ids.drain().collect();
                        for id in ids_to_delete {
                            history.delete_by_id(id);
                        }
                    } else {
                        history.clear();
                    }
                    state.scroll_y = 0;
                    relayout_dock(hwnd, state);
                }
                Some(SidebarHit::CloseBtn) => {
                    start_close_animation(hwnd, state);
                }
                Some(SidebarHit::CardOpen(idx)) => {
                    if let Some(entry) = history.entries.get(idx) {
                        let path_w: Vec<u16> = entry.file_path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
                        let _ = ShellExecuteW(Some(hwnd), w!("open"), PCWSTR(path_w.as_ptr()), None, None, SW_SHOWNORMAL);
                    }
                }
                Some(SidebarHit::CardCopy(idx)) => {
                    if let Some(entry) = history.entries.get(idx) {
                        copy_image_from_file(hwnd, &entry.file_path);
                    }
                }
                Some(SidebarHit::CardPath(idx)) => {
                    if let Some(entry) = history.entries.get(idx) {
                        copy_text_to_clipboard(hwnd, &entry.file_path.to_string_lossy());
                    }
                }
                Some(SidebarHit::CardDelete(idx)) => {
                    if let Some(entry) = history.entries.get(idx) {
                        let id = entry.id;
                        state.selected_ids.remove(&id);
                        history.delete_by_id(id);
                        relayout_dock(hwnd, state);
                    }
                }
                Some(SidebarHit::Card(idx)) => {
                    state.drag_start = Some((x, y, idx));
                }
                None => {}
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some((sx, sy, idx)) = state.drag_start.take() {
                let x = (lparam.0 & 0xffff) as i16 as i32;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
                let dist = (x - sx).abs() + (y - sy).abs();
                if dist <= 4 {
                    if let Some(entry) = history.entries.get(idx) {
                        let id = entry.id;
                        if state.selected_ids.contains(&id) {
                            state.selected_ids.remove(&id);
                        } else {
                            state.selected_ids.insert(id);
                        }
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u32;
            if vk == 0x1B { // Esc closes sidebar
                start_close_animation(hwnd, state);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            let _ = KillTimer(Some(hwnd), TIMER_ANIM_ID);
            history.sidebar_hwnd = None;
            let _ = Box::from_raw(state_ptr);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn copy_text_to_clipboard(hwnd: HWND, text: &str) {
    let _ = crate::clipboard::write_text(Some(hwnd), text);
}

fn copy_image_from_file(hwnd: HWND, path: &Path) {
    if let Ok(bytes) = std::fs::read(path) {
        if let Ok(decoder) = png::Decoder::new(std::io::Cursor::new(&bytes)).read_info() {
            let mut reader = decoder;
            let buf_size = reader.output_buffer_size().unwrap_or(3840 * 2160 * 4);
            let mut buf = vec![0u8; buf_size];
            if let Ok(info) = reader.next_frame(&mut buf) {
                let w = info.width;
                let h = info.height;
                let rgba = match info.color_type {
                    png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
                    png::ColorType::Rgb => {
                        let mut r = Vec::with_capacity((w * h * 4) as usize);
                        for chunk in buf[..info.buffer_size()].chunks_exact(3) {
                            r.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
                        }
                        r
                    }
                    _ => return,
                };
                let _ = crate::clipboard::write_image(Some(hwnd), &rgba, w, h, &bytes);
            }
        }
    }
}

pub fn open_sidebar_window(main_hwnd: HWND, history_ptr: *mut HistoryManager) -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class_name = w!("SnapdockHistorySidebar");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(sidebar_wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            style: CS_DROPSHADOW,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        // Find monitor with cursor
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let _ = GetMonitorInfoW(hmon, &mut mi);

        let work_h = mi.rcWork.bottom - mi.rcWork.top;
        let count = (*history_ptr).entries.len();
        let (dock_w, dock_h) = calculate_dock_dimensions(count, work_h);
        let target_x = mi.rcWork.right - DOCK_RIGHT_MARGIN - dock_w;
        let start_x = mi.rcWork.right + 20; // Start off-screen to the right
        let y = mi.rcWork.top + (work_h - dock_h) / 2;

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class_name,
            w!("Snapdock History"),
            WS_POPUP | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            start_x,
            y,
            dock_w,
            dock_h,
            None,
            None,
            Some(instance.into()),
            None,
        ).ok()?;

        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);

        // Apply rounded window region
        let rgn = CreateRoundRectRgn(
            0,
            0,
            dock_w + 1,
            dock_h + 1,
            DOCK_CORNER_RADIUS * 2,
            DOCK_CORNER_RADIUS * 2,
        );
        let _ = SetWindowRgn(hwnd, Some(rgn), true);

        // Windows 11 DWM rounded corner preference
        let preference = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const _,
            std::mem::size_of::<u32>() as u32,
        );

        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);

        let theme = theme::ThemeColors::current();
        theme::apply_window_theme(hwnd, theme.is_dark);

        let mut state = Box::new(SidebarState::new(main_hwnd, history_ptr));
        state.anim = Some(SidebarAnim {
            is_closing: false,
            start_time: std::time::Instant::now(),
            duration: std::time::Duration::from_millis(180),
            start_x,
            target_x,
            y,
            w: dock_w,
            h: dock_h,
        });

        let state_raw = Box::into_raw(state);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_raw as isize);

        let _ = SetTimer(Some(hwnd), TIMER_ANIM_ID, 16, None);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);

        Some(hwnd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_dock_dimensions() {
        let work_h = 1000;

        // Empty state
        let (w0, h0) = calculate_dock_dimensions(0, work_h);
        assert_eq!(w0, DOCK_WIDTH);
        assert_eq!(h0, HEADER_HEIGHT + 70 + 8);

        // 1 item
        let (w1, h1) = calculate_dock_dimensions(1, work_h);
        assert_eq!(w1, DOCK_WIDTH);
        assert_eq!(h1, HEADER_HEIGHT + CARD_HEIGHT + 10);

        // 2 items
        let (w2, h2) = calculate_dock_dimensions(2, work_h);
        assert_eq!(w2, DOCK_WIDTH);
        assert_eq!(h2, HEADER_HEIGHT + 2 * CARD_HEIGHT + CARD_GAP + 10);
        assert!(h2 > h1);

        // Max height clamping on small screen
        let (_w_clamp, h_clamp) = calculate_dock_dimensions(20, 400);
        assert_eq!(h_clamp, 400 - 48);
    }

    #[test]
    fn test_header_hit_test() {
        let dummy_hwnd = HWND(std::ptr::null_mut());
        let mut manager = HistoryManager::new();
        let state = SidebarState::new(dummy_hwnd, &mut manager as *mut HistoryManager);

        // Select All button
        assert_eq!(
            hit_test(POINT { x: 20, y: 20 }, &state, 0),
            Some(SidebarHit::SelectAllBtn)
        );

        // Clear button
        assert_eq!(
            hit_test(POINT { x: 105, y: 20 }, &state, 0),
            Some(SidebarHit::ClearBtn)
        );

        // Close button
        assert_eq!(
            hit_test(POINT { x: 135, y: 20 }, &state, 0),
            Some(SidebarHit::CloseBtn)
        );
    }
}
