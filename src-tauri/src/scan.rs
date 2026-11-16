//! 桌面扫描与分类规则：用户桌面 + 公共桌面（浅层），以及各收纳盒内容快照。

use crate::config::Config;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopItem {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub ext: String,
    /// 命中的分类 id；None 表示未命中任何启用的分类（保持原样）
    pub category: Option<String>,
    pub size: u64,
    pub mtime_ms: u64,
    /// 来自公共桌面（C:\Users\Public\Desktop）
    pub public: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoxSnapshot {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub color: String,
    pub count: usize,
    pub items: Vec<DesktopItem>,
    /// 用户自定义盒（纯手动策展）
    pub custom: bool,
    /// 内置预设分类（不可删除；删除判定与 custom 相反）
    pub built_in: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanPayload {
    pub items: Vec<DesktopItem>,
    pub boxes: Vec<BoxSnapshot>,
    pub storage_root: String,
    pub desktop_dir: String,
}

pub fn desktop_dir() -> PathBuf {
    crate::winutil::desktop_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Desktop"))
}

pub fn public_desktop_dir() -> Option<PathBuf> {
    crate::winutil::public_desktop_dir()
}

/// 解析收纳存储根目录
pub fn resolve_storage_root(cfg: &Config) -> PathBuf {
    match &cfg.storage_root {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
        _ => desktop_dir().join(crate::config::DEFAULT_STORAGE_DIR_NAME),
    }
}

pub fn ext_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// 按分类规则（先精确后兜底）判定分类
pub fn classify(cfg: &Config, is_dir: bool, ext: &str) -> Option<String> {
    for c in &cfg.categories {
        if c.enabled && !c.is_other && c.matches(is_dir, ext) {
            return Some(c.id.clone());
        }
    }
    for c in &cfg.categories {
        if c.enabled && c.matches_other(is_dir) {
            return Some(c.id.clone());
        }
    }
    None
}

/// 简易通配匹配（* 与 ?），大小写不敏感
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let p = pattern.trim().to_lowercase();
    let n = name.to_lowercase();
    if p.is_empty() {
        return false;
    }
    glob_inner(&p, &n)
}

fn glob_inner(p: &str, n: &str) -> bool {
    if p.is_empty() {
        return n.is_empty();
    }
    if let Some(rest) = p.strip_prefix('*') {
        for i in 0..=n.len() {
            if n.is_char_boundary(i) && glob_inner(rest, &n[i..]) {
                return true;
            }
        }
        false
    } else if let Some(rest) = p.strip_prefix('?') {
        match n.chars().next() {
            Some(ch) => glob_inner(rest, &n[ch.len_utf8()..]),
            None => false,
        }
    } else {
        let ch = p.chars().next().unwrap();
        if n.starts_with(ch) {
            glob_inner(&p[ch.len_utf8()..], &n[ch.len_utf8()..])
        } else {
            false
        }
    }
}

pub fn is_excluded(cfg: &Config, name: &str) -> bool {
    cfg.excludes.iter().any(|pat| glob_match(pat, name))
}

fn is_hidden(md: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        md.file_attributes() & 0x2 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = md;
        false
    }
}

/// 路径相同判断（优先 canonicalize，退化到大小写不敏感的字符串比较）
pub fn same_path(a: &Path, b: &Path) -> bool {
    if let (Ok(x), Ok(y)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        return x == y;
    }
    let norm = |p: &Path| {
        p.to_string_lossy()
            .to_lowercase()
            .trim_end_matches(['\\', '/'])
            .to_string()
    };
    norm(a) == norm(b)
}

fn file_stats(md: &std::fs::Metadata) -> (u64, u64) {
    let size = if md.is_dir() { 0 } else { md.len() };
    let mtime_ms = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    (size, mtime_ms)
}

/// 浅层扫描一个目录（跳过隐藏、排除规则、存储根目录本身）
pub fn scan_dir(dir: &Path, storage_root: &Path, cfg: &Config, public: bool, out: &mut Vec<DesktopItem>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if same_path(&path, storage_root) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if is_excluded(cfg, &name) {
            continue;
        }
        let Ok(md) = entry.metadata() else { continue };
        if is_hidden(&md) {
            continue;
        }
        let is_dir = md.is_dir();
        let ext = if is_dir { String::new() } else { ext_of(&name) };
        let category = classify(cfg, is_dir, &ext);
        let (size, mtime_ms) = file_stats(&md);
        out.push(DesktopItem {
            name,
            path: path.to_string_lossy().to_string(),
            is_dir,
            ext,
            category,
            size,
            mtime_ms,
            public,
        });
    }
}

