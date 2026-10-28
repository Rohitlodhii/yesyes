use std::path::PathBuf;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_SNAPDOCK_THUMB_DISMISSED: u32 = WM_APP + 2;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_DISMISS_ID: usize = 1;
const TIMER_ANIM_ID: usize = 2;
pub const THUMB_CORNER_RADIUS: i32 = 8;

pub struct ThumbnailWindow {
    pub id: usize,
    pub hwnd: HWND,
    pub width: i32,
    pub height: i32,
    pub cur_y: i32,
    pub is_new: bool,
}

pub struct ThumbnailStack {
    next_id: usize,
    thumbnails: Vec<ThumbnailWindow>,
}

impl ThumbnailStack {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            thumbnails: Vec::new(),
        }
    }

    pub fn push(
        &mut self,
        main_hwnd: HWND,
        saved_path: PathBuf,
        rgba: &[u8],
        orig_w: u32,
        orig_h: u32,
        lifespan: Option<u32>,
    ) {
        // If lifespan is Some(0), thumbnail is disabled
        if lifespan == Some(0) {
            return;
        }

        // Limit to at most 5 thumbnails; oldest dismissed first
        while self.thumbnails.len() >= 5 {
            let oldest = self.thumbnails.remove(0);
            unsafe {
                let _ = DestroyWindow(oldest.hwnd);
            }
        }

        let id = self.next_id;
        self.next_id += 1;

        if let Some(thumb) = create_thumbnail_window(id, main_hwnd, saved_path, rgba, orig_w, orig_h, lifespan) {
            self.thumbnails.push(thumb);
            self.relayout();
        }
    }

    pub fn remove_by_id(&mut self, id: usize) {
        if let Some(idx) = self.thumbnails.iter().position(|t| t.id == id) {
            self.thumbnails.remove(idx);
            self.relayout();
        }
    }

    pub fn relayout(&mut self) {
        if self.thumbnails.is_empty() {
            return;
        }

        unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(hmon, &mut mi);

            // Stack above each other in the bottom-right of the monitor work area
            let margin = 20;
            let gap = 10;
            let mut cur_bottom = mi.rcWork.bottom - margin;

            for thumb in self.thumbnails.iter_mut() {
                let x = mi.rcWork.right - margin - thumb.width;
                let y = cur_bottom - thumb.height;
                cur_bottom = y - gap;

                if thumb.is_new {
                    thumb.is_new = false;
                    thumb.cur_y = y;
                    start_thumb_entry_anim(thumb.hwnd, x, y, thumb.width, thumb.height);
                } else if thumb.cur_y != y {
                    start_thumb_move_anim(thumb.hwnd, x, y, thumb.width, thumb.height);
                    thumb.cur_y = y;
                }
            }
        }
    }
}

struct ThumbAnim {
    is_exiting: bool,
    start_time: std::time::Instant,
    duration: std::time::Duration,
    target_x: i32,
    start_y: i32,
    target_y: i32,
    start_alpha: u8,
    target_alpha: u8,
}

struct ThumbnailState {
    id: usize,
    main_hwnd: HWND,
    saved_path: PathBuf,
    width: i32,
    height: i32,
    mem_hdc: HDC,
    hbitmap: HBITMAP,
    old_bitmap: HGDIOBJ,

    is_hovering: bool,
    remaining_ticks: Option<u32>, // None = infinite; Some(n) = countdown ticks
    copied_indicator: bool,

    drag_start: Option<(i32, i32)>,
    anim: Option<ThumbAnim>,
    font_icon: HFONT,
}

unsafe fn start_thumb_entry_anim(hwnd: HWND, target_x: i32, target_y: i32, w: i32, h: i32) {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ThumbnailState;
    if state_ptr.is_null() {
        return;
    }
    let state = &mut *state_ptr;

    let start_y = target_y + 16;
    let _ = SetWindowPos(
        hwnd,
        Some(HWND_TOPMOST),
        target_x,
        start_y,
        w,
        h,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);

    state.anim = Some(ThumbAnim {
        is_exiting: false,
        start_time: std::time::Instant::now(),
        duration: std::time::Duration::from_millis(160),
        target_x,
        start_y,
        target_y,
        start_alpha: 0,
        target_alpha: 255,
    });
    let _ = SetTimer(Some(hwnd), TIMER_ANIM_ID, 16, None);
}

