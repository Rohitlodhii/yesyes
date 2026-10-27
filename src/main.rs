#![allow(unsafe_op_in_unsafe_fn)]

mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

static RUNNING: AtomicBool = AtomicBool::new(true);

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
                let tray_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const tray::TrayIcon;
                if !tray_ptr.is_null() {
                    (*tray_ptr).show_menu(hwnd);
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
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &tray_icon as *const _ as isize);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        drop(tray_icon);
        let _ = CloseHandle(_mutex);
        OleUninitialize();
    }

    Ok(())
}
