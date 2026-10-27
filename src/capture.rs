use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct CapturedFrame {
    pub vx: i32,
    pub vy: i32,
    pub width: i32,
    pub height: i32,
    pub hdc: HDC,
    pub hbitmap: HBITMAP,
    pub old_bitmap: HGDIOBJ,
    pub bits: *mut u8,
}

impl CapturedFrame {
    pub fn capture() -> Result<Self> {
        unsafe {
            let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);

            if width <= 0 || height <= 0 {
                return Err(Error::from_hresult(HRESULT(-1)));
            }

            let hdc_screen = GetDC(None);
            if hdc_screen.is_invalid() {
                return Err(Error::from_thread());
            }

            let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
            if hdc_mem.is_invalid() {
                ReleaseDC(None, hdc_screen);
                return Err(Error::from_thread());
            }

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // negative = top-down DIB
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: 0, // BI_RGB
                    biSizeImage: (width * height * 4) as u32,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
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

            let hbitmap = match hbitmap {
                Ok(b) => b,
                Err(e) => {
                    let _ = DeleteDC(hdc_mem);
                    ReleaseDC(None, hdc_screen);
                    return Err(e);
                }
            };

            let old_bitmap = SelectObject(hdc_mem, HGDIOBJ(hbitmap.0));

            // BitBlt virtual desktop into DIB
            let rop = ROP_CODE(SRCCOPY.0 | CAPTUREBLT.0);
            let blt_res = BitBlt(hdc_mem, 0, 0, width, height, Some(hdc_screen), vx, vy, rop);

            // Release screen DC immediately
            ReleaseDC(None, hdc_screen);

            if let Err(e) = blt_res {
                SelectObject(hdc_mem, old_bitmap);
                let _ = DeleteObject(HGDIOBJ(hbitmap.0));
                let _ = DeleteDC(hdc_mem);
                return Err(e);
            }

            Ok(Self {
                vx,
                vy,
                width,
                height,
                hdc: hdc_mem,
                hbitmap,
                old_bitmap,
                bits: bits_ptr as *mut u8,
            })
        }
    }

    /// Crops the selected virtual screen rectangle to standard RGBA (32-bit: R, G, B, 255).
    /// Rect is in virtual coordinates [left, top, right, bottom].
    pub fn crop_to_rgba(&self, rect: RECT) -> Option<(Vec<u8>, u32, u32)> {
        let left = rect.left.min(rect.right).clamp(self.vx, self.vx + self.width);
        let right = rect.left.max(rect.right).clamp(self.vx, self.vx + self.width);
        let top = rect.top.min(rect.bottom).clamp(self.vy, self.vy + self.height);
        let bottom = rect.top.max(rect.bottom).clamp(self.vy, self.vy + self.height);

        let crop_w = (right - left) as u32;
        let crop_h = (bottom - top) as u32;

        if crop_w == 0 || crop_h == 0 || self.bits.is_null() {
            return None;
        }

        let mut rgba = vec![0u8; (crop_w * crop_h * 4) as usize];
        let offset_x = left - self.vx;
        let offset_y = top - self.vy;

        unsafe {
            for row in 0..crop_h as i32 {
                let src_y = offset_y + row;
                let src_row_start = (src_y * self.width + offset_x) as usize * 4;
                let dst_row_start = (row * (crop_w as i32)) as usize * 4;

                let src_slice = std::slice::from_raw_parts(
                    self.bits.add(src_row_start),
                    (crop_w * 4) as usize,
                );
                let dst_slice = &mut rgba[dst_row_start..dst_row_start + (crop_w * 4) as usize];

                for (src_chunk, dst_chunk) in src_slice.chunks_exact(4).zip(dst_slice.chunks_exact_mut(4)) {
                    // DIB is BGRA. Convert to RGBA and ensure alpha is opaque (255)
                    dst_chunk[0] = src_chunk[2]; // R
                    dst_chunk[1] = src_chunk[1]; // G
                    dst_chunk[2] = src_chunk[0]; // B
                    dst_chunk[3] = 255;          // A
                }
            }
        }

        Some((rgba, crop_w, crop_h))
    }
}

impl Drop for CapturedFrame {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc, self.old_bitmap);
            let _ = DeleteDC(self.hdc);
            let _ = DeleteObject(HGDIOBJ(self.hbitmap.0));
        }
    }
}