unsafe fn start_thumb_move_anim(hwnd: HWND, target_x: i32, target_y: i32, _w: i32, _h: i32) {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ThumbnailState;
    if state_ptr.is_null() {
        return;
    }
    let state = &mut *state_ptr;

    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    let cur_y = rect.top;
    if cur_y == target_y {
        return;
    }

    state.anim = Some(ThumbAnim {
        is_exiting: false,
        start_time: std::time::Instant::now(),
        duration: std::time::Duration::from_millis(140),
        target_x,
        start_y: cur_y,
        target_y,
        start_alpha: 255,
        target_alpha: 255,
    });
    let _ = SetTimer(Some(hwnd), TIMER_ANIM_ID, 16, None);
}

unsafe fn start_thumb_exit_anim(hwnd: HWND, state: &mut ThumbnailState) {
    if state.anim.as_ref().map_or(false, |a| a.is_exiting) {
        return;
    }
    let _ = KillTimer(Some(hwnd), TIMER_DISMISS_ID);

    // Notify main window to remove from stack tracking immediately so layout adjusts
    let _ = PostMessageW(
        Some(state.main_hwnd),
        WM_SNAPDOCK_THUMB_DISMISSED,
        WPARAM(state.id),
        LPARAM(0),
    );

    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);

    let cur_alpha = if let Some(anim) = &state.anim {
        let elapsed = anim.start_time.elapsed().as_secs_f32() * 1000.0;
        let total = anim.duration.as_secs_f32() * 1000.0;
        let t = (elapsed / total).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - t).powi(3);
        (anim.start_alpha as f32 + ((anim.target_alpha as f32 - anim.start_alpha as f32) * ease)).round() as u8
    } else {
        255
    };

    state.anim = Some(ThumbAnim {
        is_exiting: true,
        start_time: std::time::Instant::now(),
        duration: std::time::Duration::from_millis(120),
        target_x: rect.left,
        start_y: rect.top,
        target_y: rect.top + 6,
        start_alpha: cur_alpha,
        target_alpha: 0,
    });
    let _ = SetTimer(Some(hwnd), TIMER_ANIM_ID, 16, None);
}

