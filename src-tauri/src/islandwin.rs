//! 收纳岛：屏幕顶部居中的灵动岛式状态胶囊（借鉴开源项目「岛仔」的理念——
//! 信息轻轻出现在屏幕顶部，平时收起，鼠标靠近展开）。
//! 与收纳盒（钉底、普通窗口之下）相反：岛常驻置顶，展示 桌面\桌面整理 收纳区
//! 的全局状态；展开/收起由前端 hover 驱动，窗口尺寸同步调整（顶边与中心 x 锚定）。

use crate::state::AppState;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const LABEL: &str = "island";
/// 收起态胶囊尺寸（逻辑像素）
pub const ISLAND_W: f64 = 190.0;
pub const ISLAND_H: f64 = 40.0;
/// 展开态尺寸（面板向下延伸，顶边与中心 x 不动）
pub const ISLAND_W_EXP: f64 = 340.0;
pub const ISLAND_H_EXP: f64 = 340.0;
/// 距屏幕顶部的偏移（逻辑像素）
const TOP_OFFSET: f64 = 8.0;
/// 窗口左右最小留白（防 clamp 后贴边压到内容）
const EDGE_GAP: f64 = 4.0;

/// 展开窗口重锚定：顶边不动、中心 x 对齐原胶囊中心
fn anchor_x(base_x: f64, base_w: f64, new_w: f64) -> f64 {
    base_x + (base_w - new_w) / 2.0
}

/// 越界保护：把窗口左上角 x 夹回屏幕内（多屏拓扑/分辨率变化后的兜底）
fn clamp_x(x: f64, w: f64, monitor_w: f64) -> f64 {
    if monitor_w <= w + EDGE_GAP * 2.0 {
        return (monitor_w - w).max(EDGE_GAP);
    }
    x.clamp(EDGE_GAP, monitor_w - w - EDGE_GAP)
}

/// 主屏逻辑宽度；取不到时返回 0（此时跳过 clamp，保持原值）
fn monitor_logical_width(app: &AppHandle) -> f64 {
    app.primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.size().width as f64 / m.scale_factor())
        .unwrap_or(0.0)
}

fn island_cfg(state: &AppState) -> crate::config::IslandCfg {
    state.config_read().island.clone()
}

/// 按配置同步收纳岛窗口：启用且不存在则创建（顶部居中），停用则隐藏。
/// 失败由调用方决定是否容错（setup 中仅记日志，不拖垮主面板与盒子）。
pub fn sync(app: &AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let cfg = island_cfg(&state);
    let existing = app.get_webview_window(LABEL);
    if !cfg.enabled {
        if let Some(w) = existing {
            let _ = w.hide();
        }
        return Ok(());
    }
    if existing.is_some() {
        return Ok(());
    }

    // 位置：配置未定位（0,0 哨兵）时按主屏顶部居中；已有配置则沿用并 clamp 防跑出屏幕
    let mon_w = monitor_logical_width(app);
    let (mut x, y) = if cfg.x == 0.0 && cfg.y == 0.0 {
        ((mon_w - ISLAND_W) / 2.0, TOP_OFFSET)
    } else {
        (cfg.x, cfg.y)
    };
    if mon_w > 0.0 {
        x = clamp_x(x, ISLAND_W, mon_w);
    }

    // WebView2 资源在旧实例退出后有释放延迟 → 创建失败自动重试（与盒子窗口同一策略）
    let mut win: Option<WebviewWindow> = None;
    let mut last_err = String::new();
    for attempt in 1..=crate::WEBVIEW_RETRY {
        let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html#/island".into()))
            .title("PureDesk 收纳岛")
            .inner_size(ISLAND_W, ISLAND_H)
            .position(x, y)
            .decorations(false)
            .transparent(true)
            .skip_taskbar(true)
            .shadow(false)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .closable(false)
            .focused(false)
            .always_on_top(true)
            .visible(false) // 先隐藏，落位后再显示，避免闪烁
            .data_directory(crate::resolve_webview_data_dir(app));
        match builder.build() {
            Ok(w) => {
                if attempt > 1 {
                    crate::logging::log_line(&format!("island window created on attempt {attempt}"));
                }
                win = Some(w);
                break;
            }
            Err(e) => {
                last_err = format!("attempt {attempt}: {e}");
                crate::logging::log_line(&format!("island window create retry {attempt}: {e}"));
                std::thread::sleep(std::time::Duration::from_millis(crate::WEBVIEW_RETRY_DELAY_MS));
            }
        }
    }
    let win = win.ok_or_else(|| format!("island window create failed: {last_err}"))?;

    // 首次定位后落盘，重启沿用（分辨率变化由 sync 的 clamp 兜底）
    if let Some(state) = app.try_state::<AppState>() {
        let mut cfgw = state.config_write();
        if cfgw.island.x == 0.0 && cfgw.island.y == 0.0 {
            cfgw.island.x = x;
            cfgw.island.y = y;
            drop(cfgw);
            state.persist_config().ok();
        }
    }

    let _ = win.show();
    Ok(())
}

