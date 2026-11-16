//! 轻量文件日志 + panic 钩子：release 版无控制台，
//! 任何启动失败都必须留下可查的痕迹（写在 exe 同目录，写不了再退回 %TEMP%）。

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn log_file() -> PathBuf {
    static LOG: OnceLock<PathBuf> = OnceLock::new();
    LOG.get_or_init(|| {
        // 候选依次探测「可写」才采用（低完整性令牌下 exe 目录常不可写）
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("puredesk.log"));
            }
        }
        candidates.push(std::env::temp_dir().join("com.puredesk.app").join("puredesk.log"));
        if let Ok(home) = std::env::var("USERPROFILE") {
            candidates.push(PathBuf::from(home).join("puredesk.log"));
        }
        for p in &candidates {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::OpenOptions::new().append(true).create(true).open(p).is_ok() {
                return p.clone();
            }
        }
        candidates
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("puredesk.log"))
    })
    .clone()
}

pub fn log_line(msg: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(log_file())
    {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_line(&format!("PANIC: {info}"));
        default_hook(info);
    }));
}
