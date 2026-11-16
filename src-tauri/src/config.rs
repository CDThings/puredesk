//! 应用配置：分类规则、排除规则、收纳盒几何信息等。
//! 持久化到 `%APPDATA%\com.puredesk.app\config.json`。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const APP_ID: &str = "com.puredesk.app";
/// 桌面上的收纳根目录名（未自定义存储位置时使用）
pub const DEFAULT_STORAGE_DIR_NAME: &str = "桌面整理";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Category {
    pub id: String,
    pub name: String,
    /// 盒子对应的子文件夹名（位于存储根目录下）
    pub folder: String,
    pub color: String,
    pub exts: Vec<String>,
    /// 是否收纳文件夹（match_dir 时忽略 exts）
    pub match_dir: bool,
    /// 兜底分类（未匹配任何规则且为文件时归入此类）
    pub is_other: bool,
    pub enabled: bool,
    /// 是否在桌面显示收纳盒窗口
    pub show_box: bool,
    /// 用户自定义盒：不参与自动分类，纯手动策展（拖拽/移入）
    #[serde(default)]
    pub custom: bool,
    /// 内置预设分类（不可删除；用户创建的盒子一律可删）
    #[serde(default)]
    pub built_in: bool,
}

impl Category {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: &str,
        name: &str,
        folder: &str,
        color: &str,
        exts: &[&str],
        match_dir: bool,
        is_other: bool,
        show_box: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            folder: folder.into(),
            color: color.into(),
            exts: exts.iter().map(|s| s.to_string()).collect(),
            match_dir,
            is_other,
            enabled: true,
            show_box,
            custom: false,
            built_in: true,
        }
    }

    /// 用户自定义盒：无后缀规则，不参与自动归类
    pub fn new_custom(id: &str, name: &str, folder: &str, color: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            folder: folder.into(),
            color: color.into(),
            exts: vec![],
            match_dir: false,
            is_other: false,
            enabled: true,
            show_box: true,
            custom: true,
            built_in: false,
        }
    }

    /// 精确匹配（不含兜底分类）
    pub fn matches(&self, is_dir: bool, ext: &str) -> bool {
        if self.match_dir {
            return is_dir;
        }
        !self.is_other && !is_dir && self.exts.iter().any(|e| e.eq_ignore_ascii_case(ext))
    }

    /// 兜底匹配（收编一切未匹配项，含文件夹——单盒模型的基础）
    pub fn matches_other(&self, _is_dir: bool) -> bool {
        self.is_other
    }
}

/// 收纳盒窗口的几何与显示状态（逻辑像素）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BoxCfg {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub collapsed: bool,
    pub visible: bool,
}

