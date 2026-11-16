//! Windows 平台工具：已知文件夹定位、窗口置底（钉在桌面上层、普通窗口下层）。

use std::path::PathBuf;
use tauri::WebviewWindow;

#[cfg(windows)]
pub fn known_folder(folder_id: &windows_sys::core::GUID) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::SHGetKnownFolderPath;

    unsafe {
        let mut p: windows_sys::core::PWSTR = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(folder_id, 0, std::ptr::null_mut(), &mut p);
        if hr == 0 && !p.is_null() {
            let len = (0..).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            CoTaskMemFree(p as *const _ as *mut _);
            Some(PathBuf::from(s))
        } else {
            if !p.is_null() {
                CoTaskMemFree(p as *const _ as *mut _);
            }
            None
        }
    }
}

pub fn desktop_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::FOLDERID_Desktop;
        known_folder(&FOLDERID_Desktop)
    }
    #[cfg(not(windows))]
    {
        dirs::desktop_dir()
    }
}

pub fn public_desktop_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Shell::FOLDERID_PublicDesktop;
        known_folder(&FOLDERID_PublicDesktop)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// 把收纳盒窗口钉到 Z 序最底（壁纸与桌面图标之上，所有普通窗口之下）。
/// 纯装饰性操作，失败无副作用。
pub fn pin_to_bottom(win: &WebviewWindow) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };
        let h = win.hwnd().map_err(|e| e.to_string())?;
        unsafe {
            SetWindowPos(
                h.0 as *mut _,
                HWND_BOTTOM,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = win;
    }
    Ok(())
}

/// 设置文件/文件夹的「隐藏」属性（用于隐藏桌面上的收纳文件夹）。
/// 保留其他属性位，只增删 HIDDEN 位。失败（如低完整性令牌）返回 false。
pub fn set_hidden(path: &std::path::Path, hidden: bool) -> bool {
  #[cfg(windows)]
  {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
      GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN,
    };
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    unsafe {
      let attrs = GetFileAttributesW(wide.as_ptr());
      if attrs == u32::MAX {
        return false;
      }
      let new_attrs = if hidden {
        attrs | FILE_ATTRIBUTE_HIDDEN
      } else {
        attrs & !FILE_ATTRIBUTE_HIDDEN
      };
      SetFileAttributesW(wide.as_ptr(), new_attrs) != 0
    }
  }
  #[cfg(not(windows))]
  {
    let _ = (path, hidden);
    false
  }
}
