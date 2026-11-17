//! Tauri 命令层：前端 invoke 的全部入口。

use crate::boxwin;
use crate::config::{self, Config};
use crate::organize::{self, OrganizeReport};
use crate::scan::{self, ScanPayload};
use crate::state::AppState;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

fn current_config(app: &AppHandle) -> Config {
    app.state::<AppState>().config_read().clone()
}

fn notify_changes(app: &AppHandle) {
    let _ = app.emit("scan-changed", ());
    let _ = app.emit("boxes-changed", ());
}

#[tauri::command]
pub async fn scan_desktop(app: AppHandle) -> Result<ScanPayload, String> {
    let cfg = current_config(&app);
    Ok(scan::full_scan(&cfg))
}

/// 运行环境自检：当前进程完整性级别。
/// SmartScreen 点“仍要运行”启动的进程会继承低完整性令牌（0x1000），
/// 导致无法写入桌面/AppData —— 需要引导用户解决。
#[tauri::command]
pub async fn get_runtime_info() -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, HANDLE},
            Security::{
                GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation,
                TokenIntegrityLevel, TOKEN_QUERY,
            },
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        let integrity = unsafe {
            let mut tok: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok) == 0 {
                0
            } else {
                let mut ret: u32 = 0;
                GetTokenInformation(
                    tok,
                    TokenIntegrityLevel,
                    std::ptr::null_mut(),
                    0,
                    &mut ret,
                );
                let buf = vec![0u8; ret as usize];
                let ok = GetTokenInformation(
                    tok,
                    TokenIntegrityLevel,
                    buf.as_ptr() as *mut _,
                    ret,
                    &mut ret,
                );
                CloseHandle(tok);
                if ok == 0 {
                    0
                } else {
                    let sid: *mut core::ffi::c_void = *(buf.as_ptr() as *const *mut _);
                    if sid.is_null() {
                        0
                    } else {
                        let count = *GetSidSubAuthorityCount(sid) as usize;
                        *GetSidSubAuthority(sid, (count - 1) as u32)
                    }
                }
            }
        };
        // 桌面重定向检测（OneDrive KFM 等网盘接管桌面是整理类软件的最大翻车源）
        let ddir = crate::scan::desktop_dir();
        let redirected = {
            let expect = dirs::home_dir().map(|h| h.join("Desktop"));
            match expect {
                Some(h) => !crate::scan::same_path(&ddir, &h),
                None => false,
            }
        };
        if redirected {
            crate::logging::log_line(&format!(
                "runtime self-check: desktop redirected away from %USERPROFILE%\\Desktop -> {}",
                ddir.display()
            ));
        }
        crate::logging::log_line(&format!(
            "runtime self-check: IL=0x{integrity:X} low={} redirected={redirected}",
            integrity < 0x2000
        ));
        Ok(serde_json::json!({
            "integrity": format!("0x{integrity:X}"),
            "lowIntegrity": integrity < 0x2000,
            "desktopDir": ddir.to_string_lossy(),
            "desktopRedirected": redirected,
        }))
    }
    #[cfg(not(windows))]
    Ok(serde_json::json!({ "integrity": "n/a", "lowIntegrity": false, "desktopDir": "", "desktopRedirected": false }))
}

/// 调试用：前端把拖拽事件写入日志，用于诊断系统级拖放是否到达
#[tauri::command]
pub async fn log_event(message: String) -> Result<(), String> {
    crate::logging::log_line(&message);
    Ok(())
}

#[tauri::command]
pub async fn organize_desktop(app: AppHandle) -> Result<OrganizeReport, String> {
    let cfg = current_config(&app);
    let app_state = app.state::<AppState>();
    let _guard = app_state.move_guard();
    let report = organize::organize_desktop(&cfg, None);
    crate::refresh_storage_visibility(&app);
    notify_changes(&app);
    Ok(report)
}

#[tauri::command]
pub async fn organize_category(app: AppHandle, box_id: String) -> Result<OrganizeReport, String> {
    let cfg = current_config(&app);
    if !cfg.categories.iter().any(|c| c.id == box_id && c.enabled) {
        return Err(format!("分类不存在或未启用: {box_id}"));
    }
    let app_state = app.state::<AppState>();
    let _guard = app_state.move_guard();
    let report = organize::organize_category(&cfg, &box_id);
    notify_changes(&app);
    Ok(report)
}

#[tauri::command]
pub async fn organize_paths(
    app: AppHandle,
    paths: Vec<String>,
    box_id: String,
) -> Result<OrganizeReport, String> {
    let cfg = current_config(&app);
    let app_state = app.state::<AppState>();
    let _guard = app_state.move_guard();
    let report = organize::move_into_box(&cfg, &paths, &box_id);
    notify_changes(&app);
    Ok(report)
}

