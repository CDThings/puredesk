//! 收纳盒的兜底 OLE 拖放目标。
//!
//! wry 在创建 WebView2 时向其子窗口注入拖放目标，但透明盒子的
//! Chrome_WidgetWin_1 子窗口出现较晚，注入时会扑空（主窗口不受影响）。
//! 因此在窗口稳定后，由这里重新注册自建的 IDropTarget，
//! 把拖入文件路径直接 emit 给对应盒子窗口。

use tauri::{AppHandle, Emitter, Manager};
use crate::state::AppState;
use windows::{
  core::implement,
  Win32::{
    Foundation::{HWND, LPARAM, POINTL},
    System::{
      Com::{FORMATETC, IDataObject, DVASPECT_CONTENT, TYMED_HGLOBAL},
      Ole::{
        CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, IDropTarget, IDropTarget_Impl, RegisterDragDrop,
        ReleaseStgMedium, RevokeDragDrop,
      },
      SystemServices::MODIFIERKEYS_FLAGS,
    },
    UI::{
      Shell::{DragQueryFileW, HDROP},
      WindowsAndMessaging::EnumChildWindows,
    },
  },
};

#[implement(IDropTarget)]
struct BoxDropTarget {
  label: String,
  app: AppHandle,
}

impl BoxDropTarget {
  fn extract_paths(&self, data_obj: &IDataObject) -> Option<Vec<String>> {
    unsafe {
      let fmt = FORMATETC {
        cfFormat: CF_HDROP.0,
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        ptd: std::ptr::null_mut(),
        tymed: TYMED_HGLOBAL.0 as u32,
      };
      let medium = match data_obj.GetData(&fmt) {
        Ok(m) => m,
        Err(e) => {
          crate::logging::log_line(&format!("GetData(CF_HDROP) failed: {e}"));
          return None;
        }
      };
      let hdrop = HDROP(medium.u.hGlobal.0);
      let count = DragQueryFileW(hdrop, u32::MAX, None);
      crate::logging::log_line(&format!("CF_HDROP files: {count}"));
      let mut paths = Vec::with_capacity(count as usize);
      for i in 0..count {
        let len = DragQueryFileW(hdrop, i, None) as usize;
        if len == 0 {
          continue;
        }
        let mut buf = vec![0u16; len + 1];
        // 传整个缓冲区（cch 要含 null 的空间，否则最后一个字符会被截断）
        DragQueryFileW(hdrop, i, Some(&mut buf));
        paths.push(
          String::from_utf16_lossy(&buf[..len])
            .trim_end_matches('\0')
            .to_string(),
        );
      }
      ReleaseStgMedium(&medium as *const _ as *mut _);
      for p in &paths {
        crate::logging::log_line(&format!("dropped: {p}"));
      }
      if paths.is_empty() { None } else { Some(paths) }
    }
  }
}

impl IDropTarget_Impl for BoxDropTarget_Impl {
  fn DragEnter(
    &self,
    _pdataobj: windows_core::Ref<'_, IDataObject>,
    _grfkeystate: MODIFIERKEYS_FLAGS,
    pt: &POINTL,
    pdweffect: *mut DROPEFFECT,
  ) -> windows_core::Result<()> {
    crate::logging::log_line(&format!("OLE DragEnter on {} at {},{}", self.label, pt.x, pt.y));
    unsafe { *pdweffect = DROPEFFECT_COPY };
    Ok(())
  }

  fn DragOver(
    &self,
    _grfkeystate: MODIFIERKEYS_FLAGS,
    _pt: &POINTL,
    pdweffect: *mut DROPEFFECT,
  ) -> windows_core::Result<()> {
    unsafe { *pdweffect = DROPEFFECT_COPY };
    Ok(())
  }

  fn DragLeave(&self) -> windows_core::Result<()> {
    crate::logging::log_line(&format!("OLE DragLeave on {}", self.label));
    Ok(())
  }

  fn Drop(
    &self,
    pdataobj: windows_core::Ref<'_, IDataObject>,
    _grfkeystate: MODIFIERKEYS_FLAGS,
    _pt: &POINTL,
    pdweffect: *mut DROPEFFECT,
  ) -> windows_core::Result<()> {
    crate::logging::log_line(&format!("OLE Drop on {}", self.label));
    unsafe { *pdweffect = DROPEFFECT_COPY };
    if let Ok(obj) = pdataobj.ok() {
      if let Some(paths) = self.extract_paths(obj) {
        // 直接在 Rust 侧完成收纳（不经前端事件，避免投递不稳定），
        // 完成后广播 boxes-changed 让各窗口刷新。
        // COM 回调内禁止 panic 外溢，整体 catch_unwind 防闪退。
        let label = self.label.clone();
        let app = self.app.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
          let box_id = label.trim_start_matches("box-").to_string();
          let Some(state) = app.try_state::<AppState>() else {
            return;
          };
          let cfg = state.config_read().clone();
          let _guard = state.move_guard();
          let report = crate::organize::move_into_box(&cfg, &paths, &box_id);
          for f in &report.failed {
            crate::logging::log_line(&format!(
              "drop move failed: {} ({})",
              f.path, f.error
            ));
          }
          if !report.moved.is_empty() {
            crate::logging::log_line(&format!("dropped {} 项 -> {}", report.moved.len(), box_id));
            let _ = app.emit("scan-changed", ());
            let _ = app.emit("boxes-changed", ());
          }
        }));
        if result.is_err() {
          crate::logging::log_line("OLE Drop handler panicked (caught)");
        }
      }
    }
    Ok(())
  }
}

/// 在指定顶层窗口的 WebView2 全部子窗口上注册我们的拖放目标
/// （命中测试可能落在任意一个子窗口上，须全覆盖）
fn inject(hwnd: HWND, label: &str, app: &AppHandle) -> bool {
  struct Ctx<'a> {
    label: &'a str,
    app: &'a AppHandle,
    injected: bool,
  }
  let mut ctx = Ctx { label, app, injected: false };
  let ctx_ptr = LPARAM(&mut ctx as *mut Ctx as isize);
  unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    unsafe {
      let ctx = &mut *(lparam.0 as *mut Ctx);
      let target: IDropTarget =
        BoxDropTarget { label: ctx.label.to_string(), app: ctx.app.clone() }.into();
      let _ = RevokeDragDrop(hwnd);
      if RegisterDragDrop(hwnd, &target).is_ok() {
        ctx.injected = true;
      }
      true.into() // 继续枚举所有子窗口
    }
  }
  unsafe {
    let _ = EnumChildWindows(Some(hwnd), Some(cb), ctx_ptr);
  }
  ctx.injected
}

/// 给所有收纳盒窗口注入（多轮延迟执行：透明盒子的 Chrome 子窗口
/// 出现时机不定，多轮重试保证最终全部覆盖）
pub fn schedule_injection(app: AppHandle) {
  std::thread::spawn(move || {
    // 1.5s / +2.5s / +5s 三轮补注
    for delay in [1500u64, 2500, 5000] {
      std::thread::sleep(std::time::Duration::from_millis(delay));
      let app2 = app.clone();
      let _ = app.run_on_main_thread(move || {
        let mut count = 0;
        let windows = app2.webview_windows();
        for (label, win) in windows {
          if !label.starts_with("box-") {
            continue;
          }
          if let Ok(h) = win.hwnd() {
            let h = HWND(h.0);
            if inject(h, &label, &app2) {
              count += 1;
            }
          }
        }
        if count > 0 {
          crate::logging::log_line(&format!("drop targets injected: {count}"));
        } else {
          crate::logging::log_line("drop targets injection found no windows");
        }
      });
    }
  });
}
