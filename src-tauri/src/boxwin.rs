//! 收纳盒窗口管理：为每个启用分类创建/同步一个无边框透明置底小窗。

use crate::config::{BoxCfg, Category};
use crate::state::AppState;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const COLLAPSED_H: f64 = 44.0;
const BOX_W: f64 = 216.0;
const BOX_H: f64 = 480.0;
/// 默认摆放：一行最多放几个盒子
const BOX_PER_ROW: usize = 4;

pub fn label_of(id: &str) -> String {
    format!("box-{}", id)
}

/// 依据当前配置计算盒子的默认摆放位置（主屏右侧横向排列，满一行换行）
pub fn default_box_cfg(app: &AppHandle, id: String, idx: usize) -> BoxCfg {
    let (w, h) = (BOX_W, BOX_H);
    let mut x = 120.0 + idx as f64 * 48.0;
    let mut y = 120.0 + idx as f64 * 40.0;
    if let Ok(Some(m)) = app.primary_monitor() {
        let s = m.scale_factor();
        let pos = m.position();
        let size = m.size();
        let right = (pos.x as f64 + size.width as f64) / s;
        let top = pos.y as f64 / s;
        let bottom = (pos.y as f64 + size.height as f64) / s;
        let col = idx % BOX_PER_ROW;
        let row = idx / BOX_PER_ROW;
        x = (right - w - 12.0 - col as f64 * (w + 12.0)).max(8.0);
        y = top + 40.0 + row as f64 * (h + 12.0);
        let max_y = bottom - h - 40.0;
        if y > max_y {
            y = max_y.max(top + 8.0);
        }
    }
    BoxCfg { id, x, y, w, h, collapsed: false, visible: true }
}

fn current_config(app: &AppHandle) -> crate::config::Config {
    app.state::<AppState>().config_read().clone()
}

fn pin(win: &WebviewWindow) {
    let _ = crate::winutil::pin_to_bottom(win);
}

/// 按配置同步盒子窗口：新建缺失的、关闭多余的、对齐可见性
pub fn sync(app: &AppHandle) -> Result<(), String> {
    let cfg = current_config(app);
    let enabled: Vec<Category> = cfg
        .categories
        .iter()
        .filter(|c| c.enabled && c.show_box)
        .cloned()
        .collect();
    let existing = app.webview_windows();

    // 关闭已停用分类的窗口 / 对齐可见性
    for (label, win) in &existing {
        let Some(id) = label.strip_prefix("box-") else { continue };
        let keep = enabled.iter().any(|c| c.id == id);
        if !keep {
            let _ = win.close();
            continue;
        }
        let visible_cfg = cfg.boxes.iter().find(|b| b.id == id).map(|b| b.visible).unwrap_or(true);
        if !visible_cfg {
            let _ = win.hide();
        } else if !win.is_visible().unwrap_or(true) {
            let _ = win.show();
            pin(win);
        }
    }

    // 创建缺失的盒子窗口
    let mut idx = 0usize;
    for c in &enabled {
        let label = label_of(&c.id);
        idx += 1;
        if existing.contains_key(&label) {
            continue;
        }
        let mut new_cfg_pushed = false;
        let box_cfg = match cfg.boxes.iter().find(|b| b.id == c.id) {
            Some(b) => b.clone(),
            None => {
                let d = default_box_cfg(app, c.id.clone(), idx - 1);
                let state = app.state::<AppState>();
                state.config_write().boxes.push(d.clone());
                new_cfg_pushed = true;
                d
            }
        };
        if new_cfg_pushed {
            app.state::<AppState>().persist_config().ok();
        }

        let url = WebviewUrl::App(format!("index.html#/box/{}", c.id).into());
        // WebView2 资源在旧实例退出后有释放延迟 → 创建失败自动重试（防快速重启闪退）
        let mut win: Option<WebviewWindow> = None;
        let mut last_err = String::new();
        for attempt in 1..=12usize {
            let builder = WebviewWindowBuilder::new(app, label.clone(), url.clone())
                .title(c.name.clone())
                .inner_size(box_cfg.w, if box_cfg.collapsed { COLLAPSED_H } else { box_cfg.h })
                .position(box_cfg.x, box_cfg.y)
                .decorations(false)
                .transparent(true)
                .skip_taskbar(true)
                .shadow(false)
                .resizable(true)
                .min_inner_size(150.0, COLLAPSED_H)
                .maximizable(false)
                .minimizable(false)
                .closable(true)
                .focused(false)
                // 拖拽接收默认开启（勿调用 disable_drag_drop_handler）
                .visible(false) // 先隐藏，定位钉底后再显示，避免闪烁
                .data_directory(crate::resolve_webview_data_dir(app));
            match builder.build() {
                Ok(w) => {
                    if attempt > 1 {
                        crate::logging::log_line(&format!(
                            "box window {label} created on attempt {attempt}"
                        ));
                    }
                    win = Some(w);
                    break;
                }
                Err(e) => {
                    last_err = format!("attempt {attempt}: {e}");
                    crate::logging::log_line(&format!(
                        "box window {label} create retry {attempt}: {e}"
                    ));
                    std::thread::sleep(std::time::Duration::from_millis(600));
                }
            }
        }
        let win = win.ok_or(last_err)?;
        if box_cfg.visible {
            let _ = win.show();
            pin(&win);
        }
    }
    // 新建的盒子也需要补注入拖放目标（Chrome 子窗口就绪后）
    #[cfg(windows)]
    crate::droptarget::schedule_injection(app.clone());
    Ok(())
}