#[tauri::command]
pub async fn restore_paths(app: AppHandle, paths: Vec<String>) -> Result<OrganizeReport, String> {
    let cfg = current_config(&app);
    // 抑制自动整理，避免刚还原的文件又被搬回盒子
    app.state::<AppState>().suppress_auto_organize(6);
    let app_state = app.state::<AppState>();
    let _guard = app_state.move_guard();
    let report = organize::restore_to_desktop(&cfg, &paths);
    notify_changes(&app);
    Ok(report)
}

/// 一键还原全部：把所有启用盒子里的内容移回桌面根目录（“恢复我原来的桌面”）
#[tauri::command]
pub async fn restore_all(app: AppHandle) -> Result<OrganizeReport, String> {
    let cfg = current_config(&app);
    app.state::<AppState>().suppress_auto_organize(8);
    let app_state = app.state::<AppState>();
    let _guard = app_state.move_guard();
    let report = organize::restore_all_to_desktop(&cfg);
    crate::logging::log_line(&format!(
        "restore_all: moved {} failed {}",
        report.moved.len(),
        report.failed.len()
    ));
    crate::refresh_storage_visibility(&app);
    notify_changes(&app);
    Ok(report)
}

/// 撤销最近一次收纳批次：把上次整理/拖入盒子的文件反向搬回原位
#[tauri::command]
pub async fn undo_last_organize(app: AppHandle) -> Result<OrganizeReport, String> {
    app.state::<AppState>().suppress_auto_organize(8);
    let app_state = app.state::<AppState>();
    let _guard = app_state.move_guard();
    let report = organize::undo_last_batch();
    crate::refresh_storage_visibility(&app);
    notify_changes(&app);
    Ok(report)
}

/// 存储区自检修复：清理“目录被搬进自身子树”产生的嵌套骨架（历史事故自愈）。
/// 返回 (删除的空骨架数, 抢救出的含文件副本数)。
#[tauri::command]
pub async fn repair_storage(app: AppHandle) -> Result<(usize, usize), String> {
    let cfg = current_config(&app);
    let r = organize::cleanup_nested_storage_junk(&cfg);
    notify_changes(&app);
    Ok(r)
}

#[tauri::command]
pub async fn open_path(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reveal_path(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path))
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
    Ok(())
}

