use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::CF_DIBV5;

pub fn write_text(hwnd: Option<HWND>, text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }

    let mut opened = false;
    for _ in 0..10 {
        if unsafe { OpenClipboard(hwnd).is_ok() } {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    if !opened {
        return Err(Error::from_thread());
    }

    unsafe {
        let _ = EmptyClipboard();
        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_len = utf16.len() * std::mem::size_of::<u16>();

        if let Ok(hglobal) = GlobalAlloc(GMEM_MOVEABLE, byte_len) {
            let ptr = GlobalLock(hglobal);
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(utf16.as_ptr() as *const u8, ptr as *mut u8, byte_len);
                let _ = GlobalUnlock(hglobal);
                let _ = SetClipboardData(13, Some(HANDLE(hglobal.0))); // CF_UNICODETEXT = 13
            }
        }
        let _ = CloseClipboard();
    }

    Ok(())
}

pub fn write_image(
    hwnd: Option<HWND>,
    rgba: &[u8],
    width: u32,
    height: u32,
    png_bytes: &[u8],
) -> Result<()> {
    if width == 0 || height == 0 || rgba.is_empty() {
        return Ok(());
    }

    // Retry loop: up to 10 attempts with 10ms delay
    let mut opened = false;
    for _ in 0..10 {
        if unsafe { OpenClipboard(hwnd).is_ok() } {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    if !opened {
        return Err(Error::from_thread());
    }

    unsafe {
        let _ = EmptyClipboard();

        // 1. Write registered "PNG" clipboard format
        let cf_png = RegisterClipboardFormatW(w!("PNG"));
        if cf_png != 0 && !png_bytes.is_empty() {
            if let Ok(hglobal) = GlobalAlloc(GMEM_MOVEABLE, png_bytes.len()) {
                let ptr = GlobalLock(hglobal);
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(
                        png_bytes.as_ptr(),
                        ptr as *mut u8,
                        png_bytes.len(),
                    );
                    let _ = GlobalUnlock(hglobal);
                    let _ = SetClipboardData(cf_png, Some(HANDLE(hglobal.0)));
                }
            }
        }

        // 2. Write CF_DIBV5 format (bottom-up BGRA with sRGB color space)
        let header_size = std::mem::size_of::<BITMAPV5HEADER>();
        let image_size = (width * height * 4) as usize;
        let total_size = header_size + image_size;

        if let Ok(hglobal) = GlobalAlloc(GMEM_MOVEABLE, total_size) {
            let ptr = GlobalLock(hglobal);
            if !ptr.is_null() {
                let header = BITMAPV5HEADER {
                    bV5Size: header_size as u32,
                    bV5Width: width as i32,
                    bV5Height: height as i32, // positive = bottom-up DIB
                    bV5Planes: 1,
                    bV5BitCount: 32,
                    bV5Compression: BI_BITFIELDS,
                    bV5SizeImage: image_size as u32,
                    bV5RedMask: 0x00FF0000,
                    bV5GreenMask: 0x0000FF00,
                    bV5BlueMask: 0x000000FF,
                    bV5AlphaMask: 0xFF000000,
                    bV5CSType: 0x73524742, // 'sRGB'
                    bV5Intent: 4,          // LCS_GM_IMAGES
                    ..Default::default()
                };

                std::ptr::copy_nonoverlapping(
                    &header as *const _ as *const u8,
                    ptr as *mut u8,
                    header_size,
                );

                let dst_pixels = (ptr as *mut u8).add(header_size);
                for row in 0..height {
                    // bottom-up: row 0 in DIB is bottom row of the image
                    let src_y = height - 1 - row;
                    let src_offset = (src_y * width * 4) as usize;
                    let dst_offset = (row * width * 4) as usize;

                    let src_slice = &rgba[src_offset..src_offset + (width * 4) as usize];
                    let dst_slice = std::slice::from_raw_parts_mut(
                        dst_pixels.add(dst_offset),
                        (width * 4) as usize,
                    );

                    for (src_px, dst_px) in src_slice.chunks_exact(4).zip(dst_slice.chunks_exact_mut(4)) {
                        dst_px[0] = src_px[2]; // B
                        dst_px[1] = src_px[1]; // G
                        dst_px[2] = src_px[0]; // R
                        dst_px[3] = src_px[3]; // A
                    }
                }

                let _ = GlobalUnlock(hglobal);
                let _ = SetClipboardData(CF_DIBV5.0 as u32, Some(HANDLE(hglobal.0)));
            }
        }

        let _ = CloseClipboard();
    }

    Ok(())
}
