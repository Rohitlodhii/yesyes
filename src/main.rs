#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]

mod capture;
mod clipboard;
mod config;
mod dragout;
mod history;
mod hotkey;
mod overlay;
mod settings;
mod storage;
mod thumbnail;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static RUNNING: AtomicBool = AtomicBool::new(true);

struct AppContext {
    config: config::AppConfig,
    tray_icon: tray::TrayIcon,
    hotkey: hotkey::HotkeyManager,
    thumb_stack: thumbnail::ThumbnailStack,
    history: history::HistoryManager,
    settings_hwnd: Option<HWND>,
}

unsafe fn open_or_focus_settings(main_hwnd: HWND, ctx: &mut AppContext) {
    if let Some(existing) = ctx.settings_hwnd {
        if IsWindow(Some(existing)).as_bool() {
            let _ = SetForegroundWindow(existing);
            return;
        }
    }
    ctx.settings_hwnd = settings::open_settings_window(main_hwnd, &ctx.config);
}

fn capture_region_screenshot(hwnd: HWND, ctx: &mut AppContext) {
    match capture::CapturedFrame::capture() {
        Ok(frame) => {
            let selected_rect = overlay::run_overlay(&frame);
            if let Some(rect) = selected_rect {
                if let Some((rgba, w, h)) = frame.crop_to_rgba(rect) {
                    match storage::save_png_to_dir(&ctx.config.save_directory, &rgba, w, h) {
                        Ok((saved_path, png_bytes)) => {
                            let _ = clipboard::write_image(
                                Some(hwnd),
                                &rgba,
                                w,
                                h,
                                &png_bytes,
                            );

                            ctx.thumb_stack.push(
                                hwnd,
                                saved_path.clone(),
                                &rgba,
                                w,
                                h,
                                ctx.config.thumbnail_lifespan_secs,
                            );

                            ctx.history.push(saved_path.clone(), &rgba, w, h);

                            let msg = format!("Saved region screenshot: {}\n", saved_path.display());
                            let msg_w: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
                            unsafe {
                                windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(
                                    PCWSTR(msg_w.as_ptr()),
                                );
                            }
                        }
                        Err(e) => {
                            let err_msg = format!("Failed to save screenshot: {e}\n");
                            let err_w: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
                            unsafe {
                                windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(
                                    PCWSTR(err_w.as_ptr()),
                                );
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            let err_msg = format!("Capture failed: {e}\n");
            let err_w: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(
                    PCWSTR(err_w.as_ptr()),
                );
            }
        }
    }
}

fn capture_fullscreen_screenshot(hwnd: HWND, ctx: &mut AppContext) {
    match capture::CapturedFrame::capture() {
        Ok(frame) => {
            unsafe {
                // Find monitor holding cursor
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let _ = GetMonitorInfoW(hmon, &mut mi);
                let rect = mi.rcMonitor;

                if let Some((rgba, w, h)) = frame.crop_to_rgba(rect) {
                    match storage::save_png_to_dir(&ctx.config.save_directory, &rgba, w, h) {
                        Ok((saved_path, png_bytes)) => {
                            let _ = clipboard::write_image(
                                Some(hwnd),
                                &rgba,
                                w,
                                h,
                                &png_bytes,
                            );

                            ctx.thumb_stack.push(
                                hwnd,
                                saved_path.clone(),
                                &rgba,
                                w,
                                h,
                                ctx.config.thumbnail_lifespan_secs,
                            );

                            ctx.history.push(saved_path.clone(), &rgba, w, h);

                            let msg = format!("Saved fullscreen screenshot: {}\n", saved_path.display());
                            let msg_w: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
                            windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(
                                PCWSTR(msg_w.as_ptr()),
                            );
                        }
                        Err(e) => {
                            let err_msg = format!("Failed to save screenshot: {e}\n");
                            let err_w: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
                            windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(
                                PCWSTR(err_w.as_ptr()),
                            );
                        }
                    }
                }
            }
        }
        Err(e) => {
            let err_msg = format!("Fullscreen capture failed: {e}\n");
            let err_w: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(
                    PCWSTR(err_w.as_ptr()),
                );
            }
        }
    }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        tray::WM_SNAPDOCK_TRAY => {
            let event = (lparam.0 & 0xffff) as u32;
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppContext;
            if !ctx_ptr.is_null() {
                if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                    (*ctx_ptr).tray_icon.show_menu(hwnd);
                } else if event == WM_LBUTTONDBLCLK {
                    open_or_focus_settings(hwnd, &mut *ctx_ptr);
                }
            }
            LRESULT(0)
        }
        thumbnail::WM_SNAPDOCK_THUMB_DISMISSED => {
            let id = wparam.0 as usize;
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppContext;
            if !ctx_ptr.is_null() {
                (*ctx_ptr).thumb_stack.remove_by_id(id);
            }
            LRESULT(0)
        }
        settings::WM_SNAPDOCK_SETTINGS_SAVED => {
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppContext;
            if !ctx_ptr.is_null() {
                let new_cfg = *Box::from_raw(wparam.0 as *mut config::AppConfig);
                let ctx = &mut *ctx_ptr;

                if new_cfg.region_hotkey_modifiers != ctx.hotkey.region.modifiers
                    || new_cfg.region_hotkey_vk != ctx.hotkey.region.vk
                {
                    if let Err(e) = ctx.hotkey.rebind_region(
                        new_cfg.region_hotkey_modifiers,
                        new_cfg.region_hotkey_vk,
                    ) {
                        ctx.tray_icon.show_balloon("Hotkey Conflict", &e);
                    }
                }

                if new_cfg.fullscreen_hotkey_modifiers != ctx.hotkey.fullscreen.modifiers
                    || new_cfg.fullscreen_hotkey_vk != ctx.hotkey.fullscreen.vk
                {
                    if let Err(e) = ctx.hotkey.rebind_fullscreen(
                        new_cfg.fullscreen_hotkey_modifiers,
                        new_cfg.fullscreen_hotkey_vk,
                    ) {
                        ctx.tray_icon.show_balloon("Hotkey Conflict", &e);
                    }
                }

                if new_cfg.history_hotkey_modifiers != ctx.hotkey.history.modifiers
                    || new_cfg.history_hotkey_vk != ctx.hotkey.history.vk
                {
                    if let Err(e) = ctx.hotkey.rebind_history(
                        new_cfg.history_hotkey_modifiers,
                        new_cfg.history_hotkey_vk,
                    ) {
                        ctx.tray_icon.show_balloon("Hotkey Conflict", &e);
                    }
                }

                let tip = format!(
                    "Snapdock (Region: {}, Full: {})\0",
                    hotkey::format_hotkey(new_cfg.region_hotkey_modifiers, new_cfg.region_hotkey_vk),
                    hotkey::format_hotkey(new_cfg.fullscreen_hotkey_modifiers, new_cfg.fullscreen_hotkey_vk),
                );
                ctx.tray_icon.update_tip(&tip);
                ctx.config = new_cfg;
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            let id = wparam.0 as i32;
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppContext;
            if !ctx_ptr.is_null() {
                match id {
                    hotkey::SNAPDOCK_HOTKEY_REGION_ID => {
                        capture_region_screenshot(hwnd, &mut *ctx_ptr);
                    }
                    hotkey::SNAPDOCK_HOTKEY_FULLSCREEN_ID => {
                        capture_fullscreen_screenshot(hwnd, &mut *ctx_ptr);
                    }
                    hotkey::SNAPDOCK_HOTKEY_HISTORY_ID => {
                        (*ctx_ptr).history.toggle_sidebar(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as usize;
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppContext;
            if !ctx_ptr.is_null() {
                match id {
                    tray::IDM_REGION_CAPTURE => {
                        capture_region_screenshot(hwnd, &mut *ctx_ptr);
                    }
                    tray::IDM_FULLSCREEN_CAPTURE => {
                        capture_fullscreen_screenshot(hwnd, &mut *ctx_ptr);
                    }
                    tray::IDM_HISTORY => {
                        (*ctx_ptr).history.toggle_sidebar(hwnd);
                    }
                    tray::IDM_SETTINGS => {
                        open_or_focus_settings(hwnd, &mut *ctx_ptr);
                    }
                    tray::IDM_QUIT => {
                        RUNNING.store(false, Ordering::SeqCst);
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() -> Result<()> {
    // Attach UI thread to active input desktop before creating any windows
    unsafe {
        if let Ok(hdesk) = windows::Win32::System::StationsAndDesktops::OpenInputDesktop(
            windows::Win32::System::StationsAndDesktops::DESKTOP_CONTROL_FLAGS(0),
            false,
            windows::Win32::System::StationsAndDesktops::DESKTOP_ACCESS_FLAGS(0x01FF),
        ) {
            let _ = windows::Win32::System::StationsAndDesktops::SetThreadDesktop(hdesk);
            let _ = windows::Win32::System::StationsAndDesktops::CloseDesktop(hdesk);
        }
    }

    // Single-instance check
    let _mutex = unsafe {
        let m = CreateMutexW(None, true, w!("Local\\Snapdock_SingleInstance_Mutex"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(m);
            return Ok(());
        }
        m
    };

    // Initialize OLE on UI thread
    unsafe {
        OleInitialize(None)?;
    }

    unsafe {
        let instance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
        let class_name = w!("SnapdockMessageWindow");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name,
            w!("Snapdock"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;

        let cfg = config::AppConfig::load();
        let mut tray_icon = tray::TrayIcon::new(hwnd);
        let tip = format!(
            "Snapdock (Region: {}, Full: {})\0",
            hotkey::format_hotkey(cfg.region_hotkey_modifiers, cfg.region_hotkey_vk),
            hotkey::format_hotkey(cfg.fullscreen_hotkey_modifiers, cfg.fullscreen_hotkey_vk),
        );
        tray_icon.update_tip(&tip);

        let (hotkey, hotkey_warnings) = hotkey::HotkeyManager::new(
            hwnd,
            cfg.region_hotkey_modifiers,
            cfg.region_hotkey_vk,
            cfg.fullscreen_hotkey_modifiers,
            cfg.fullscreen_hotkey_vk,
            cfg.history_hotkey_modifiers,
            cfg.history_hotkey_vk,
        );

        for warning in hotkey_warnings {
            tray_icon.show_balloon("Hotkey Conflict", &warning);
        }

        let mut ctx = AppContext {
            config: cfg,
            tray_icon,
            hotkey,
            thumb_stack: thumbnail::ThumbnailStack::new(),
            history: history::HistoryManager::new(),
            settings_hwnd: None,
        };
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut ctx as *mut _ as isize);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        drop(ctx);
        let _ = CloseHandle(_mutex);
        OleUninitialize();
    }

    Ok(())
}