#[tauri::command]
pub async fn get_icons(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<HashMap<String, Option<String>>, String> {
    let state = app.state::<AppState>();
    let mut out = HashMap::new();
    for p in paths.iter().take(400) {
        out.insert(p.clone(), state.icons.get(p));
    }
    Ok(out)
}

#[tauri::command]
pub fn get_config(app: AppHandle) -> Result<Config, String> {
    Ok(current_config(&app))
}

fn validate_config(cfg: &Config) -> Result<(), String> {
    if let Some(p) = &cfg.storage_root {
        let p = p.trim();
        if !p.is_empty() && !Path::new(p).is_absolute() {
            return Err("存储路径必须是绝对路径".into());
        }
    }
    let mut ids = std::collections::HashSet::new();
    let mut folders = std::collections::HashSet::new();
    for c in &cfg.categories {
        if !ids.insert(c.id.clone()) {
            return Err(format!("分类 id 重复: {}", c.id));
        }
        if c.name.trim().is_empty() {
            return Err("分类名称不能为空".into());
        }
        if !folders.insert(c.folder.to_lowercase()) {
            return Err(format!("盒子文件夹名重复: {}", c.folder));
        }
    }
    Ok(())
}

const BOX_PALETTE: [&str; 8] = [
    "#e5533d", "#3d7be5", "#39b54a", "#9b59b6", "#f5a623", "#00b8d9", "#ff6b9d", "#7f8c8d",
];

#[tauri::command]
pub async fn create_box(
    app: AppHandle,
    name: String,
    exts: Option<Vec<String>>,
    auto_collect: Option<bool>,
) -> Result<config::Category, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("盒子名称不能为空".into());
    }
    let state = app.state::<AppState>();
    let created = {
        let mut cfgw = state.config_write();
        let mut folder = config::sanitize_folder_name(&name);
        let mut n = 2;
        while cfgw.categories.iter().any(|c| c.folder == folder) {
            folder = format!("{} ({})", config::sanitize_folder_name(&name), n);
            n += 1;
        }
        let id = format!(
            "custom-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let color = BOX_PALETTE[cfgw.categories.iter().filter(|c| c.custom).count() % BOX_PALETTE.len()];
        let mut cat = config::Category::new_custom(&id, &name, &folder, color);
        // 用户选择了扩展名 → 该盒成为自动归类盒；留空 → 纯手动策展盒
        let ext_list: Vec<String> = exts
            .unwrap_or_default()
            .iter()
            .map(|e| e.trim().trim_start_matches('.').to_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        if !ext_list.is_empty() {
            cat.exts = ext_list;
            cat.custom = false;
        }
        cfgw.categories.push(cat.clone());
        cat
    };
    state.persist_config()?;
    boxwin::sync(&app)?;
    notify_changes(&app);
    // 创建后立即收纳桌面上同类文件
    if auto_collect.unwrap_or(false) && !created.exts.is_empty() {
        let cfg = current_config(&app);
        let app_state = app.state::<AppState>();
        let _guard = app_state.move_guard();
        let _ = organize::organize_category(&cfg, &created.id);
    }
    Ok(created)
}

#[tauri::command]
pub async fn delete_box(app: AppHandle, id: String) -> Result<OrganizeReport, String> {
    let cfg = current_config(&app);
    let Some(cat) = cfg.categories.iter().find(|c| c.id == id) else {
        return Err("盒子不存在".into());
    };
    if cat.built_in {
        return Err("内置分类不可删除，可在设置中关闭显示".into());
    }
    // 盒内文件先全部还原到桌面，再删配置
    app.state::<AppState>().suppress_auto_organize(6);
    let report = organize::restore_box_contents(&cfg, &id);
    for f in &report.failed {
        crate::logging::log_line(&format!("delete_box restore failed: {} ({})", f.path, f.error));
    }
    let folder = cat.folder.clone();
    {
        let state = app.state::<AppState>();
        {
            let mut cfgw = state.config_write();
            cfgw.categories.retain(|c| c.id != id);
            cfgw.boxes.retain(|b| b.id != id);
        }
        state.persist_config()?;
    }
    if let Some(w) = app.get_webview_window(&boxwin::label_of(&id)) {
        let _ = w.close();
    }
    // 清掉空文件夹（非空则保留，防误删）
    let dir = scan::resolve_storage_root(&cfg).join(&folder);
    if dir.is_dir() {
        let _ = std::fs::remove_dir(&dir);
    }
    notify_changes(&app);
    Ok(report)
}

#[tauri::command]
pub async fn save_config(app: AppHandle, config: Config) -> Result<(), String> {
    let mut config = config;
    // 规范化分类文件夹名
    for c in config.categories.iter_mut() {
        c.folder = config::sanitize_folder_name(&c.folder);
    }
    validate_config(&config)?;
    // 文件夹改名迁移：盒子重命名后文件跟着走
    {
        let old_cfg = current_config(&app);
        let storage = scan::resolve_storage_root(&old_cfg);
        for c in &config.categories {
            if let Some(o) = old_cfg.categories.iter().find(|x| x.id == c.id) {
                if o.folder != c.folder {
                    let from = storage.join(&o.folder);
                    let to = storage.join(&c.folder);
                    if from.is_dir() && !to.exists() {
                        if let Err(e) = std::fs::rename(&from, &to) {
                            crate::logging::log_line(&format!(
                                "folder rename failed: {} -> {} ({e})",
                                from.display(),
                                to.display()
                            ));
                        }
                    }
                }
            }
        }
    }
    {
        let state = app.state::<AppState>();
        *state.config_write() = config.clone();
        state.persist_config()?;
    }
    boxwin::sync(&app)?;
    crate::refresh_storage_visibility(&app);
    notify_changes(&app);
    Ok(())
}

#[tauri::command]
pub async fn reset_box_layout(app: AppHandle) -> Result<(), String> {
    boxwin::reset_layout(&app)
}

/// 重建盒子窗口（启动时窗口创建失败 / 被关闭后恢复用）
#[tauri::command]
pub async fn rebuild_boxes(app: AppHandle) -> Result<(), String> {
    boxwin::sync(&app)
}

#[tauri::command]
pub async fn show_box(app: AppHandle, id: String) -> Result<(), String> {
    boxwin::set_box_visible(&app, &id, true)
}

#[tauri::command]
pub async fn hide_box(app: AppHandle, id: String) -> Result<(), String> {
    boxwin::set_box_visible(&app, &id, false)
}

#[tauri::command]
pub async fn set_box_collapsed(app: AppHandle, id: String, collapsed: bool) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        {
            let mut cfgw = state.config_write();
            let b = cfgw.boxes.iter_mut().find(|b| b.id == id).ok_or("盒子配置不存在")?;
            b.collapsed = collapsed;
        }
        state.persist_config()?;
    }
    boxwin::apply_collapsed(&app, &id, collapsed)?;
    let _ = app.emit("boxes-changed", ());
    Ok(())
}

