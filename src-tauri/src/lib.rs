//! PureDesk —— 轻量级 Windows 桌面整理收纳工具。
//!
//! 设计原则（对标“元气桌面整理”的核心能力，去掉一切商业化负担）：
//! - 一键整理：按扩展名/文件夹规则把桌面文件移入 `桌面\桌面整理\<分类>`；
//! - 收纳盒：每个分类一个无边框透明小窗，永久钉在桌面层（普通窗口之下）；
//! - 自动整理：监听桌面目录，新文件自动归位；
//! - 纯本地：无账号、无广告、无后台上报，配置就是一个 JSON。

mod boxwin;
mod commands;
mod config;
#[cfg(windows)]
mod droptarget;
mod icons;
mod logging;
mod organize;
mod scan;
mod state;
mod watcher;
mod winutil;

use state::AppState;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub fn run() {
    logging::install_panic_hook();
    logging::log_line("app starting");
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let id = shortcut.to_string();
                        match id.as_str() {
                            // Ctrl+Alt+G：一键整理
                            "Ctrl+Alt+G" | "ctrl+alt+g" => {
                                let app = app.clone();
                                std::thread::spawn(move || {
                                    let Some(state) = app.try_state::<AppState>() else {
                                        return;
                                    };
                                    let cfg = state.config_read().clone();
                                    let guard = state.move_guard();
                                    let report = organize::organize_desktop(&cfg, None);
                                    drop(guard);
                                    crate::refresh_storage_visibility(&app);
                                    let _ = app.emit("organize-report", &report);
                                    let _ = app.emit("scan-changed", ());
                                    let _ = app.emit("boxes-changed", ());
                                });
                            }
                            // Ctrl+Alt+D：显示/隐藏主面板
                            "Ctrl+Alt+D" | "ctrl+alt+d" => {
                                if let Some(w) = app.get_webview_window("main") {
                                    if w.is_visible().unwrap_or(false) {
                                        let _ = w.hide();
                                    } else {
                                        show_main(app);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                })
                .build(),
        )
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::scan_desktop,
            commands::log_event,
            commands::get_runtime_info,
            commands::delete_to_trash,
            commands::rename_item,
            commands::organize_desktop,
            commands::organize_category,
            commands::create_box,
            commands::delete_box,
            commands::organize_paths,
            commands::restore_paths,
            commands::open_path,
            commands::reveal_path,
            commands::get_icons,
            commands::get_config,
            commands::save_config,
            commands::reset_box_layout,
            commands::rebuild_boxes,
            commands::show_box,
            commands::hide_box,
            commands::show_main_panel,
            commands::set_box_collapsed,
            commands::set_box_size,
            commands::set_box_icon_size,
            commands::set_autostart,
            commands::get_autostart,
            commands::hide_main,
            commands::quit_app,
        ])
        .on_window_event(on_window_event)
        .setup(|app| {
            let handle = app.handle().clone();
            // 首启保护：创建收纳盒目录会触发监听事件，
            // 抑制数秒内的自动整理，避免“一打开就搬光桌面”的惊吓。
            app.state::<AppState>().suppress_auto_organize(8);
            // 单盒模型迁移（一次性，必须在创建盒子窗口之前）：
            // 预设分类停用、旧分类内容并入收纳盒
            {
                let state = app.state::<AppState>();
                let mut cfg = state.config_write();
                let n = organize::migrate_single_box_files(&mut cfg);
                drop(cfg);
                if n > 0 {
                    state.persist_config().ok();
                    logging::log_line(&format!("migration merged {n} items"));
                }
            }
            logging::log_line("setup: begin");
            create_main_window(&handle)?;
            logging::log_line("setup: main window ok");
            setup_tray(&handle)?;
            logging::log_line("setup: tray ok");
            watcher::spawn(handle.clone());
            spawn_geom_flusher(handle.clone());
            // 全局快捷键
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt as _;
                let gs = handle.global_shortcut();
                if let Err(e) = gs.register("ctrl+alt+g") {
                    logging::log_line(&format!("register ctrl+alt+g failed: {e}"));
                }
                if let Err(e) = gs.register("ctrl+alt+d") {
                    logging::log_line(&format!("register ctrl+alt+d failed: {e}"));
                }
            }
            // 盒子窗口创建失败（如 WebView2 初始化异常）不应拖垮整个应用：
            // 主面板与托盘保持可用，盒子可后续通过「显示收纳盒 / 重置布局」重建
            if let Err(e) = boxwin::sync(&handle) {
                logging::log_line(&format!("setup: boxwin sync failed (recoverable): {e}"));
            }
            logging::log_line("setup: boxes ok");
            refresh_storage_visibility(&handle);
            // 透明盒子的 Chrome 子窗口出现较晚，延后重新注入拖放目标
            #[cfg(windows)]
            droptarget::schedule_injection(handle.clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building puredesk");
    app.run(|_handle, event| {
        if let tauri::RunEvent::Exit = event {
            // 进程结束兜底：无论运行中是否选了「隐藏收纳文件夹」，退出时都恢复可见——
            // 收纳区里的快捷方式/文件必须始终能直接从桌面使用，软件关了桌面不能残废
            let cfg = config::load();
            let storage = scan::resolve_storage_root(&cfg);
            if storage.is_dir() && winutil::set_hidden(&storage, false) {
                logging::log_line("exit: storage folder restored to visible");
            }
        }
    });
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// WebView2 用户数据目录：优先便携模式环境变量，其次 %LOCALAPPDATA%，
/// 再降级到 exe 同目录 / %TEMP%——任何环境下都不至于无声崩溃。
/// 注意：目录必须实测「可写」才算可用（目录已存在时 create_dir_all 直接成功，
/// 但低完整性令牌进程无法写入 %LOCALAPPDATA%）。
fn probe_writable(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".write-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new().create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn resolve_webview_data_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    if let Ok(p) = std::env::var("PUREDESK_DATA_DIR") {
        let p = PathBuf::from(p.trim());
        if !p.as_os_str().is_empty() && probe_writable(&p) {
            logging::log_line(&format!("webview data dir (env): {}", p.display()));
            return p;
        }
    }
    let primary = app
        .path()
        .app_local_data_dir()
        .map(|d| d.join("EBWebView"))
        .unwrap_or_else(|_| PathBuf::from("EBWebView"));
    if probe_writable(&primary) {
        logging::log_line(&format!("webview data dir: {}", primary.display()));
        return primary;
    }
    let fallback = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default()
        .join("webview-data");
    if !fallback.as_os_str().is_empty() && probe_writable(&fallback) {
        logging::log_line(&format!(
            "webview data dir (exe fallback): {}",
            fallback.display()
        ));
        return fallback;
    }
    let temp = std::env::temp_dir().join("com.puredesk.app").join("EBWebView");
    let _ = std::fs::create_dir_all(&temp);
    logging::log_line(&format!("webview data dir (temp fallback): {}", temp.display()));
    temp
}

/// WebView2 数据目录/资源在旧实例退出后有短暂释放延迟，
/// 快速重启时创建会报“资源占用”→ 自动重试直至成功或超时。
const WEBVIEW_RETRY: usize = 12;
const WEBVIEW_RETRY_DELAY_MS: u64 = 600;

fn create_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let dir = resolve_webview_data_dir(app);
    let mut last_err: Option<tauri::Error> = None;
    for attempt in 1..=WEBVIEW_RETRY {
        let builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
            .title("PureDesk · 桌面收纳")
            .inner_size(960.0, 700.0)
            .min_inner_size(800.0, 560.0)
            .center()
            .resizable(true)
            .decorations(false)
            .transparent(true)
            .visible(true)
            .data_directory(dir.clone());
        match builder.build() {
            Ok(_) => {
                if attempt > 1 {
                    logging::log_line(&format!("main window created on attempt {attempt}"));
                }
                return Ok(());
            }
            Err(e) => {
                logging::log_line(&format!(
                    "main window create attempt {attempt}/{WEBVIEW_RETRY} failed: {e}"
                ));
                last_err = Some(e);
                std::thread::sleep(Duration::from_millis(WEBVIEW_RETRY_DELAY_MS));
            }
        }
    }
    Err(last_err
        .map(|e| e.to_string())
        .unwrap_or_else(|| "window create retries exhausted".to_string()))
}

fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主面板", true, None::<&str>)?;
    let organize_now = MenuItem::with_id(app, "organize", "立即整理桌面", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 PureDesk", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &organize_now, &quit])?;

    TrayIconBuilder::with_id("puredesk-tray")
        .icon(app.default_window_icon().expect("missing app icon").clone())
        .tooltip("PureDesk · 桌面收纳")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "organize" => {
                let app = app.clone();
                std::thread::spawn(move || {
                    let Some(state) = app.try_state::<AppState>() else {
                        return;
                    };
                    let cfg = state.config_read().clone();
                    let guard = state.move_guard();
                    let report = organize::organize_desktop(&cfg, None);
                    drop(guard);
                    crate::refresh_storage_visibility(&app);
                    let _ = app.emit("organize-report", &report);
                    let _ = app.emit("scan-changed", ());
                    let _ = app.emit("boxes-changed", ());
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 拖动结束后把盒子窗口重新钉回底层（代次计数避免拖动中途打架）
fn schedule_pin(app: tauri::AppHandle, id: String) {
    let gen = {
        app.state::<AppState>()
            .pin_gen
            .fetch_add(1, Ordering::Relaxed)
            + 1
    };
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(320));
        // 应用退出后 state 已释放，直接结束线程（防闪退）
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if state.pin_gen.load(Ordering::Relaxed) == gen {
            if let Some(w) = app.get_webview_window(&boxwin::label_of(&id)) {
                let _ = winutil::pin_to_bottom(&w);
            }
        }
    });
}

fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let label = window.label().to_string();
    let is_box = label.starts_with("box-");
    if !is_box && label != "main" {
        return;
    }
    match event {
        WindowEvent::Moved(pos) => {
            if !is_box {
                return;
            }
            let id = label.trim_start_matches("box-").to_string();
            let scale = window.scale_factor().unwrap_or(1.0);
            let app = window.app_handle();
            let state = app.state::<AppState>();
            {
                let mut cfg = state.config_write();
                if let Some(b) = cfg.boxes.iter_mut().find(|b| b.id == id) {
                    b.x = pos.x as f64 / scale;
                    b.y = pos.y as f64 / scale;
                }
            }
            state.geom_dirty.store(true, Ordering::Relaxed);
            schedule_pin(app.clone(), id);
        }
        WindowEvent::Resized(size) => {
            if !is_box {
                return;
            }
            // 用户拖拽窗口边缘/右下角手柄调整尺寸 → 持久化宽高（逻辑像素）。
            // 收起态窗口高度恒为 COLLAPSED_H，此时不能把展开高度写坏 → 只更新 w。
            let id = label.trim_start_matches("box-").to_string();
            let scale = window.scale_factor().unwrap_or(1.0);
            let app = window.app_handle();
            let state = app.state::<AppState>();
            {
                let mut cfg = state.config_write();
                if let Some(b) = cfg.boxes.iter_mut().find(|b| b.id == id) {
                    b.w = (size.width as f64 / scale).round();
                    if !b.collapsed {
                        b.h = (size.height as f64 / scale).round();
                    }
                }
            }
            state.geom_dirty.store(true, Ordering::Relaxed);
        }
        WindowEvent::Focused(focused) => {
            if *focused && is_box {
                // 点击盒子后短暂浮起，随即回到桌面层
                let app = window.app_handle().clone();
                let lbl = label.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(160));
                    if let Some(w) = app.get_webview_window(&lbl) {
                        let _ = winutil::pin_to_bottom(&w);
                    }
                });
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            if is_box {
                // 盒子：关闭=隐藏（小组件常驻）
                api.prevent_close();
                let _ = window.hide();
                let id = label.trim_start_matches("box-").to_string();
                let app = window.app_handle();
                let state = app.state::<AppState>();
                if let Some(b) = state.config_write().boxes.iter_mut().find(|b| b.id == id) {
                    b.visible = false;
                }
                state.persist_config().ok();
                let _ = app.emit("scan-changed", ());
            } else {
                // 主面板：按用户设置的关闭行为（最小化到托盘 / 结束进程）
                let exit = window
                    .app_handle()
                    .try_state::<AppState>()
                    .map(|s| s.config_read().close_action == "exit")
                    .unwrap_or(false);
                if exit {
                    logging::log_line("main window closed with exit action, bye");
                    window.app_handle().exit(0);
                } else {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        }
        _ => {}
    }
}

/// 按设置隐藏/显示桌面上的收纳文件夹（一键整理后桌面真正干净）
pub fn refresh_storage_visibility(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let cfg = state.config_read().clone();
    let storage = scan::resolve_storage_root(&cfg);
    if !storage.exists() {
        return;
    }
    if winutil::set_hidden(&storage, cfg.hide_storage_folder) {
        logging::log_line(&format!("storage folder hidden={}", cfg.hide_storage_folder));
    } else {
        logging::log_line("storage folder visibility change failed (permission?)");
    }
}

/// 盒子拖动时几何信息高频变化，由后台线程低频落盘
fn spawn_geom_flusher(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(700));
        // 应用退出（state 释放）后结束线程（防闪退）
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        state.flush_if_dirty();
    });
}