/// 各收纳盒内容快照
pub fn scan_boxes(cfg: &Config, storage_root: &Path) -> Vec<BoxSnapshot> {
    let mut out = Vec::new();
    for c in &cfg.categories {
        if !(c.enabled && c.show_box) {
            continue;
        }
        let folder = storage_root.join(&c.folder);
        let mut items = Vec::new();
        scan_dir(&folder, storage_root, cfg, false, &mut items);
        items.sort_by_key(|i| i.name.to_lowercase());
        out.push(BoxSnapshot {
            id: c.id.clone(),
            name: c.name.clone(),
            folder: folder.to_string_lossy().to_string(),
            color: c.color.clone(),
            count: items.len(),
            items,
            custom: c.custom,
            built_in: c.built_in,
        });
    }
    out
}

/// 完整扫描：用户桌面 + 公共桌面 + 盒子
pub fn full_scan(cfg: &Config) -> ScanPayload {
    let storage_root = resolve_storage_root(cfg);
    let ddir = desktop_dir();
    let mut items = Vec::new();
    scan_dir(&ddir, &storage_root, cfg, false, &mut items);
    if let Some(pdir) = public_desktop_dir() {
        scan_dir(&pdir, &storage_root, cfg, true, &mut items);
    }
    items.sort_by_key(|i| i.name.to_lowercase());
    ScanPayload {
        items,
        boxes: scan_boxes(cfg, &storage_root),
        storage_root: storage_root.to_string_lossy().to_string(),
        desktop_dir: ddir.to_string_lossy().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_categories, Config};

    fn cfg() -> Config {
        Config {
            categories: default_categories(),
            ..Default::default()
        }
    }

    #[test]
    fn test_classify() {
        let c = cfg();
        assert_eq!(classify(&c, false, "lnk").as_deref(), Some("apps"));
        assert_eq!(classify(&c, false, "EXE").as_deref(), Some("apps"));
        assert_eq!(classify(&c, false, "pdf").as_deref(), Some("docs"));
        assert_eq!(classify(&c, false, "PNG").as_deref(), Some("images"));
        assert_eq!(classify(&c, false, "mp4").as_deref(), Some("videos"));
        assert_eq!(classify(&c, false, "zip").as_deref(), Some("archives"));
        assert_eq!(classify(&c, false, "xyz123").as_deref(), Some("others"));
        assert_eq!(classify(&c, true, "").as_deref(), Some("folders"));
        // 禁用兜底后，未知扩展名留在桌面
        let mut c2 = cfg();
        c2.categories.iter_mut().for_each(|x| {
            if x.is_other {
                x.enabled = false;
            }
        });
        assert_eq!(classify(&c2, false, "xyz123"), None);
    }

    #[test]
    fn test_glob() {
        assert!(glob_match("~$*", "~$abc.docx"));
        assert!(glob_match("*.tmp", "a.TMP"));
        assert!(glob_match("desktop.ini", "Desktop.INI"));
        assert!(glob_match("a?c.txt", "abc.txt"));
        assert!(!glob_match("a?c.txt", "abbc.txt"));
        assert!(!glob_match("*.tmp", "a.txt"));
        assert!(!glob_match("", "anything"));
        // "*" 通配匹配一切非空文件名
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", "a b c"));
    }

    #[test]
    fn test_scan_skips_storage_root_and_excludes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let storage = root.join("桌面整理");
        std::fs::create_dir_all(&storage).unwrap();
        std::fs::write(root.join("a.txt"), "x").unwrap();
        std::fs::write(root.join("desktop.ini"), "x").unwrap();
        std::fs::write(root.join("~$doc.docx"), "x").unwrap();
        std::fs::write(storage.join("b.pdf"), "x").unwrap();

        let c = cfg();
        let mut items = Vec::new();
        scan_dir(root, &storage, &c, false, &mut items);
        let names: Vec<&str> = items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["a.txt"]);
        assert_eq!(items[0].category.as_deref(), Some("docs"));
    }
}