impl Default for BoxCfg {
    fn default() -> Self {
        Self {
            id: String::new(),
            x: 120.0,
            y: 120.0,
            w: 232.0,
            h: 304.0,
            collapsed: false,
            visible: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// 收纳存储根目录；None 表示 桌面\桌面整理
    pub storage_root: Option<String>,
    pub auto_organize: bool,
    pub organize_public_desktop: bool,
    /// 主面板关闭按钮行为：minimize=缩到托盘，exit=结束进程
    pub close_action: String,
    /// 隐藏桌面上的收纳文件夹（一键整理后桌面真正干净）。
    /// 默认 false：关软件后收纳区仍需直接可用；退出时后端也会自动恢复可见
    #[serde(default)]
    pub hide_storage_folder: bool,
    /// 单盒模型迁移标记（v0.2.1：默认只显示一个「收纳盒」）
    #[serde(default)]
    pub migrated_single_box: bool,
    /// 收纳盒图标基准大小（px）；None=默认 30。前端据此设置 CSS 变量 --icon
    #[serde(default)]
    pub icon_size: Option<u32>,
    pub categories: Vec<Category>,
    /// 排除规则（支持 * 与 ? 通配，匹配文件名，不区分大小写）
    pub excludes: Vec<String>,
    pub boxes: Vec<BoxCfg>,
}

/// 默认排除规则：系统文件 + 常见"正在下载"的半成品扩展名
pub const DEFAULT_EXCLUDES: [&str; 8] = [
    "desktop.ini",
    "Thumbs.db",
    ".DS_Store",
    "~$*",
    "*.tmp",
    "*.crdownload",
    "*.part",
    "*.download",
];

impl Default for Config {
    fn default() -> Self {
        Self {
            storage_root: None,
            // 默认关闭：避免首次使用就自动搬动全部桌面文件；主面板有醒目开关
            auto_organize: false,
            organize_public_desktop: false,
            close_action: "minimize".to_string(),
            // 默认保持收纳区可见：关软件后桌面快捷方式仍可直接使用（退出时后端兜底恢复可见）
            hide_storage_folder: false,
            migrated_single_box: true,
            icon_size: None,
            // 单盒模型：只显示一个兜底「收纳盒」，预设规则保留但默认停用。
            // 内置预设（含收纳盒）标记 built_in=true → 不可删除（防误删）；
            // 用户新建盒子一律 built_in=false → 可删除（文件自动还原到桌面）。
            categories: {
                let mut cats = default_categories();
                for c in cats.iter_mut() {
                    c.enabled = false;
                    c.show_box = false;
                }
                let mut inbox = Category::new_custom("inbox", "收纳盒", "收纳盒", "#5b8cff");
                inbox.is_other = true;
                inbox.built_in = true; // 兜底盒视为内置，不可删（可停用/隐藏）
                cats.insert(0, inbox);
                cats
            },
            excludes: DEFAULT_EXCLUDES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            boxes: Vec::new(),
        }
    }
}

pub fn default_categories() -> Vec<Category> {
    vec![
        Category::new("apps", "应用", "应用", "#5b8cff", &["lnk", "exe", "url", "msi", "bat", "cmd", "scr", "appref-ms"], false, false, true),
        Category::new("docs", "文档", "文档", "#34c77b", &["doc", "docx", "pdf", "txt", "md", "xls", "xlsx", "ppt", "pptx", "csv", "wps", "et", "dps", "rtf", "odt", "ods", "odp", "xmind", "epub", "mobi", "chm", "log", "json", "xml"], false, false, true),
        Category::new("images", "图片", "图片", "#f5a623", &["png", "jpg", "jpeg", "gif", "bmp", "webp", "svg", "ico", "tif", "tiff", "psd", "ai", "heic", "heif", "raw", "avif", "cr2", "nef"], false, false, true),
        Category::new("videos", "视频", "视频", "#ff5f5f", &["mp4", "mkv", "avi", "mov", "wmv", "flv", "rmvb", "m4v", "webm", "mpg", "mpeg", "3gp", "ts", "vob"], false, false, true),
        Category::new("audio", "音频", "音频", "#a86bff", &["mp3", "wav", "flac", "aac", "ogg", "m4a", "wma", "ape", "mid", "opus"], false, false, true),
        Category::new("archives", "压缩包", "压缩包", "#ff9f43", &["zip", "rar", "7z", "tar", "gz", "bz2", "xz", "iso", "cab", "jar", "gho", "zst", "lz4"], false, false, true),
        Category::new("folders", "文件夹", "文件夹", "#00b8d9", &[], true, false, true),
        Category::new("others", "其他", "其他", "#8a97a6", &[], false, true, true),
    ]
}

pub fn config_path() -> PathBuf {
    static RESOLVED: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            // 便携模式：PUREDESK_CONFIG 可指定配置文件路径
            if let Ok(p) = std::env::var("PUREDESK_CONFIG") {
                let p = p.trim();
                if !p.is_empty() {
                    return PathBuf::from(p);
                }
            }
            let base = std::env::var("APPDATA")
                .map(PathBuf::from)
                .ok()
                .or_else(dirs::config_dir)
                .unwrap_or_else(|| PathBuf::from("."));
            let primary_dir = base.join(APP_ID);
            if std::fs::create_dir_all(&primary_dir).is_ok() {
                return primary_dir.join("config.json");
            }
            // %APPDATA% 不可写（受限环境）：降级到 exe 同目录
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    let fallback = dir.join("puredesk-config.json");
                    if std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&fallback)
                        .is_ok()
                    {
                        crate::logging::log_line(&format!(
                            "config fallback (exe dir): {}",
                            fallback.display()
                        ));
                        return fallback;
                    }
                }
            }
            primary_dir.join("config.json")
        })
        .clone()
}

pub fn load() -> Config {
    match std::fs::read_to_string(config_path()) {
        Ok(s) => {
            let mut cfg: Config = serde_json::from_str(&s).unwrap_or_default();
            // 合并新增的默认排除项（老配置升级时自动获得新规则）
            for d in DEFAULT_EXCLUDES {
                if !cfg.excludes.iter().any(|e| e.eq_ignore_ascii_case(d)) {
                    cfg.excludes.push(d.to_string());
                }
            }
            // 关闭行为只允许两个值
            if cfg.close_action != "exit" {
                cfg.close_action = "minimize".to_string();
            }
            // 老配置缺 built_in 标记：按 id 补齐。
            // 内置预设（默认分类）与「收纳盒」（v0.2.1 单盒模型）视为不可删；
            // 自定义盒（custom- 前缀或任意其他 id）可删。
            let presets: &[&str] = &[
                "inbox", "apps", "docs", "images", "videos", "audio", "archives", "folders",
                "others",
            ];
            for c in &mut cfg.categories {
                c.built_in = presets.contains(&c.id.as_str());
            }
            cfg
        }
        Err(_) => Config::default(),
    }
}

pub fn save(cfg: &Config) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let s = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(&p, s).map_err(|e| e.to_string())
}

/// 清理文件夹名中的非法字符（Windows 文件名限制）
pub fn sanitize_folder_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "_".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_roundtrip() {
        let cfg = Config::default();
        let s = serde_json::to_string(&cfg).unwrap();
        let back: Config = serde_json::from_str(&s).unwrap();
        assert_eq!(back.categories.len(), cfg.categories.len());
        assert_eq!(back.categories[0].id, "inbox"); // 单盒模型：兜底收纳盒在前
    }

    #[test]
    fn test_old_config_missing_fields_get_defaults() {
        let s = r#"{"autoOrganize": false}"#;
        let cfg: Config = serde_json::from_str(s).unwrap();
        assert!(!cfg.auto_organize);
        assert_eq!(cfg.categories.len(), 9); // inbox + 8 预设
    }

    #[test]
    fn test_sanitize() {
        assert_eq!(sanitize_folder_name("a/b:c"), "a_b_c");
        assert_eq!(sanitize_folder_name("  .. "), "_");
        assert_eq!(sanitize_folder_name("文档"), "文档");
        // 尾部点号会被 Windows 拒绝，须修剪
        assert_eq!(sanitize_folder_name("report."), "report");
        assert_eq!(sanitize_folder_name("a.."), "a");
    }
}
