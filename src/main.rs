#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]

mod capture;
mod clipboard;
mod config;
mod dragout;
mod hotkey;
mod overlay;
mod settings;
mod storage;
mod thumbnail;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static RUNNING: AtomicBool = AtomicBool::new(true);

struct AppContext {
    config: config::AppConfig,
    tray_icon: tray::TrayIcon,
    hotkey: hotkey::HotkeyManager,
    thumb_stack: thumbnail::ThumbnailStack,
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
                if new_cfg.hotkey_modifiers != ctx.hotkey.modifiers || new_cfg.hotkey_vk != ctx.hotkey.vk {
                    match ctx.hotkey.rebind(new_cfg.hotkey_modifiers, new_cfg.hotkey_vk) {
                        Ok(()) => {
                            let tip = format!("Snapdock ({})\0", hotkey::format_hotkey(new_cfg.hotkey_modifiers, new_cfg.hotkey_vk));
                            ctx.tray_icon.update_tip(&tip);
                        }
                        Err(err_msg) => {
                            ctx.tray_icon.show_balloon("Hotkey Conflict", &err_msg);
                        }
                    }
                }
                ctx.config = new_cfg;
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            if wparam.0 as i32 == hotkey::SNAPDOCK_HOTKEY_ID {
                let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppContext;
                if !ctx_ptr.is_null() {
                    match capture::CapturedFrame::capture() {
                        Ok(frame) => {
                            let selected_rect = overlay::run_overlay(&frame);
                            if let Some(rect) = selected_rect {
                                if let Some((rgba, w, h)) = frame.crop_to_rgba(rect) {
                                    match storage::save_png_to_dir(&(*ctx_ptr).config.save_directory, &rgba, w, h) {
                                        Ok((saved_path, png_bytes)) => {
                                            // Write to clipboard (CF_DIBV5 + registered PNG)
                                            let _ = clipboard::write_image(
                                                Some(hwnd),
                                                &rgba,
                                                w,
                                                h,
                                                &png_bytes,
                                            );

                                            // Push to floating thumbnail stack
                                            (*ctx_ptr).thumb_stack.push(
                                                hwnd,
                                                saved_path.clone(),
                                                &rgba,
                                                w,
                                                h,
                                                (*ctx_ptr).config.thumbnail_lifespan_secs,
                                            );

                                            let msg = format!("Saved screenshot: {}\n", saved_path.display());
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
                            let err_msg = format!("Capture failed: {e}\n");
                            let err_w: Vec<u16> = err_msg.encode_utf16().chain(std::iter::once(0)).collect();
                            windows::Win32::System::Diagnostics::Debug::OutputDebugStringW(
                                PCWSTR(err_w.as_ptr()),
                            );
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as usize;
            let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppContext;
            if id == tray::IDM_SETTINGS && !ctx_ptr.is_null() {
                open_or_focus_settings(hwnd, &mut *ctx_ptr);
            } else if id == tray::IDM_QUIT {
                RUNNING.store(false, Ordering::SeqCst);
                let _ = DestroyWindow(hwnd);
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
            // A second launch exits silently
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
        let tip = format!("Snapdock ({})\0", hotkey::format_hotkey(cfg.hotkey_modifiers, cfg.hotkey_vk));
        tray_icon.update_tip(&tip);

        let (hotkey, hotkey_result) = hotkey::HotkeyManager::new(hwnd, cfg.hotkey_modifiers, cfg.hotkey_vk);

        if let Err(err_msg) = hotkey_result {
            tray_icon.show_balloon("Hotkey Conflict", &err_msg);
        }

        let mut ctx = AppContext {
            config: cfg,
            tray_icon,
            hotkey,
            thumb_stack: thumbnail::ThumbnailStack::new(),
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