/// 展开 / 收起（窗口尺寸同步调整；顶边不动、中心 x 锚定胶囊中心）。
/// 幂等：已是目标尺寸时直接返回，前端 hover 抖动不会造成反复 resize。
pub fn set_expanded(app: &AppHandle, expanded: bool) -> Result<(), String> {
    let Some(win) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    let (w, h) = if expanded { (ISLAND_W_EXP, ISLAND_H_EXP) } else { (ISLAND_W, ISLAND_H) };
    let scale = win.scale_factor().unwrap_or(1.0);
    let cur = win.outer_size().map_err(|e| e.to_string())?;
    if ((cur.height as f64 / scale) - h).abs() < 1.0 {
        return Ok(());
    }
    // 以当前窗口实际位置重锚：顶边不动、中心 x 对齐（兼容窗口位置被系统微调的情况）
    let pos = win.outer_position().map_err(|e| e.to_string())?;
    let base_x = pos.x as f64 / scale;
    let base_w = cur.width as f64 / scale;
    let cur_y = pos.y as f64 / scale;
    let mon_w = monitor_logical_width(app);
    let anchored = anchor_x(base_x, base_w, w);
    let nx = if mon_w > 0.0 { clamp_x(anchored, w, mon_w) } else { anchored };
    win.set_size(LogicalSize::new(w, h)).map_err(|e| e.to_string())?;
    win.set_position(LogicalPosition::new(nx, cur_y)).map_err(|e| e.to_string())?;
    Ok(())
}

/// 显示/隐藏收纳岛（设置页与托盘共用；持久化到 config.island.enabled）
pub fn set_visible(app: &AppHandle, visible: bool) -> Result<(), String> {
    {
        let Some(state) = app.try_state::<AppState>() else {
            return Ok(());
        };
        let mut cfgw = state.config_write();
        cfgw.island.enabled = visible;
        drop(cfgw);
        state.persist_config()?;
    }
    if visible {
        match app.get_webview_window(LABEL) {
            Some(w) => {
                let _ = w.show();
            }
            None => sync(app)?,
        }
    } else if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_anchor_x_keeps_center() {
        // 收起(190) ↔ 展开(340)：中心对齐，顶边不动
        assert!((anchor_x(100.0, ISLAND_W, ISLAND_W_EXP) - (100.0 + (ISLAND_W - ISLAND_W_EXP) / 2.0)).abs() < 1e-9);
        assert!((anchor_x(100.0, ISLAND_W, ISLAND_W) - 100.0).abs() < 1e-9);
        let cx = anchor_x(100.0, ISLAND_W, ISLAND_W_EXP) + ISLAND_W_EXP / 2.0;
        assert!((cx - (100.0 + ISLAND_W / 2.0)).abs() < 1e-9);
    }

    #[test]
    fn test_clamp_x_bounds() {
        let mw = 1920.0;
        assert!((clamp_x(-50.0, ISLAND_W, mw) - EDGE_GAP).abs() < 1e-9);
        assert!((clamp_x(5000.0, ISLAND_W, mw) - (mw - ISLAND_W - EDGE_GAP)).abs() < 1e-9);
        assert!((clamp_x(865.0, ISLAND_W, mw) - 865.0).abs() < 1e-9);
        // 异常小的屏幕：贴左但不为负
        assert!((clamp_x(10.0, 300.0, 200.0) - 4.0).abs() < 1e-9);
    }

    #[test]
    fn test_expand_within_monitor_wide_enough() {
        // 1920 宽屏幕上展开 340 窗口：中心不变的往返
        let mon = 1920.0;
        let base_x = (mon - ISLAND_W) / 2.0;
        let exp_x = clamp_x(anchor_x(base_x, ISLAND_W, ISLAND_W_EXP), ISLAND_W_EXP, mon);
        assert!((exp_x + ISLAND_W_EXP / 2.0 - (base_x + ISLAND_W / 2.0)).abs() < 1e-9);
        // 极窄屏幕（宽度恰够展开）：clamp 后仍在屏内
        let exp_x2 = clamp_x(anchor_x(-10.0, ISLAND_W, ISLAND_W_EXP), ISLAND_W_EXP, 400.0);
        assert!(exp_x2 >= EDGE_GAP && exp_x2 + ISLAND_W_EXP <= 400.0);
    }
}