/// 设置盒子可见性（不存在配置时补默认几何）
pub fn set_box_visible(app: &AppHandle, id: &str, visible: bool) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut cfgw = state.config_write();
        let cat_ok = cfgw
            .categories
            .iter()
            .any(|c| c.id == id && c.enabled && c.show_box);
        if !cat_ok {
            return Err(format!("分类未启用或不存在: {}", id));
        }
        match cfgw.boxes.iter_mut().find(|b| b.id == id) {
            Some(b) => b.visible = visible,
            None => {
                let idx = cfgw.boxes.len();
                cfgw.boxes.push(BoxCfg { visible, ..default_box_cfg(app, id.to_string(), idx) });
            }
        }
        state.persist_config()?;
    }
    let label = label_of(id);
    if let Some(win) = app.get_webview_window(&label) {
        if visible {
            win.show().map_err(|e| e.to_string())?;
            pin(&win);
        } else {
            win.hide().map_err(|e| e.to_string())?;
        }
    } else if visible {
        sync(app)?;
    }
    let _ = app.emit("scan-changed", ());
    let _ = app.emit("boxes-changed", ());
    Ok(())
}

/// 展开 / 收起盒子（窗口高度同步调整）
pub fn apply_collapsed(app: &AppHandle, id: &str, collapsed: bool) -> Result<(), String> {
    let cfg = current_config(app);
    let b = cfg.boxes.iter().find(|b| b.id == id).ok_or("盒子配置不存在")?;
    if let Some(win) = app.get_webview_window(&label_of(id)) {
        let h = if collapsed { COLLAPSED_H } else { b.h };
        win.set_size(LogicalSize::new(b.w, h)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 调整盒子窗口尺寸（手动拖拽边缘或前端调大小后调用；收起时只记录，展开后生效）
pub fn apply_size(app: &AppHandle, id: &str, w: f64, h: f64) -> Result<(), String> {
    let cfg = current_config(app);
    let b = cfg.boxes.iter().find(|b| b.id == id).ok_or("盒子配置不存在")?;
    if !b.collapsed {
        if let Some(win) = app.get_webview_window(&label_of(id)) {
            win.set_size(LogicalSize::new(w, h)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 重置所有盒子的布局为默认摆放
pub fn reset_layout(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let mut cfgw = state.config_write();
        let enabled: Vec<String> = cfgw
            .categories
            .iter()
            .filter(|c| c.enabled && c.show_box)
            .map(|c| c.id.clone())
            .collect();
        for (i, id) in enabled.iter().enumerate() {
            let d = default_box_cfg(app, id.clone(), i);
            if let Some(b) = cfgw.boxes.iter_mut().find(|b| b.id == *id) {
                b.x = d.x;
                b.y = d.y;
                b.w = d.w;
                b.h = d.h;
            } else {
                cfgw.boxes.push(d);
            }
        }
        state.persist_config()?;
    }
    let cfg = state.config_read().clone();
    for b in &cfg.boxes {
        if let Some(win) = app.get_webview_window(&label_of(&b.id)) {
            let _ = win.set_position(LogicalPosition::new(b.x, b.y));
            let _ = win.set_size(LogicalSize::new(b.w, if b.collapsed { COLLAPSED_H } else { b.h }));
            if b.visible {
                let _ = win.show();
                pin(&win);
            }
        }
    }
    let _ = app.emit("boxes-changed", ());
    Ok(())
}
