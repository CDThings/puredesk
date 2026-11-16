//! 桌面文件监听：变更后（去抖）触发自动整理，并广播刷新事件。
//! 收纳存储根目录位于桌面之下，自身移动产生的后续事件不会再次搬移
//! （扫描时排除存储根目录），因此不会形成搬移循环。

use crate::organize;
use crate::state::AppState;
use notify::{RecursiveMode, Watcher};
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const QUIET_MS: u64 = 900;

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let dir = crate::scan::desktop_dir();
        let (tx, rx) = mpsc::channel::<Result<notify::Event, notify::Error>>();
        let mut watcher = match notify::recommended_watcher(tx) {
            Ok(w) => w,
            Err(_) => return,
        };
        if watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
            return;
        }
        loop {
            // 等待第一个事件
            if rx.recv().is_err() {
                return;
            }
            // 静默窗口：继续吞掉后续事件，直到安静 QUIET_MS
            loop {
                match rx.recv_timeout(Duration::from_millis(QUIET_MS)) {
                    Ok(_) => continue,
                    Err(mpsc::RecvTimeoutError::Timeout) => break,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            handle_change(&app);
        }
    });
}

fn handle_change(app: &AppHandle) {
    // 应用退出后 state 已释放，结束（防闪退）
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let (auto, cfg) = {
        let cfg = state.config_read().clone();
        (cfg.auto_organize, cfg)
    };
    if auto && !state.auto_organize_suppressed() {
        let _guard = state.move_guard();
        let report = organize::organize_desktop_auto(&cfg);
        for f in &report.failed {
            crate::logging::log_line(&format!("auto-organize failed: {} ({})", f.path, f.error));
        }
        if !report.moved.is_empty() || !report.failed.is_empty() {
            let _ = app.emit("organize-report", &report);
        }
    }
    let _ = app.emit("scan-changed", ());
    let _ = app.emit("boxes-changed", ());
}