unsafe extern "system" fn thumb_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut ThumbnailState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *state_ptr;

    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER => {
            if wparam.0 == TIMER_DISMISS_ID {
                // Auto-dismiss paused while mouse hovers
                if !state.is_hovering {
                    if let Some(ticks) = state.remaining_ticks.as_mut() {
                        if *ticks > 0 {
                            *ticks -= 1;
                        }
                        if *ticks == 0 {
                            start_thumb_exit_anim(hwnd, state);
                        }
                    }
                }
            } else if wparam.0 == TIMER_ANIM_ID {
                if let Some(anim) = &mut state.anim {
                    let elapsed = anim.start_time.elapsed().as_secs_f32() * 1000.0;
                    let total = anim.duration.as_secs_f32() * 1000.0;
                    let t = (elapsed / total).clamp(0.0, 1.0);

                    let ease = if anim.is_exiting {
                        t * t
                    } else {
                        1.0 - (1.0 - t).powi(3)
                    };

                    let cur_y = anim.start_y + ((anim.target_y - anim.start_y) as f32 * ease).round() as i32;
                    let cur_alpha = (anim.start_alpha as f32 + ((anim.target_alpha as f32 - anim.start_alpha as f32) * ease)).round() as u8;

                    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), cur_alpha, LWA_ALPHA);
                    let _ = SetWindowPos(
                        hwnd,
                        Some(HWND_TOPMOST),
                        anim.target_x,
                        cur_y,
                        state.width,
                        state.height,
                        SWP_NOACTIVATE,
                    );

                    if t >= 1.0 {
                        let is_exiting = anim.is_exiting;
                        state.anim = None;
                        let _ = KillTimer(Some(hwnd), TIMER_ANIM_ID);
                        if is_exiting {
                            let _ = DestroyWindow(hwnd);
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if state.anim.as_ref().map_or(false, |a| a.is_exiting) {
                return LRESULT(0);
            }

            let x = (lparam.0 & 0xffff) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;

            if !state.is_hovering {
                state.is_hovering = true;
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }

            // Drag-out check
            if (wparam.0 & 0x0001) != 0 {
                if let Some((sx, sy)) = state.drag_start {
                    let dx = (x - sx).abs();
                    let dy = (y - sy).abs();
                    if dx > 4 || dy > 4 {
                        state.drag_start = None;
                        start_drag_out(hwnd, state);
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            state.is_hovering = false;
            state.drag_start = None;
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            if state.anim.as_ref().map_or(false, |a| a.is_exiting) {
                return LRESULT(0);
            }
            let x = (lparam.0 & 0xffff) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;
            state.drag_start = Some((x, y));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if state.anim.as_ref().map_or(false, |a| a.is_exiting) {
                return LRESULT(0);
            }
            let x = (lparam.0 & 0xffff) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32;

            if let Some((sx, sy)) = state.drag_start.take() {
                let dx = (x - sx).abs();
                let dy = (y - sy).abs();
                if dx <= 4 && dy <= 4 {
                    // Plain click without dragging
                    if state.is_hovering && is_close_button(x, y, state.width) {
                        start_thumb_exit_anim(hwnd, state);
                    } else if state.is_hovering && is_copy_button(x, y) {
                        // Clicked copy-path button
                        if let Some(path_str) = state.saved_path.to_str() {
                            let _ = crate::clipboard::write_text(Some(hwnd), path_str);
                            state.copied_indicator = true;
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                    } else {
                        // Plain click on thumbnail opens PNG in default viewer
                        let path_w: Vec<u16> = state
                            .saved_path
                            .as_os_str()
                            .encode_wide()
                            .chain(std::iter::once(0))
                            .collect();
                        ShellExecuteW(
                            None,
                            w!("open"),
                            PCWSTR(path_w.as_ptr()),
                            None,
                            None,
                            SW_SHOWNORMAL,
                        );
                    }
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            if state.anim.as_ref().map_or(false, |a| a.is_exiting) {
                return LRESULT(0);
            }
            start_thumb_exit_anim(hwnd, state);
            LRESULT(0)
        }
        crate::theme::WM_SETTINGCHANGE | crate::theme::WM_THEMECHANGED => {
            let _ = InvalidateRect(Some(hwnd), None, false);
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            let radius = THUMB_CORNER_RADIUS
                .min(state.width / 4)
                .min(state.height / 4)
                .max(4);
            let diameter = radius * 2;

            // Clip drawing to rounded rectangle so the blitted image follows rounded corners
            let clip_rgn = CreateRoundRectRgn(
                0,
                0,
                state.width + 1,
                state.height + 1,
                diameter,
                diameter,
            );
            SelectClipRgn(hdc, Some(clip_rgn));

            // Blit downscaled image
            let _ = BitBlt(hdc, 0, 0, state.width, state.height, Some(state.mem_hdc), 0, 0, SRCCOPY);

            // Reset clipping
            SelectClipRgn(hdc, None);
            let _ = DeleteObject(HGDIOBJ(clip_rgn.0));

            // Rounded border around the thumbnail
            paint_thumbnail_border(state, hdc, radius);

            // Hover UI: buttons bar
            if state.is_hovering {
                paint_hover_ui(state, hdc);
            }

            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let _ = KillTimer(Some(hwnd), TIMER_DISMISS_ID);
            let _ = KillTimer(Some(hwnd), TIMER_ANIM_ID);
            let _ = DeleteObject(HGDIOBJ(state.font_icon.0));
            SelectObject(state.mem_hdc, state.old_bitmap);
            let _ = DeleteObject(HGDIOBJ(state.hbitmap.0));
            let _ = DeleteDC(state.mem_hdc);
            let _ = Box::from_raw(state_ptr);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn is_close_button(x: i32, y: i32, w: i32) -> bool {
    x >= (w - 26) && x <= (w - 4) && y >= 4 && y <= 26
}

fn is_copy_button(x: i32, y: i32) -> bool {
    x >= 4 && x <= 72 && y >= 4 && y <= 26
}

unsafe fn paint_thumbnail_border(state: &ThumbnailState, hdc: HDC, radius: i32) {
    let theme = crate::theme::ThemeColors::current();
    let border_color = if state.is_hovering {
        theme.card_hover
    } else if theme.is_dark {
        // High-contrast clean subtle border for dark mode
        crate::theme::rgb(85, 90, 100)
    } else {
        // Crisp border for light mode
        crate::theme::rgb(195, 200, 210)
    };

    let diameter = radius * 2;
    let pen = CreatePen(PS_SOLID, 1, border_color);
    let null_brush = GetStockObject(HOLLOW_BRUSH);
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    let old_brush = SelectObject(hdc, null_brush);

    let _ = RoundRect(
        hdc,
        0,
        0,
        state.width,
        state.height,
        diameter,
        diameter,
    );

    SelectObject(hdc, old_pen);
    SelectObject(hdc, old_brush);
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

unsafe fn draw_rounded_badge(hdc: HDC, rect: &RECT, bg: COLORREF, border: COLORREF, radius: i32) {
    let brush = CreateSolidBrush(bg);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));

    let _ = RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, radius * 2, radius * 2);

    SelectObject(hdc, old_pen);
    SelectObject(hdc, old_brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

unsafe fn paint_hover_ui(state: &ThumbnailState, hdc: HDC) {
    let theme = crate::theme::ThemeColors::current();

    // Close button (top right): red badge with Fluent vector '✕'
    let close_rect = RECT {
        left: state.width - 24,
        top: 6,
        right: state.width - 6,
        bottom: 24,
    };
    draw_rounded_badge(
        hdc,
        &close_rect,
        COLORREF(0x003333E5), // soft red
        COLORREF(0x002222B5), // border
        4,
    );

    let _ = SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, COLORREF(0x00FFFFFF));
    let old_font = SelectObject(hdc, HGDIOBJ(state.font_icon.0));
    let mut x_text: Vec<u16> = crate::theme::icons::CLOSE.encode_utf16().chain(std::iter::once(0)).collect();
    let mut cr = close_rect;
    DrawTextW(hdc, &mut x_text, &mut cr, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // Copy path button (top left): badge with Fluent vector icon + text
    let copy_rect = RECT {
        left: 6,
        top: 6,
        right: if state.copied_indicator { 70 } else { 58 },
        bottom: 24,
    };
    let (copy_bg, copy_border) = if theme.is_dark {
        (COLORREF(0x00222222), COLORREF(0x00444444))
    } else {
        (COLORREF(0x00F0F0F0), COLORREF(0x00CCCCCC))
    };
    draw_rounded_badge(hdc, &copy_rect, copy_bg, copy_border, 4);

    let (icon, label) = if state.copied_indicator {
        (crate::theme::icons::CHECKMARK, "Copied")
    } else {
        (crate::theme::icons::COPY, "Path")
    };

    let text_color = if theme.is_dark { COLORREF(0x00FFFFFF) } else { COLORREF(0x00111111) };
    SetTextColor(hdc, text_color);

    // Draw icon
    let mut icon_w: Vec<u16> = icon.encode_utf16().chain(std::iter::once(0)).collect();
    let mut r_icon = RECT { left: 9, top: 6, right: 23, bottom: 24 };
    DrawTextW(hdc, &mut icon_w, &mut r_icon, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

    // Draw label
    SelectObject(hdc, old_font);
    let mut label_w: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
    let mut r_lbl = RECT { left: 25, top: 6, right: copy_rect.right - 4, bottom: 24 };
    DrawTextW(hdc, &mut label_w, &mut r_lbl, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
}

use std::os::windows::ffi::OsStrExt;

fn create_thumbnail_window(
    id: usize,
    main_hwnd: HWND,
    saved_path: PathBuf,
    rgba: &[u8],
    orig_w: u32,
    orig_h: u32,
    lifespan: Option<u32>,
) -> Option<ThumbnailWindow> {
    if orig_w == 0 || orig_h == 0 {
        return None;
    }

    // Scale to at most 160 px wide
    let thumb_w = 160.min(orig_w).max(40);
    let thumb_h = ((orig_h as f32 / orig_w as f32) * thumb_w as f32).round().max(20.0).min(240.0) as u32;

    let downscaled_rgba = downscale_rgba(rgba, orig_w, orig_h, thumb_w, thumb_h);

    unsafe {
        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok()?;
        let class_name = w!("SnapdockThumbnailWindow");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(thumb_wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            style: CS_DROPSHADOW,
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        let hdc_screen = GetDC(None);
        let mem_hdc = CreateCompatibleDC(Some(hdc_screen));

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: thumb_w as i32,
                biHeight: -(thumb_h as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: (thumb_w * thumb_h * 4),
                ..Default::default()
            },
            bmiColors: [RGBQUAD::default(); 1],
        };

        let mut bits_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbitmap = CreateDIBSection(
            Some(hdc_screen),
            &bmi,
            DIB_RGB_COLORS,
            &mut bits_ptr,
            None,
            0,
        );
        ReleaseDC(None, hdc_screen);

        let hbitmap = match hbitmap {
            Ok(b) => b,
            Err(_) => {
                let _ = DeleteDC(mem_hdc);
                return None;
            }
        };

        let old_bitmap = SelectObject(mem_hdc, HGDIOBJ(hbitmap.0));

        // Copy downscaled RGBA to BGRA
        let total_pixels = (thumb_w * thumb_h) as usize;
        let dst_slice = std::slice::from_raw_parts_mut(bits_ptr as *mut u8, total_pixels * 4);
        for (src_chunk, dst_chunk) in downscaled_rgba.chunks_exact(4).zip(dst_slice.chunks_exact_mut(4)) {
            dst_chunk[0] = src_chunk[2]; // B
            dst_chunk[1] = src_chunk[1]; // G
            dst_chunk[2] = src_chunk[0]; // R
            dst_chunk[3] = 255;          // A
        }

        let state = Box::new(ThumbnailState {
            id,
            main_hwnd,
            saved_path,
            width: thumb_w as i32,
            height: thumb_h as i32,
            mem_hdc,
            hbitmap,
            old_bitmap,
            is_hovering: false,
            remaining_ticks: lifespan,
            copied_indicator: false,
            drag_start: None,
            anim: None,
            font_icon: crate::theme::create_icon_font(11),
        });

        // Styles: WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            class_name,
            w!("SnapdockThumbnail"),
            WS_POPUP,
            0,
            0,
            thumb_w as i32,
            thumb_h as i32,
            None,
            None,
            Some(instance.into()),
            None,
        );

        let hwnd = match hwnd {
            Ok(h) => h,
            Err(_) => {
                SelectObject(mem_hdc, old_bitmap);
                let _ = DeleteObject(HGDIOBJ(hbitmap.0));
                let _ = DeleteDC(mem_hdc);
                return None;
            }
        };

        // Exclude thumbnail from screen capture
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);

        // Apply rounded window region so corners outside the thumbnail are fully clipped
        let radius = THUMB_CORNER_RADIUS
            .min((thumb_w as i32) / 4)
            .min((thumb_h as i32) / 4)
            .max(4);
        let diameter = radius * 2;
        let rgn = CreateRoundRectRgn(
            0,
            0,
            thumb_w as i32 + 1,
            thumb_h as i32 + 1,
            diameter,
            diameter,
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

        // Initially fully transparent until entry animation starts
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);

        let state_raw = Box::into_raw(state);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_raw as isize);

        // If timed, start 1-second auto-dismiss timer
        if let Some(ticks) = lifespan {
            if ticks > 0 {
                let _ = SetTimer(Some(hwnd), TIMER_DISMISS_ID, 1000, None);
            }
        }

        Some(ThumbnailWindow {
            id,
            hwnd,
            width: thumb_w as i32,
            height: thumb_h as i32,
            cur_y: 0,
            is_new: true,
        })
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

fn start_drag_out(_hwnd: HWND, state: &ThumbnailState) {
    crate::dragout::execute_drag_drop(&state.saved_path, state.hbitmap, state.width, state.height);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_hit_testing() {
        let width = 160;
        // Close button: right - 26 to right - 4, y: 4..=26
        assert!(is_close_button(width - 15, 15, width));
        assert!(is_close_button(width - 26, 4, width));
        assert!(is_close_button(width - 4, 26, width));
        assert!(!is_close_button(width - 30, 15, width));
        assert!(!is_close_button(width - 15, 30, width));

        // Copy button: 4..=72, y: 4..=26
        assert!(is_copy_button(20, 15));
        assert!(is_copy_button(4, 4));
        assert!(is_copy_button(72, 26));
        assert!(!is_copy_button(75, 15));
        assert!(!is_copy_button(2, 15));
        assert!(!is_copy_button(20, 30));

        // Close and Copy buttons do not overlap
        assert!(!is_close_button(20, 15, width));
        assert!(!is_copy_button(width - 15, 15));
    }

    #[test]
    fn test_thumbnail_corner_radius() {
        // Standard thumbnail (160x100) -> radius should be 8
        let r1 = THUMB_CORNER_RADIUS.min(160 / 4).min(100 / 4).max(4);
        assert_eq!(r1, 8);

        // Very thin ribbon (160x20) -> radius should scale down safely to 5
        let r2 = THUMB_CORNER_RADIUS.min(160 / 4).min(20 / 4).max(4);
        assert_eq!(r2, 5);

        // Extremely small -> clamped to at least 4
        let r3 = THUMB_CORNER_RADIUS.min(10 / 4).min(10 / 4).max(4);
        assert_eq!(r3, 4);
    }

    #[test]
    fn test_rounded_region_creation() {
        unsafe {
            let width = 160;
            let height = 100;
            let radius = THUMB_CORNER_RADIUS.min(width / 4).min(height / 4).max(4);
            let diameter = radius * 2;
            let rgn = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
            assert!(!rgn.is_invalid());

            // Center should be in region
            assert!(PtInRegion(rgn, width / 2, height / 2).as_bool());
            // Top-left corner (0, 0) should be outside due to rounding
            assert!(!PtInRegion(rgn, 0, 0).as_bool());
            // Bottom-right corner (width - 1, height - 1) should be outside due to rounding
            assert!(!PtInRegion(rgn, width - 1, height - 1).as_bool());

            let _ = DeleteObject(HGDIOBJ(rgn.0));
        }
    }
}
