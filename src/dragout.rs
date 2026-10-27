use std::path::Path;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::*;
use windows_core::*;

#[windows_core::implement(IDataObject)]
pub struct FileDataObject {
    hdrop_bytes: Vec<u8>,
    text_bytes: Vec<u8>,
    formats: Vec<FORMATETC>,
}

impl FileDataObject {
    pub fn new(file_path: &Path) -> Self {
        let abs_path = std::fs::canonicalize(file_path).unwrap_or_else(|_| file_path.to_path_buf());
        let mut path_str = abs_path.to_string_lossy().to_string();
        // Remove UNC prefix \\?\ if added by canonicalize
        if path_str.starts_with(r"\\?\") {
            path_str = path_str[4..].to_string();
        }

        let mut path_w: Vec<u16> = path_str.encode_utf16().collect();
        path_w.push(0); // single null terminator
        path_w.push(0); // double null terminator for DROPFILES list

        let dropfiles_header = DROPFILES {
            pFiles: std::mem::size_of::<DROPFILES>() as u32,
            pt: POINT { x: 0, y: 0 },
            fNC: BOOL(0),
            fWide: BOOL(1),
        };

        let mut hdrop_bytes = Vec::with_capacity(
            std::mem::size_of::<DROPFILES>() + path_w.len() * std::mem::size_of::<u16>(),
        );

        unsafe {
            let header_slice = std::slice::from_raw_parts(
                &dropfiles_header as *const _ as *const u8,
                std::mem::size_of::<DROPFILES>(),
            );
            hdrop_bytes.extend_from_slice(header_slice);

            let path_slice = std::slice::from_raw_parts(
                path_w.as_ptr() as *const u8,
                path_w.len() * std::mem::size_of::<u16>(),
            );
            hdrop_bytes.extend_from_slice(path_slice);
        }

        // CF_UNICODETEXT buffer (single null-terminated)
        let mut text_w: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
        let text_bytes = unsafe {
            std::slice::from_raw_parts(
                text_w.as_mut_ptr() as *const u8,
                text_w.len() * std::mem::size_of::<u16>(),
            )
            .to_vec()
        };

        let formats = vec![
            FORMATETC {
                cfFormat: 15, // CF_HDROP
                ptd: std::ptr::null_mut(),
                dwAspect: 1, // DVASPECT_CONTENT
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32,
            },
            FORMATETC {
                cfFormat: 13, // CF_UNICODETEXT
                ptd: std::ptr::null_mut(),
                dwAspect: 1,
                lindex: -1,
                tymed: TYMED_HGLOBAL.0 as u32,
            },
        ];

        Self {
            hdrop_bytes,
            text_bytes,
            formats,
        }
    }
}