/// 持久化盒子窗口的宽高（手动拖拽边缘 / 前端调大小后调用）。
/// w/h 为逻辑像素，由前端从 window.innerWidth/innerHeight 换算。
#[tauri::command]
pub async fn set_box_size(app: AppHandle, id: String, w: f64, h: f64) -> Result<(), String> {
    let clamped_w = w.clamp(150.0, 800.0);
    let clamped_h = h.clamp(44.0, 1200.0);
    {
        let state = app.state::<AppState>();
        {
            let mut cfgw = state.config_write();
            match cfgw.boxes.iter_mut().find(|b| b.id == id) {
                Some(b) => {
                    b.w = clamped_w;
                    b.h = clamped_h;
                }
                None => return Err("盒子配置不存在".into()),
            }
        }
        state.persist_config()?;
    }
    boxwin::apply_size(&app, &id, clamped_w, clamped_h)?;
    let _ = app.emit("boxes-changed", ());
    Ok(())
}

/// 设置全局收纳盒图标基准大小（px）：前端据此设置 CSS 变量 --icon。
/// 0 表示重置为默认 30。
#[tauri::command]
pub async fn set_box_icon_size(app: AppHandle, size: u32) -> Result<(), String> {
    let state = app.state::<AppState>();
    let v = if size == 0 { 30u32 } else { size.clamp(18, 64) };
    {
        let mut cfgw = state.config_write();
        cfgw.icon_size = Some(v);
    }
    state.persist_config()?;
    let _ = app.emit("boxes-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn set_autostart(app: AppHandle, enable: bool) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    if enable {
        al.enable().map_err(|e| e.to_string())?;
    } else {
        al.disable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// 展开/收起收纳岛（前端 hover 驱动；后端按当前尺寸幂等，抖动不会反复 resize）
#[tauri::command]
pub async fn island_expand(app: AppHandle, expanded: bool) -> Result<(), String> {
    crate::islandwin::set_expanded(&app, expanded)
}

/// 显示/隐藏收纳岛（设置页与托盘共用；持久化 enabled 并同步窗口）
#[tauri::command]
pub async fn set_island_visible(app: AppHandle, visible: bool) -> Result<(), String> {
    crate::islandwin::set_visible(&app, visible)?;
    notify_changes(&app);
    Ok(())
}

#[tauri::command]
pub fn hide_main(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        w.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

/// 删除到系统回收站（可随时还原，不做永久删除）
#[tauri::command]
pub async fn delete_to_trash(
    app: AppHandle,
    paths: Vec<String>,
) -> Result<OrganizeReport, String> {
    let cfg = current_config(&app);
    let app_state = app.state::<AppState>();
    let _guard = app_state.move_guard();
    let storage = scan::resolve_storage_root(&cfg);
    let mut report = OrganizeReport::default();
    for p in &paths {
        let src = PathBuf::from(p);
        // 保护：存储根目录与其直接子盒子文件夹不可删除
        if scan::same_path(&src, &storage) {
            report.failed.push(organize::FailedEntry {
                path: p.clone(),
                error: "收纳根目录不能删除".into(),
            });
            continue;
        }
        match trash::delete(src) {
            Ok(()) => report.moved.push(organize::MovedEntry {
                from: p.clone(),
                to: "回收站".into(),
                category: "trash".into(),
            }),
            Err(e) => report.failed.push(organize::FailedEntry {
                path: p.clone(),
                error: e.to_string(),
            }),
        }
    }
    notify_changes(&app);
    Ok(report)
}

/// 显示主面板（盒子「⋮」菜单里「编辑盒子」入口）
#[tauri::command]
pub async fn show_main_panel(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        w.show().map_err(|e| e.to_string())?;
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    Ok(())
}

/// 重命名盒内/桌面条目（同目录内改名，重名校验）
const INVALID_NAME_CHARS: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

#[tauri::command]
pub async fn rename_item(
    app: AppHandle,
    path: String,
    new_name: String,
) -> Result<String, String> {
    let name = new_name.trim();
    if name.is_empty() {
        return Err("名称不能为空".into());
    }
    if name.chars().any(|c| INVALID_NAME_CHARS.contains(&c)) {
        return Err("名称包含非法字符".into());
    }
    let src = PathBuf::from(&path);
    if !src.exists() {
        return Err("文件不存在".into());
    }
    let Some(dir) = src.parent() else {
        return Err("无父目录".into());
    };
    let dst = dir.join(name);
    if dst.exists() {
        return Err(format!("「{name}」已存在"));
    }
    std::fs::rename(&src, &dst).map_err(|e| format!("重命名失败: {e}"))?;
    let _ = app.emit("scan-changed", ());
    let _ = app.emit("boxes-changed", ());
    Ok(dst.to_string_lossy().to_string())
}
