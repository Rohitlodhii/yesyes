#![allow(unsafe_op_in_unsafe_fn)]

mod capture;
mod hotkey;
mod overlay;
mod storage;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static RUNNING: AtomicBool = AtomicBool::new(true);

struct AppContext {
    tray_icon: tray::TrayIcon,
    _hotkey: hotkey::HotkeyManager,
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
            if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                let ctx_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const AppContext;
                if !ctx_ptr.is_null() {
                    (*ctx_ptr).tray_icon.show_menu(hwnd);
                }
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            if wparam.0 as i32 == hotkey::SNAPDOCK_HOTKEY_ID {
                match capture::CapturedFrame::capture() {
                    Ok(frame) => {
                        let selected_rect = overlay::run_overlay(&frame);
                        if let Some(rect) = selected_rect {
                            if let Some((rgba, w, h)) = frame.crop_to_rgba(rect) {
                                match storage::save_png(&rgba, w, h) {
                                    Ok((saved_path, _png_bytes)) => {
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
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as usize;
            if id == tray::IDM_QUIT {
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

        let tray_icon = tray::TrayIcon::new(hwnd);
        let (_hotkey, hotkey_result) = hotkey::HotkeyManager::new(hwnd);

        if let Err(err_msg) = hotkey_result {
            tray_icon.show_balloon("Hotkey Conflict", &err_msg);
        }

        let ctx = AppContext {
            tray_icon,
            _hotkey,
        };
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &ctx as *const _ as isize);

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