impl IDataObject_Impl for FileDataObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> Result<STGMEDIUM> {
        unsafe {
            if pformatetcin.is_null() {
                return Err(Error::from_hresult(DV_E_FORMATETC));
            }
            let fmt = &*pformatetcin;

            if fmt.cfFormat == 15 {
                // CF_HDROP
                let hglobal = GlobalAlloc(GMEM_MOVEABLE, self.hdrop_bytes.len())?;
                let ptr = GlobalLock(hglobal);
                if ptr.is_null() {
                    return Err(Error::from_thread());
                }
                std::ptr::copy_nonoverlapping(
                    self.hdrop_bytes.as_ptr(),
                    ptr as *mut u8,
                    self.hdrop_bytes.len(),
                );
                let _ = GlobalUnlock(hglobal);

                let mut stg = STGMEDIUM::default();
                stg.tymed = TYMED_HGLOBAL.0 as u32;
                stg.u.hGlobal = hglobal;
                stg.pUnkForRelease = std::mem::ManuallyDrop::new(None);
                Ok(stg)
            } else if fmt.cfFormat == 13 {
                // CF_UNICODETEXT
                let hglobal = GlobalAlloc(GMEM_MOVEABLE, self.text_bytes.len())?;
                let ptr = GlobalLock(hglobal);
                if ptr.is_null() {
                    return Err(Error::from_thread());
                }
                std::ptr::copy_nonoverlapping(
                    self.text_bytes.as_ptr(),
                    ptr as *mut u8,
                    self.text_bytes.len(),
                );
                let _ = GlobalUnlock(hglobal);

                let mut stg = STGMEDIUM::default();
                stg.tymed = TYMED_HGLOBAL.0 as u32;
                stg.u.hGlobal = hglobal;
                stg.pUnkForRelease = std::mem::ManuallyDrop::new(None);
                Ok(stg)
            } else {
                Err(Error::from_hresult(DV_E_FORMATETC))
            }
        }
    }

    fn GetDataHere(&self, _pformatetc: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> Result<()> {
        Err(Error::from_hresult(DV_E_FORMATETC))
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        unsafe {
            if pformatetc.is_null() {
                return DV_E_FORMATETC;
            }
            let fmt = &*pformatetc;
            if (fmt.cfFormat == 15 || fmt.cfFormat == 13) && (fmt.tymed & (TYMED_HGLOBAL.0 as u32) != 0) {
                S_OK
            } else {
                DV_E_FORMATETC
            }
        }
    }

    fn GetCanonicalFormatEtc(&self, _pformatectin: *const FORMATETC, _pformatetcout: *mut FORMATETC) -> HRESULT {
        DATA_S_SAMEFORMATETC
    }

    fn SetData(&self, _pformatetc: *const FORMATETC, _pmedium: *const STGMEDIUM, _frelease: BOOL) -> Result<()> {
        Ok(())
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> Result<IEnumFORMATETC> {
        if dwdirection == 1 {
            // DATADIR_GET
            unsafe { SHCreateStdEnumFmtEtc(&self.formats) }
        } else {
            Err(Error::from_hresult(E_NOTIMPL))
        }
    }

    fn DAdvise(&self, _pformatetc: *const FORMATETC, _advf: u32, _padvsink: Ref<IAdviseSink>) -> Result<u32> {
        Err(Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }

    fn DUnadvise(&self, _dwconnection: u32) -> Result<()> {
        Err(Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(Error::from_hresult(OLE_E_ADVISENOTSUPPORTED))
    }
}

#[windows_core::implement(IDropSource)]
pub struct FileDropSource;

impl IDropSource_Impl for FileDropSource_Impl {
    fn QueryContinueDrag(
        &self,
        fescapepressed: BOOL,
        grfkeystate: MODIFIERKEYS_FLAGS,
    ) -> HRESULT {
        if fescapepressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if (grfkeystate.0 & 0x0001) == 0 {
            // MK_LBUTTON (0x0001) released: drop!
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

pub fn execute_drag_drop(file_path: &Path, hbitmap: HBITMAP, width: i32, height: i32) {
    unsafe {
        let data_obj: IDataObject = ComObject::new(FileDataObject::new(file_path)).into_interface();
        let drop_source: IDropSource = ComObject::new(FileDropSource).into_interface();

        // Attach thumbnail drag image via Shell IDragSourceHelper
        let helper: Result<IDragSourceHelper> = CoCreateInstance(
            &CLSID_DragDropHelper,
            None,
            CLSCTX_INPROC_SERVER,
        );

        if let Ok(helper) = helper {
            let shdi = SHDRAGIMAGE {
                sizeDragImage: SIZE { cx: width, cy: height },
                ptOffset: POINT { x: width / 2, y: height / 2 },
                hbmpDragImage: hbitmap,
                crColorKey: COLORREF(0),
            };
            let _ = helper.InitializeFromBitmap(&shdi, &data_obj);
        }

        let mut pdw_effect = DROPEFFECT(0);
        let _ = DoDragDrop(&data_obj, &drop_source, DROPEFFECT_COPY, &mut pdw_effect);
    }
}
