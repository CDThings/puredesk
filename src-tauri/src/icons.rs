//! 文件图标提取：调用 Shell 拿到文件关联图标并编码为 PNG data-url。
//! 结果按路径缓存（.lnk 会解析出目标图标）。

use base64::Engine;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct IconCache {
    map: Mutex<HashMap<String, Option<String>>>,
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// 取图标 data-url，失败返回 None（前端回退为扩展名徽章）
    pub fn get(&self, path: &str) -> Option<String> {
        let mut map = self.map.lock().unwrap();
        if let Some(v) = map.get(path) {
            return v.clone();
        }
        let v = load_icon_data_url(path);
        if map.len() > 4096 {
            map.clear();
        }
        map.insert(path.to_string(), v.clone());
        v
    }
}

impl Default for IconCache {
    fn default() -> Self {
        Self::new()
    }
}

pub fn load_icon_data_url(path: &str) -> Option<String> {
    #[cfg(windows)]
    {
        extract_shell_icon(path)
            .map(|png| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

#[cfg(windows)]
fn extract_shell_icon(path: &str) -> Option<Vec<u8>> {
    use windows_sys::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo};

    unsafe {
        let mut wpath: Vec<u16> = path.encode_utf16().collect();
        wpath.push(0);
        let mut sfi: SHFILEINFOW = std::mem::zeroed();
        let ok = SHGetFileInfoW(
            wpath.as_ptr(),
            0,
            &mut sfi,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || sfi.hIcon.is_null() {
            return None;
        }
        let hicon = sfi.hIcon;
        let result = (|| {
            let mut ii: windows_sys::Win32::UI::WindowsAndMessaging::ICONINFO = std::mem::zeroed();
            if GetIconInfo(hicon, &mut ii) == 0 {
                return None;
            }
            let out = render_iconinfo(&ii);
            if !ii.hbmMask.is_null() {
                windows_sys::Win32::Graphics::Gdi::DeleteObject(ii.hbmMask as *mut _);
            }
            if !ii.hbmColor.is_null() {
                windows_sys::Win32::Graphics::Gdi::DeleteObject(ii.hbmColor as *mut _);
            }
            out
        })();
        DestroyIcon(hicon);
        result
    }
}

/// 把 ICONINFO 中的颜色位图读出为 PNG（BGRA→RGBA，兼容无 alpha 的老图标）
#[cfg(windows)]
unsafe fn render_iconinfo(
    ii: &windows_sys::Win32::UI::WindowsAndMessaging::ICONINFO,
) -> Option<Vec<u8>> {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };

    let hbmp = ii.hbmColor;
    if hbmp.is_null() {
        return None;
    }
    let mut bm: BITMAP = std::mem::zeroed();
    if GetObjectW(
        hbmp as *mut _,
        std::mem::size_of::<BITMAP>() as i32,
        &mut bm as *mut _ as *mut _,
    ) == 0
    {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    if w <= 0 || h <= 0 || w > 256 || h > 256 {
        return None;
    }

    let hdc_screen = GetDC(std::ptr::null_mut());
    let hdc_mem = CreateCompatibleDC(hdc_screen);
    let mut bi: BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h; // 自顶向下
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    let lines = GetDIBits(
        hdc_mem,
        hbmp as *mut _,
        0,
        h as u32,
        buf.as_mut_ptr() as *mut _,
        &mut bi,
        DIB_RGB_COLORS,
    );
    DeleteDC(hdc_mem);
    ReleaseDC(std::ptr::null_mut(), hdc_screen);
    if lines == 0 {
        return None;
    }

    let mut all_zero_alpha = true;
    for px in buf.as_chunks_mut::<4>().0 {
        px.swap(0, 2); // BGRA -> RGBA
        if px[3] != 0 {
            all_zero_alpha = false;
        }
    }
    if all_zero_alpha {
        for px in buf.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
    }

    let img = image::RgbaImage::from_raw(w as u32, h as u32, buf)?;
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}
