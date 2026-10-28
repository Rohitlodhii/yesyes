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

const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_ANIM_ID: usize = 99;
const WM_REQUEST_TOGGLE: u32 = WM_USER + 102;

#[derive(Clone)]
pub struct HistoryEntry {
    pub id: usize,
    pub file_path: PathBuf,
    pub timestamp_str: String,
    pub width: u32,
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

        // Thumbnail: max 110 px wide, proportional height max 80 px
        let thumb_w = 110.min(orig_w).max(40);
        let thumb_h = (((orig_h as f32 / orig_w as f32) * thumb_w as f32).round() as u32).clamp(30, 80);

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
                        let _ = InvalidateRect(Some(hwnd), None, false);
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
                        let _ = InvalidateRect(Some(hwnd), None, false);
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
                    let _ = InvalidateRect(Some(hwnd), None, false);
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

// BGR Colors
const COLOR_SIDEBAR_BG: COLORREF = COLORREF(0x001B1818); // #18181B
const COLOR_CARD_BG: COLORREF = COLORREF(0x00272424);    // #242427
const COLOR_CARD_BORDER: COLORREF = COLORREF(0x003F3838);// #38383F
const COLOR_CARD_HOVER: COLORREF = COLORREF(0x00F6823B); // #3B82F6 (Blue)
const COLOR_TEXT_WHITE: COLORREF = COLORREF(0x00F5F4F4);
const COLOR_TEXT_MUTED: COLORREF = COLORREF(0x00AAA1A1);
const COLOR_BTN_BG: COLORREF = COLORREF(0x00383333);
const COLOR_BTN_HOVER: COLORREF = COLORREF(0x004A4444);
const COLOR_CLOSE_RED: COLORREF = COLORREF(0x003333E5);

const SIDEBAR_WIDTH: i32 = 380;
const HEADER_HEIGHT: i32 = 65;
const CARD_HEIGHT: i32 = 110;
const CARD_MARGIN: i32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarHit {
    CloseBtn,
    ClearBtn,
    Card(usize),
    CardCopy(usize),
    CardOpen(usize),
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
    drag_start: Option<(i32, i32, usize)>, // x, y, card index
    anim: Option<SidebarAnim>,

    font_title: HFONT,
    font_bold: HFONT,
    font_body: HFONT,
    font_small: HFONT,
}

impl SidebarState {
    fn new(main_hwnd: HWND, history_ptr: *mut HistoryManager) -> Self {
        unsafe {
            Self {
                _main_hwnd: main_hwnd,
                history_ptr,
                scroll_y: 0,
                hovered: None,
                drag_start: None,
                anim: None,
                font_title: create_font(17, 700),
                font_bold: create_font(13, 600),
                font_body: create_font(12, 400),
                font_small: create_font(11, 400),
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

unsafe fn start_close_animation(hwnd: HWND, state: &mut SidebarState) {
    if state.anim.as_ref().map_or(false, |a| a.is_closing) {
        return;
    }
    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    let work = get_monitor_work_rect(hwnd);
    let work_h = work.bottom - work.top;
    let cur_x = rect.left;
    let target_x = work.right;

    state.anim = Some(SidebarAnim {
        is_closing: true,
        start_time: std::time::Instant::now(),
        duration: std::time::Duration::from_millis(150),
        start_x: cur_x,
        target_x,
        y: work.top,
        w: SIDEBAR_WIDTH,
        h: work_h,
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
    // Header controls
    let close_rect = RECT { left: SIDEBAR_WIDTH - 36, top: 16, right: SIDEBAR_WIDTH - 12, bottom: 42 };
    let clear_rect = RECT { left: SIDEBAR_WIDTH - 96, top: 16, right: SIDEBAR_WIDTH - 44, bottom: 42 };

    unsafe {
        if PtInRect(&close_rect, pt).as_bool() {
            return Some(SidebarHit::CloseBtn);
        }
        if PtInRect(&clear_rect, pt).as_bool() {
            return Some(SidebarHit::ClearBtn);
        }
    }

    if pt.y < HEADER_HEIGHT {
        return None;
    }

    let y_in_cards = pt.y - HEADER_HEIGHT + state.scroll_y;
    let card_total_h = CARD_HEIGHT + CARD_MARGIN;
    let idx = (y_in_cards / card_total_h) as usize;

    if idx < count {
        let card_top = HEADER_HEIGHT + (idx as i32 * card_total_h) - state.scroll_y;
        let card_bottom = card_top + CARD_HEIGHT;

        if pt.y >= card_top && pt.y <= card_bottom && pt.x >= 12 && pt.x <= SIDEBAR_WIDTH - 12 {
            // Check button hits within this card
            // Buttons row at bottom of card: y: card_bottom - 32 .. card_bottom - 8
            let btn_top = card_bottom - 30;
            let btn_bottom = card_bottom - 8;

            if pt.y >= btn_top && pt.y <= btn_bottom {
                // Button 1: Copy (x: 130..178)
                if pt.x >= 130 && pt.x <= 178 {
                    return Some(SidebarHit::CardCopy(idx));
                }
                // Button 2: Open (x: 184..232)
                if pt.x >= 184 && pt.x <= 232 {
                    return Some(SidebarHit::CardOpen(idx));
                }
                // Button 3: Path (x: 238..286)
                if pt.x >= 238 && pt.x <= 286 {
                    return Some(SidebarHit::CardPath(idx));
                }
                // Button 4: Delete (x: 292..325)
                if pt.x >= 292 && pt.x <= 325 {
                    return Some(SidebarHit::CardDelete(idx));
                }
            }

            return Some(SidebarHit::Card(idx));
        }
    }

    None
}

unsafe fn draw_rounded_card(hdc: HDC, rect: &RECT, bg: COLORREF, border: COLORREF) {
    let brush = CreateSolidBrush(bg);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));

    let _ = RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, 10, 10);

    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

unsafe fn draw_btn(
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

unsafe fn paint_sidebar(hwnd: HWND, state: &SidebarState) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);

    let mut cr = RECT::default();
    let _ = GetClientRect(hwnd, &mut cr);

    let mem_dc = CreateCompatibleDC(Some(hdc));
    let mem_bmp = CreateCompatibleBitmap(hdc, cr.right, cr.bottom);
    let old_bmp = SelectObject(mem_dc, HGDIOBJ(mem_bmp.0));

    // Fill background
    let bg_brush = CreateSolidBrush(COLOR_SIDEBAR_BG);
    FillRect(mem_dc, &cr, bg_brush);
    let _ = DeleteObject(HGDIOBJ(bg_brush.0));

    let history = &*state.history_ptr;
    let count = history.entries.len();

    // --- CARDS LIST (Clipped to below header) ---
    let card_area = RECT { left: 0, top: HEADER_HEIGHT, right: cr.right, bottom: cr.bottom };
    let card_rgn = CreateRectRgn(card_area.left, card_area.top, card_area.right, card_area.bottom);
    SelectClipRgn(mem_dc, Some(card_rgn));

    let card_total_h = CARD_HEIGHT + CARD_MARGIN;
    for (idx, entry) in history.entries.iter().enumerate() {
        let card_top = HEADER_HEIGHT + (idx as i32 * card_total_h) - state.scroll_y;
        let card_bottom = card_top + CARD_HEIGHT;

        if card_bottom < HEADER_HEIGHT || card_top > cr.bottom {
            continue; // off-screen
        }

        let is_hovered = matches!(state.hovered, Some(SidebarHit::Card(i)) | Some(SidebarHit::CardCopy(i)) | Some(SidebarHit::CardOpen(i)) | Some(SidebarHit::CardPath(i)) | Some(SidebarHit::CardDelete(i)) if i == idx);

        let border_color = if is_hovered { COLOR_CARD_HOVER } else { COLOR_CARD_BORDER };
        let card_rect = RECT { left: 12, top: card_top, right: cr.right - 12, bottom: card_bottom };
        draw_rounded_card(mem_dc, &card_rect, COLOR_CARD_BG, border_color);

        // Blit Thumbnail on left
        let thumb_x = 22;
        let thumb_y = card_top + (CARD_HEIGHT - entry.thumb_h) / 2;
        let thumb_dc = CreateCompatibleDC(Some(mem_dc));
        let old_thumb_bmp = SelectObject(thumb_dc, HGDIOBJ(entry.thumb_hbitmap.0));
        let _ = BitBlt(mem_dc, thumb_x, thumb_y, entry.thumb_w, entry.thumb_h, Some(thumb_dc), 0, 0, SRCCOPY);
        SelectObject(thumb_dc, old_thumb_bmp);
        let _ = DeleteDC(thumb_dc);

        // Text metadata on right
        let text_left = thumb_x + entry.thumb_w + 12;

        let _ = SetBkMode(mem_dc, TRANSPARENT);

        // Timestamp
        let old_font = SelectObject(mem_dc, HGDIOBJ(state.font_bold.0));
        SetTextColor(mem_dc, COLOR_TEXT_WHITE);
        let mut time_w: Vec<u16> = format!("{}\0", entry.timestamp_str).encode_utf16().collect();
        let mut r_time = RECT { left: text_left, top: card_top + 14, right: cr.right - 20, bottom: card_top + 32 };
        DrawTextW(mem_dc, &mut time_w, &mut r_time, DT_LEFT | DT_SINGLELINE);

        // Resolution
        SelectObject(mem_dc, HGDIOBJ(state.font_small.0));
        SetTextColor(mem_dc, COLOR_TEXT_MUTED);
        let mut res_w: Vec<u16> = format!("{} × {} px\0", entry.width, entry.height).encode_utf16().collect();
        let mut r_res = RECT { left: text_left, top: card_top + 34, right: cr.right - 20, bottom: card_top + 50 };
        DrawTextW(mem_dc, &mut res_w, &mut r_res, DT_LEFT | DT_SINGLELINE);

        // File name (clipped)
        let file_name = entry.file_path.file_name().unwrap_or_default().to_string_lossy();
        let mut name_w: Vec<u16> = format!("{file_name}\0").encode_utf16().collect();
        let mut r_name = RECT { left: text_left, top: card_top + 52, right: cr.right - 20, bottom: card_top + 68 };
        DrawTextW(mem_dc, &mut name_w, &mut r_name, DT_LEFT | DT_SINGLELINE | DT_PATH_ELLIPSIS);

        // Action buttons row (Copy, Open, Path, Delete)
        let btn_top = card_bottom - 30;
        let btn_bottom = card_bottom - 8;

        let copy_hover = state.hovered == Some(SidebarHit::CardCopy(idx));
        let open_hover = state.hovered == Some(SidebarHit::CardOpen(idx));
        let path_hover = state.hovered == Some(SidebarHit::CardPath(idx));
        let del_hover = state.hovered == Some(SidebarHit::CardDelete(idx));

        draw_btn(mem_dc, &RECT { left: 130, top: btn_top, right: 178, bottom: btn_bottom }, "Copy", state.font_small, if copy_hover { COLOR_BTN_HOVER } else { COLOR_BTN_BG }, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);
        draw_btn(mem_dc, &RECT { left: 184, top: btn_top, right: 232, bottom: btn_bottom }, "Open", state.font_small, if open_hover { COLOR_BTN_HOVER } else { COLOR_BTN_BG }, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);
        draw_btn(mem_dc, &RECT { left: 238, top: btn_top, right: 286, bottom: btn_bottom }, "Path", state.font_small, if path_hover { COLOR_BTN_HOVER } else { COLOR_BTN_BG }, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);
        draw_btn(mem_dc, &RECT { left: 292, top: btn_top, right: 325, bottom: btn_bottom }, "✕", state.font_small, if del_hover { COLOR_CLOSE_RED } else { COLOR_BTN_BG }, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);

        SelectObject(mem_dc, old_font);
    }

    SelectClipRgn(mem_dc, None);
    let _ = DeleteObject(HGDIOBJ(card_rgn.0));

    if count == 0 {
        let old_font = SelectObject(mem_dc, HGDIOBJ(state.font_body.0));
        SetTextColor(mem_dc, COLOR_TEXT_MUTED);
        let mut empty_msg: Vec<u16> = "No screenshots taken yet.\nUse your hotkey to capture!".encode_utf16().chain(std::iter::once(0)).collect();
        let mut r_empty = RECT { left: 20, top: 200, right: cr.right - 20, bottom: 300 };
        DrawTextW(mem_dc, &mut empty_msg, &mut r_empty, DT_CENTER | DT_WORDBREAK);
        SelectObject(mem_dc, old_font);
    }

    // --- HEADER (Drawn on top to stay fixed while scrolling) ---
    let header_rect = RECT { left: 0, top: 0, right: cr.right, bottom: HEADER_HEIGHT };
    let h_brush = CreateSolidBrush(COLOR_SIDEBAR_BG);
    FillRect(mem_dc, &header_rect, h_brush);
    let _ = DeleteObject(HGDIOBJ(h_brush.0));

    // Bottom border of header
    let line_pen = CreatePen(PS_SOLID, 1, COLOR_CARD_BORDER);
    let old_pen = SelectObject(mem_dc, HGDIOBJ(line_pen.0));
    let _ = MoveToEx(mem_dc, 0, HEADER_HEIGHT - 1, None);
    let _ = LineTo(mem_dc, cr.right, HEADER_HEIGHT - 1);
    SelectObject(mem_dc, old_pen);
    let _ = DeleteObject(HGDIOBJ(line_pen.0));

    // Title
    let old_font = SelectObject(mem_dc, HGDIOBJ(state.font_title.0));
    SetTextColor(mem_dc, COLOR_TEXT_WHITE);
    let mut title_w: Vec<u16> = "History\0".encode_utf16().collect();
    let mut r_title = RECT { left: 16, top: 12, right: 150, bottom: 36 };
    DrawTextW(mem_dc, &mut title_w, &mut r_title, DT_LEFT | DT_SINGLELINE);

    // Count badge
    SelectObject(mem_dc, HGDIOBJ(state.font_small.0));
    SetTextColor(mem_dc, COLOR_TEXT_MUTED);
    let count_text = format!("{count} items\0");
    let mut count_w: Vec<u16> = count_text.encode_utf16().collect();
    let mut r_count = RECT { left: 16, top: 38, right: 150, bottom: 54 };
    DrawTextW(mem_dc, &mut count_w, &mut r_count, DT_LEFT | DT_SINGLELINE);

    // Clear Button
    let clear_hover = state.hovered == Some(SidebarHit::ClearBtn);
    let clear_rect = RECT { left: SIDEBAR_WIDTH - 96, top: 16, right: SIDEBAR_WIDTH - 44, bottom: 42 };
    draw_btn(mem_dc, &clear_rect, "Clear", state.font_small, if clear_hover { COLOR_BTN_HOVER } else { COLOR_BTN_BG }, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);

    // Close Button (✕)
    let close_hover = state.hovered == Some(SidebarHit::CloseBtn);
    let close_rect = RECT { left: SIDEBAR_WIDTH - 36, top: 16, right: SIDEBAR_WIDTH - 12, bottom: 42 };
    draw_btn(mem_dc, &close_rect, "✕", state.font_bold, if close_hover { COLOR_CLOSE_RED } else { COLOR_BTN_BG }, COLOR_TEXT_WHITE, COLOR_CARD_BORDER);

    SelectObject(mem_dc, old_font);

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
            let total_content_h = count as i32 * (CARD_HEIGHT + CARD_MARGIN);

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
                    let target_x = work.right - SIDEBAR_WIDTH;
                    state.anim = Some(SidebarAnim {
                        is_closing: false,
                        start_time: std::time::Instant::now(),
                        duration: std::time::Duration::from_millis(160),
                        start_x: rect.left,
                        target_x,
                        y: work.top,
                        w: SIDEBAR_WIDTH,
                        h: work.bottom - work.top,
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
                Some(SidebarHit::CloseBtn) => {
                    start_close_animation(hwnd, state);
                }
                Some(SidebarHit::ClearBtn) => {
                    history.clear();
                    state.scroll_y = 0;
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                Some(SidebarHit::CardCopy(idx)) => {
                    if let Some(entry) = history.entries.get(idx) {
                        copy_image_from_file(hwnd, &entry.file_path);
                    }
                }
                Some(SidebarHit::CardOpen(idx)) => {
                    if let Some(entry) = history.entries.get(idx) {
                        let path_w: Vec<u16> = entry.file_path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
                        let _ = ShellExecuteW(Some(hwnd), w!("open"), PCWSTR(path_w.as_ptr()), None, None, SW_SHOWNORMAL);
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
                        history.delete_by_id(id);
                    }
                }
                Some(SidebarHit::Card(idx)) => {
                    // Start drag tracking
                    state.drag_start = Some((x, y, idx));
                }
                None => {}
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            state.drag_start = None;
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
        let target_x = mi.rcWork.right - SIDEBAR_WIDTH;
        let start_x = mi.rcWork.right; // Start completely off-screen to the right
        let y = mi.rcWork.top;

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("Snapdock History"),
            WS_POPUP | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
            start_x,
            y,
            SIDEBAR_WIDTH,
            work_h,
            None,
            None,
            Some(instance.into()),
            None,
        ).ok()?;

        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);

        let dark_mode: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_mode as *const _ as *const _,
            std::mem::size_of::<i32>() as u32,
        );

        let mut state = Box::new(SidebarState::new(main_hwnd, history_ptr));
        state.anim = Some(SidebarAnim {
            is_closing: false,
            start_time: std::time::Instant::now(),
            duration: std::time::Duration::from_millis(180),
            start_x,
            target_x,
            y,
            w: SIDEBAR_WIDTH,
            h: work_h,
        });

        let state_raw = Box::into_raw(state);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_raw as isize);

        let _ = SetTimer(Some(hwnd), TIMER_ANIM_ID, 16, None);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);

        Some(hwnd)
    }
}
