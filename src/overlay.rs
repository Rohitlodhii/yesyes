use crate::capture::CapturedFrame;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

struct OverlayState<'a> {
    frame: &'a CapturedFrame,
    dimmed_hdc: HDC,

    is_mouse_down: bool,
    is_drag: bool,
    start_pt: (i32, i32), // virtual screen coordinates
    cur_pt: (i32, i32),
    selection: Option<RECT>, // virtual screen coordinates

    is_active: bool,
    confirmed_rect: Option<RECT>,
}

unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayState;
    if state_ptr.is_null() {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let state = &mut *state_ptr;

    match msg {
        WM_ERASEBKGND => LRESULT(1), // Prevent flickering
        WM_SETCURSOR => {
            let cursor = LoadCursorW(None, IDC_CROSS).unwrap_or_default();
            SetCursor(Some(cursor));
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xffff) as i16 as i32 + state.frame.vx;
            let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32 + state.frame.vy;
            state.is_mouse_down = true;
            state.is_drag = false;
            state.start_pt = (x, y);
            state.cur_pt = (x, y);
            state.selection = None;
            let _ = SetCapture(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if state.is_mouse_down {
                let x = (lparam.0 & 0xffff) as i16 as i32 + state.frame.vx;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32 + state.frame.vy;
                state.cur_pt = (x, y);

                let dx = (x - state.start_pt.0).abs();
                let dy = (y - state.start_pt.1).abs();
                if dx > 3 || dy > 3 {
                    state.is_drag = true;
                    state.selection = Some(RECT {
                        left: state.start_pt.0.min(x),
                        top: state.start_pt.1.min(y),
                        right: state.start_pt.0.max(x),
                        bottom: state.start_pt.1.max(y),
                    });
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if state.is_mouse_down {
                state.is_mouse_down = false;
                let _ = ReleaseCapture();

                let x = (lparam.0 & 0xffff) as i16 as i32 + state.frame.vx;
                let y = ((lparam.0 >> 16) & 0xffff) as i16 as i32 + state.frame.vy;

                if state.is_drag {
                    // Confirmed dragged rectangle
                    let rect = RECT {
                        left: state.start_pt.0.min(x),
                        top: state.start_pt.1.min(y),
                        right: state.start_pt.0.max(x),
                        bottom: state.start_pt.1.max(y),
                    };
                    if (rect.right - rect.left) > 4 && (rect.bottom - rect.top) > 4 {
                        state.confirmed_rect = Some(rect);
                    }
                    state.is_active = false;
                } else {
                    // Click without dragging: select window under cursor using DWMWA_EXTENDED_FRAME_BOUNDS
                    let pt = POINT { x, y };
                    let snapped = find_window_at_point(pt, hwnd);
                    if let Some(w_rect) = snapped {
                        state.confirmed_rect = Some(w_rect);
                    } else {
                        // Fallback to monitor under cursor
                        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
                        let mut mi = MONITORINFO {
                            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                            ..Default::default()
                        };
                        let _ = GetMonitorInfoW(hmon, &mut mi);
                        state.confirmed_rect = Some(mi.rcMonitor);
                    }
                    state.is_active = false;
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let key = wparam.0 as u32;
            if key == VK_ESCAPE.0 as u32 {
                // Esc cancels
                state.confirmed_rect = None;
                state.is_active = false;
            } else if key == VK_SPACE.0 as u32 {
                // Space captures full monitor under cursor
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let _ = GetMonitorInfoW(hmon, &mut mi);
                state.confirmed_rect = Some(mi.rcMonitor);
                state.is_active = false;
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            paint_overlay(state, hdc);

            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_overlay(state: &OverlayState, hdc: HDC) {
    let f = state.frame;
    if let Some(sel) = state.selection {
        let lx = sel.left - f.vx;
        let ly = sel.top - f.vy;
        let lw = sel.right - sel.left;
        let lh = sel.bottom - sel.top;

        // Blit dimmed background
        let _ = BitBlt(hdc, 0, 0, f.width, f.height, Some(state.dimmed_hdc), 0, 0, SRCCOPY);

        // Blit undimmed selection
        if lw > 0 && lh > 0 {
            let _ = BitBlt(hdc, lx, ly, lw, lh, Some(f.hdc), lx, ly, SRCCOPY);

            // Draw crisp 2px border around selection
            let border_brush = CreateSolidBrush(COLORREF(0x00FF8000)); // vibrant modern cyan-blue
            let white_brush = CreateSolidBrush(COLORREF(0x00FFFFFF));
            let r1 = RECT {
                left: lx - 1,
                top: ly - 1,
                right: lx + lw + 1,
                bottom: ly + lh + 1,
            };
            let r2 = RECT {
                left: lx - 2,
                top: ly - 2,
                right: lx + lw + 2,
                bottom: ly + lh + 2,
            };
            FrameRect(hdc, &r2, border_brush);
            FrameRect(hdc, &r1, white_brush);
            let _ = DeleteObject(HGDIOBJ(border_brush.0));
            let _ = DeleteObject(HGDIOBJ(white_brush.0));

            // Draw live width × height badge
            let label = format!("{} × {}", lw, lh);
            let mut label_w: Vec<u16> = label.encode_utf16().collect();

            let badge_w = 90.max(label.len() as i32 * 9 + 16);
            let badge_h = 24;
            let mut badge_y = ly + lh + 6;
            if badge_y + badge_h > f.height - 10 {
                badge_y = ly - badge_h - 6;
            }
            let mut badge_x = lx + (lw - badge_w) / 2;
            if badge_x < 4 {
                badge_x = 4;
            } else if badge_x + badge_w > f.width - 4 {
                badge_x = f.width - badge_w - 4;
            }

            let badge_rect = RECT {
                left: badge_x,
                top: badge_y,
                right: badge_x + badge_w,
                bottom: badge_y + badge_h,
            };

            let bg_brush = CreateSolidBrush(COLORREF(0x001E1E1E));
            FillRect(hdc, &badge_rect, bg_brush);
            let _ = DeleteObject(HGDIOBJ(bg_brush.0));

            let _ = SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, COLORREF(0x00FFFFFF));
            let mut text_rect = badge_rect;
            DrawTextW(
                hdc,
                &mut label_w,
                &mut text_rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
    } else {
        // Entire screen dimmed
        let _ = BitBlt(hdc, 0, 0, f.width, f.height, Some(state.dimmed_hdc), 0, 0, SRCCOPY);
    }
}

struct WindowFindContext {
    pt: POINT,
    overlay_hwnd: HWND,
    found_rect: Option<RECT>,
}

unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut WindowFindContext);
    if hwnd == ctx.overlay_hwnd {
        return BOOL(1);
    }
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return BOOL(1);
    }
    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
        return BOOL(1);
    }

    let mut rect = RECT::default();
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        &mut rect as *mut _ as _,
        std::mem::size_of::<RECT>() as u32,
    ).is_ok() {
        if ctx.pt.x >= rect.left && ctx.pt.x < rect.right && ctx.pt.y >= rect.top && ctx.pt.y < rect.bottom {
            if rect.right > rect.left && rect.bottom > rect.top {
                ctx.found_rect = Some(rect);
                return BOOL(0); // Stop enumeration
            }
        }
    }
    BOOL(1)
}

fn find_window_at_point(pt: POINT, overlay_hwnd: HWND) -> Option<RECT> {
    let mut ctx = WindowFindContext {
        pt,
        overlay_hwnd,
        found_rect: None,
    };
    unsafe {
        let _ = EnumWindows(Some(enum_windows_callback), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.found_rect
}

pub fn run_overlay(frame: &CapturedFrame) -> Option<RECT> {
    unsafe {
        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok()?;
        let class_name = w!("SnapdockOverlayWindow");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(overlay_wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
            ..Default::default()
        };
        let _ = RegisterClassW(&wc);

        // Create dimmed copy of screen DIB
        let dimmed_hdc = CreateCompatibleDC(Some(frame.hdc));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: frame.width,
                biHeight: -frame.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0,
                biSizeImage: (frame.width * frame.height * 4) as u32,
                ..Default::default()
            },
            bmiColors: [RGBQUAD::default(); 1],
        };

        let mut dimmed_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let dimmed_hbitmap = match CreateDIBSection(
            Some(frame.hdc),
            &bmi,
            DIB_RGB_COLORS,
            &mut dimmed_bits,
            None,
            0,
        ) {
            Ok(b) => b,
            Err(_) => {
                let _ = DeleteDC(dimmed_hdc);
                return None;
            }
        };

        let old_dimmed = SelectObject(dimmed_hdc, HGDIOBJ(dimmed_hbitmap.0));

        // Dim pixels to 50% brightness
        let total_bytes = (frame.width * frame.height * 4) as usize;
        let src_slice = std::slice::from_raw_parts(frame.bits, total_bytes);
        let dst_slice = std::slice::from_raw_parts_mut(dimmed_bits as *mut u8, total_bytes);
        for (s, d) in src_slice.iter().zip(dst_slice.iter_mut()) {
            *d = *s >> 1; // 50% luminance
        }

        let mut state = OverlayState {
            frame,
            dimmed_hdc,
            is_mouse_down: false,
            is_drag: false,
            start_pt: (0, 0),
            cur_pt: (0, 0),
            selection: None,
            is_active: true,
            confirmed_rect: None,
        };

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class_name,
            w!("SnapdockOverlay"),
            WS_POPUP,
            frame.vx,
            frame.vy,
            frame.width,
            frame.height,
            None,
            None,
            Some(instance.into()),
            None,
        );

        let hwnd = match hwnd {
            Ok(h) => h,
            Err(_) => {
                SelectObject(dimmed_hdc, old_dimmed);
                let _ = DeleteObject(HGDIOBJ(dimmed_hbitmap.0));
                let _ = DeleteDC(dimmed_hdc);
                return None;
            }
        };

        // Exclude overlay window from screen capture
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut state as *mut _ as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);

        // Modal overlay event loop
        let mut msg = MSG::default();
        while state.is_active && GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = DestroyWindow(hwnd);

        // Free dimmed GDI objects
        SelectObject(dimmed_hdc, old_dimmed);
        let _ = DeleteObject(HGDIOBJ(dimmed_hbitmap.0));
        let _ = DeleteDC(dimmed_hdc);

        state.confirmed_rect
    }
}
