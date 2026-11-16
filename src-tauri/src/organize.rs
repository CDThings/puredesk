//! 整理引擎：一键整理、移入盒子、还原到桌面。
//! 所有移动都做冲突改名（“名字 (2).ext”），失败逐条记录、互不影响。
//! 核心函数接受显式目录参数以便测试；对外包装使用真实桌面。

use crate::config::Config;
use crate::scan::{self, DesktopItem};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MovedEntry {
    pub from: String,
    pub to: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedEntry {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeReport {
    pub moved: Vec<MovedEntry>,
    pub failed: Vec<FailedEntry>,
    pub skipped: usize,
}

/// 目标目录内的不冲突路径：“name.ext” → “name (2).ext” …
pub fn unique_destination(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let ext = Path::new(name)
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();
    for i in 2..10000 {
        let cand = dir.join(format!("{} ({}){}", stem, i, ext));
        if !cand.exists() {
            return cand;
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    dir.join(format!("{} ({}){}", stem, stamp, ext))
}

/// 移动文件/目录：优先 rename（失败重试 3 次，瞬时占用常会自行解除），
/// 跨盘时退化为复制+删除；源被占用时清理掉半成品副本，避免重复。
pub fn move_path(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录失败 {}: {}", parent.display(), e))?;
    }
    let mut last_err = String::new();
    for _ in 0..3 {
        match std::fs::rename(src, dst) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = e.to_string();
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }
    // rename 失败后的兜底：仅在目标不存在时做复制+删除（防止覆盖目标已有的同名项，
    // 跨盘或占用解除失败时宁可报错也不能误伤已存在的文件）
    if dst.exists() {
        return Err(format!("目标已存在且无法移动: {last_err}"));
    }
    if src.is_dir() {
        if let Err(e) = copy_dir_recursive(src, dst) {
            return Err(format!("复制目录失败: {e}"));
        }
        if std::fs::remove_dir_all(src).is_err() {
            let _ = std::fs::remove_dir_all(dst);
            return Err(format!("源目录被占用: {last_err}"));
        }
    } else {
        if std::fs::copy(src, dst).is_err() {
            return Err(format!("文件被占用: {last_err}"));
        }
        if std::fs::remove_file(src).is_err() {
            let _ = std::fs::remove_file(dst);
            return Err(format!("源文件被占用: {last_err}"));
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let t = entry.path();
        let d = dst.join(entry.file_name());
        if t.is_dir() {
            copy_dir_recursive(&t, &d)?;
        } else {
            std::fs::copy(&t, &d).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// path 是否位于 dir 之内
pub fn inside_of(path: &Path, dir: &Path) -> bool {
    if let (Ok(p), Ok(d)) = (std::fs::canonicalize(path), std::fs::canonicalize(dir)) {
        return p.starts_with(&d);
    }
    let pn = path.to_string_lossy().to_lowercase();
    let dn = dir.to_string_lossy().to_lowercase();
    pn.starts_with(&dn)
}

fn category_folder(cfg: &Config, category_id: &str) -> Option<PathBuf> {
    let c = cfg
        .categories
        .iter()
        .find(|c| c.id == category_id && c.enabled)?;
    let storage = scan::resolve_storage_root(cfg);
    Some(storage.join(&c.folder))
}

/// 核心：把指定路径移入指定分类的盒子。
/// 来源限制：桌面目录内的文件，或收纳区内的文件（盒子之间移动）。
pub fn move_into_box_in(
    cfg: &Config,
    desktop: &Path,
    paths: &[String],
    box_id: &str,
) -> OrganizeReport {
    let mut report = OrganizeReport::default();
    let Some(dest_dir) = category_folder(cfg, box_id) else {
        report.failed.push(FailedEntry {
            path: String::new(),
            error: format!("目标盒子不存在或未启用: {}", box_id),
        });
        return report;
    };
    let storage = scan::resolve_storage_root(cfg);

    for p in paths {
        let src = PathBuf::from(p);
        if !src.exists() {
            report.failed.push(FailedEntry { path: p.clone(), error: "文件不存在".into() });
            continue;
        }
        // 保护：绝不移动存储根目录本身（它是所有盒子的容器）
        if scan::same_path(&src, &storage) {
            report.failed.push(FailedEntry {
                path: p.clone(),
                error: "收纳根目录不能被收纳".into(),
            });
            continue;
        }
        if inside_of(&src, &storage) {
            // 已在收纳区：允许在盒子之间移动
            if let Some(parent) = src.parent() {
                if scan::same_path(parent, &dest_dir) {
                    report.skipped += 1;
                    continue;
                }
            }
        } else if src
            .parent()
            .map(|pp| !scan::same_path(pp, desktop))
            .unwrap_or(true)
        {
            report
                .failed
                .push(FailedEntry { path: p.clone(), error: "仅支持整理桌面上的文件".into() });
            continue;
        }
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            report.skipped += 1;
            continue;
        }
        let dst = unique_destination(&dest_dir, &name);
        match move_path(&src, &dst) {
            Ok(()) => report.moved.push(MovedEntry {
                from: p.clone(),
                to: dst.to_string_lossy().to_string(),
                category: box_id.into(),
            }),
            Err(e) => report.failed.push(FailedEntry { path: p.clone(), error: e }),
        }
    }
    report
}

/// 对外包装：真实桌面
pub fn move_into_box(cfg: &Config, paths: &[String], box_id: &str) -> OrganizeReport {
    move_into_box_in(cfg, &scan::desktop_dir(), paths, box_id)
}

/// 核心：一键整理指定桌面目录
/// - `category`：只整理该分类（收纳同类）
/// - `skip_recent_ms`：跳过最近 N 毫秒内修改过的文件（正在写入，下一轮再收）
pub fn organize_dir_filtered(
    cfg: &Config,
    desktop: &Path,
    only: Option<&[String]>,
    category: Option<&str>,
    skip_recent_ms: Option<u64>,
) -> OrganizeReport {
    let mut report = OrganizeReport::default();
    let storage = scan::resolve_storage_root(cfg);

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let mut items: Vec<DesktopItem> = Vec::new();
    scan::scan_dir(desktop, &storage, cfg, false, &mut items);

    let mut done = HashSet::new();
    for item in items {
        if let Some(only) = only {
            if !only.iter().any(|p| p == &item.path) {
                continue;
            }
        }
        if let (Some(cat), Some(item_cat)) = (category, &item.category) {
            if cat != item_cat {
                continue;
            }
        }
        if !done.insert(item.path.clone()) {
            continue;
        }
        if let Some(window) = skip_recent_ms {
            if item.mtime_ms > 0 && now_ms.saturating_sub(item.mtime_ms) < window {
                report.skipped += 1;
                continue;
            }
        }
        let Some(cat) = item.category.clone() else {
            report.skipped += 1;
            continue;
        };
        let Some(dest_dir) = category_folder(cfg, &cat) else {
            report.skipped += 1;
            continue;
        };
        let src = PathBuf::from(&item.path);
        if !src.exists() {
            report.skipped += 1;
            continue;
        }
        let dst = unique_destination(&dest_dir, &item.name);
        match move_path(&src, &dst) {
            Ok(()) => report.moved.push(MovedEntry {
                from: item.path.clone(),
                to: dst.to_string_lossy().to_string(),
                category: cat,
            }),
            Err(e) => {
                crate::logging::log_line(&format!("move failed: {} -> {}", item.path, e));
                report
                    .failed
                    .push(FailedEntry { path: item.path.clone(), error: e });
            }
        }
    }
    report
}

/// 兼容包装：全部整理（无分类过滤、无保护期——手动整理强制执行）
pub fn organize_dir(cfg: &Config, desktop: &Path, only: Option<&[String]>) -> OrganizeReport {
    organize_dir_filtered(cfg, desktop, only, None, None)
}

/// 收纳同类：把桌面上属于指定分类的文件全部移入该盒
pub fn organize_category(cfg: &Config, category_id: &str) -> OrganizeReport {
    let mut report =
        organize_dir_filtered(cfg, &scan::desktop_dir(), None, Some(category_id), None);
    if cfg.organize_public_desktop {
        if let Some(pd) = scan::public_desktop_dir() {
            let r2 = organize_dir_filtered(cfg, &pd, None, Some(category_id), None);
            report.moved.extend(r2.moved);
            report.failed.extend(r2.failed);
            report.skipped += r2.skipped;
        }
    }
    report
}

/// 对外：一键整理真实桌面（含可选公共桌面）
pub fn organize_desktop(cfg: &Config, only: Option<&[String]>) -> OrganizeReport {
    let mut report = organize_dir(cfg, &scan::desktop_dir(), only);
    if cfg.organize_public_desktop {
        if let Some(pd) = scan::public_desktop_dir() {
            let r2 = organize_dir(cfg, &pd, only);
            report.moved.extend(r2.moved);
            report.failed.extend(r2.failed);
            report.skipped += r2.skipped;
        }
    }
    report
}

/// 对外：自动整理（带 8 秒新文件保护期，避免搬走正在写入的文件）
pub fn organize_desktop_auto(cfg: &Config) -> OrganizeReport {
    let mut report =
        organize_dir_filtered(cfg, &scan::desktop_dir(), None, None, Some(8000));
    if cfg.organize_public_desktop {
        if let Some(pd) = scan::public_desktop_dir() {
            let r2 = organize_dir_filtered(cfg, &pd, None, None, Some(8000));
            report.moved.extend(r2.moved);
            report.failed.extend(r2.failed);
            report.skipped += r2.skipped;
        }
    }
    report
}

/// 核心：还原到指定桌面目录（仅允许移动收纳区内的文件，防止误移任意路径）
pub fn restore_to_dir(cfg: &Config, desktop: &Path, paths: &[String]) -> OrganizeReport {
    let mut report = OrganizeReport::default();
    let storage = scan::resolve_storage_root(cfg);

    for p in paths {
        let src = PathBuf::from(p);
        if !src.exists() {
            report.failed.push(FailedEntry { path: p.clone(), error: "文件不存在".into() });
            continue;
        }
        // 保护：绝不移动存储根目录本身（它是所有盒子的容器）
        if scan::same_path(&src, &storage) {
            report.failed.push(FailedEntry {
                path: p.clone(),
                error: "收纳根目录不能被收纳".into(),
            });
            continue;
        }
        if !inside_of(&src, &storage) {
            report.failed.push(FailedEntry {
                path: p.clone(),
                error: "文件不在收纳区内，无法还原".into(),
            });
            continue;
        }
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            report.skipped += 1;
            continue;
        }
        let dst = unique_destination(desktop, &name);
        match move_path(&src, &dst) {
            Ok(()) => report.moved.push(MovedEntry {
                from: p.clone(),
                to: dst.to_string_lossy().to_string(),
                category: "desktop".into(),
            }),
            Err(e) => report.failed.push(FailedEntry { path: p.clone(), error: e }),
        }
    }
    report
}

/// 对外：还原到真实桌面
pub fn restore_to_desktop(cfg: &Config, paths: &[String]) -> OrganizeReport {
    restore_to_dir(cfg, &scan::desktop_dir(), paths)
}

/// 把指定盒子里的全部文件还原到桌面（删除盒子前调用）
pub fn restore_box_contents(cfg: &Config, box_id: &str) -> OrganizeReport {
    let Some(cat) = cfg.categories.iter().find(|c| c.id == box_id) else {
        return OrganizeReport::default();
    };
    let dir = scan::resolve_storage_root(cfg).join(&cat.folder);
    let mut paths = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            paths.push(e.path().to_string_lossy().to_string());
        }
    }
    restore_to_dir(cfg, &scan::desktop_dir(), &paths)
}

/// 单盒模型一次性迁移（v0.2.1）：预设分类停用/隐藏，历史文件合并进「收纳盒」。
/// 返回合并的文件数（0 = 无需迁移）。
pub fn migrate_single_box_files(cfg: &mut Config) -> usize {
    crate::logging::log_line("single-box migration: checking");
    if cfg.migrated_single_box {
        return 0;
    }
    cfg.migrated_single_box = true;
    let mut moved = 0;

    // 1) 预设规则停用 + 不显示（保留配置以便用户随时按需重建）
    for c in &mut cfg.categories {
        if !c.custom && c.id != "inbox" {
            c.enabled = false;
            c.show_box = false;
        }
    }
    // 2) 确保「收纳盒」存在（兜底：收编一切未匹配项，含文件夹）
    if !cfg.categories.iter().any(|c| c.id == "inbox") {
        let mut inbox = crate::config::Category::new_custom("inbox", "收纳盒", "收纳盒", "#5b8cff");
        inbox.is_other = true;
        cfg.categories.insert(0, inbox);
    }

    // 3) 旧分类文件夹的内容合并进收纳盒
    let storage = scan::resolve_storage_root(cfg);
    let inbox_dir = storage.join("收纳盒");
    if std::fs::create_dir_all(&inbox_dir).is_err() {
        return moved;
    }
    for c in &cfg.categories {
        if c.custom || c.id == "inbox" {
            continue;
        }
        let dir = storage.join(&c.folder);
        if !dir.is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(&dir)
            .map(|r| r.flatten().collect::<Vec<_>>())
            .unwrap_or_default();
        for entry in entries {
            let from = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            let to = unique_destination(&inbox_dir, &name);
            if move_path(&from, &to).is_ok() {
                moved += 1;
            }
        }
        let _ = std::fs::remove_dir(&dir); // 空目录顺手清理
    }
    if moved > 0 {
        crate::logging::log_line(&format!(
            "single-box migration: merged {moved} items into 收纳盒"
        ));
    }
    moved
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{default_categories, Config};

    fn cfg_with_storage(root: &Path) -> Config {
        Config {
            storage_root: Some(root.join("收纳").to_string_lossy().to_string()),
            categories: default_categories(),
            ..Default::default()
        }
    }

    #[test]
    fn test_unique_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert_eq!(unique_destination(dir, "a.txt"), dir.join("a.txt"));
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(unique_destination(dir, "a.txt"), dir.join("a (2).txt"));
        std::fs::write(dir.join("a (2).txt"), "x").unwrap();
        assert_eq!(unique_destination(dir, "a.txt"), dir.join("a (3).txt"));
        std::fs::create_dir(dir.join("文件夹")).unwrap();
        assert_eq!(unique_destination(dir, "文件夹"), dir.join("文件夹 (2)"));
    }

    #[test]
    fn test_organize_and_restore_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let desktop = tmp.path().join("Desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        std::fs::write(desktop.join("报告.pdf"), "x").unwrap();
        std::fs::write(desktop.join("游戏.lnk"), "x").unwrap();
        std::fs::write(desktop.join("abc.xyz"), "x").unwrap();
        std::fs::create_dir(desktop.join("我的资料")).unwrap();

        let cfg = cfg_with_storage(tmp.path());
        let storage = scan::resolve_storage_root(&cfg);
        let mut items = Vec::new();
        scan::scan_dir(&desktop, &storage, &cfg, false, &mut items);
        assert_eq!(items.len(), 4);

        let report = organize_dir(&cfg, &desktop, None);
        assert_eq!(report.moved.len(), 4, "report: {:?}", report);
        assert!(storage.join("文档").join("报告.pdf").exists());
        assert!(storage.join("应用").join("游戏.lnk").exists());
        assert!(storage.join("其他").join("abc.xyz").exists());
        assert!(storage.join("文件夹").join("我的资料").is_dir());
        assert_eq!(desktop.read_dir().unwrap().count(), 0, "桌面应被清空");

        // 再跑一次应无事可做
        let again = organize_dir(&cfg, &desktop, None);
        assert!(again.moved.is_empty());

        // 还原
        let back: Vec<String> = report.moved.iter().map(|m| m.to.clone()).collect();
        let r2 = restore_to_dir(&cfg, &desktop, &back);
        assert_eq!(r2.moved.len(), 4);
        assert!(desktop.join("报告.pdf").exists());
        assert!(desktop.join("游戏.lnk").exists());
        assert!(desktop.join("abc.xyz").exists());
        assert!(desktop.join("我的资料").is_dir());
    }

    #[test]
    fn test_restore_rejects_outside_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_with_storage(tmp.path());
        let desktop = tmp.path().join("Desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        let outside = tmp.path().join("外部.txt");
        std::fs::write(&outside, "x").unwrap();
        let r = restore_to_dir(&cfg, &desktop, &[outside.to_string_lossy().to_string()]);
        assert!(r.moved.is_empty());
        assert_eq!(r.failed.len(), 1);
        assert!(outside.exists(), "外部文件不应被动过");
    }

    #[test]
    fn test_move_between_boxes_and_source_limits() {
        let tmp = tempfile::tempdir().unwrap();
        let desktop = tmp.path().join("Desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        let cfg = cfg_with_storage(tmp.path());
        let storage = scan::resolve_storage_root(&cfg);
        let docs = storage.join("文档");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("n.txt"), "x").unwrap();

        // 盒子间移动：文档 -> 其他
        let from = docs.join("n.txt").to_string_lossy().to_string();
        let r = move_into_box_in(&cfg, &desktop, &[from], "others");
        assert_eq!(r.moved.len(), 1);
        assert!(storage.join("其他").join("n.txt").exists());

        // 非桌面、非收纳区的来源应被拒绝
        let elsewhere = tmp.path().join("elsewhere.txt");
        std::fs::write(&elsewhere, "x").unwrap();
        let r2 = move_into_box_in(&cfg, &desktop, &[elsewhere.to_string_lossy().to_string()], "docs");
        assert!(r2.moved.is_empty());
        assert_eq!(r2.failed.len(), 1);
        assert!(elsewhere.exists());
    }

    #[test]
    fn test_conflict_rename_on_move() {
        let tmp = tempfile::tempdir().unwrap();
        let docs = tmp.path().join("文档");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("a.txt"), "old").unwrap();

        let src = tmp.path().join("a.txt");
        std::fs::write(&src, "new").unwrap();
        let dst = unique_destination(&docs, "a.txt");
        move_path(&src, &dst).unwrap();
        assert!(dst.ends_with("a (2).txt"));
        assert_eq!(std::fs::read_to_string(docs.join("a.txt")).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(dst).unwrap(), "new");
    }

    #[test]
    fn test_category_filter_and_recent_skip() {
        let tmp = tempfile::tempdir().unwrap();
        let desktop = tmp.path().join("Desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        std::fs::write(desktop.join("a.txt"), "x").unwrap();
        std::fs::write(desktop.join("b.png"), "x").unwrap();
        let cfg = cfg_with_storage(tmp.path());
        let storage = scan::resolve_storage_root(&cfg);

        // 只收「文档」分类：a.txt 移动，b.png 保留
        let r = organize_dir_filtered(&cfg, &desktop, None, Some("docs"), None);
        assert_eq!(r.moved.len(), 1);
        assert!(storage.join("文档").join("a.txt").exists());
        assert!(desktop.join("b.png").exists());

        // 新文件保护期：刚写入的文件在保护窗口内被跳过
        std::fs::write(desktop.join("c.txt"), "x").unwrap();
        let r2 = organize_dir_filtered(&cfg, &desktop, None, None, Some(8000));
        assert!(r2.moved.is_empty(), "保护期内不应搬移: {:?}", r2.moved);
        assert!(desktop.join("c.txt").exists());
        assert!(desktop.join("b.png").exists());

        // 手动整理（无保护期）：全部搬移
        let r3 = organize_dir_filtered(&cfg, &desktop, None, None, None);
        assert_eq!(r3.moved.len(), 2, "{:?}", r3);
        assert!(storage.join("文档").join("c.txt").exists());
        assert!(storage.join("图片").join("b.png").exists());
    }

    #[test]
    fn test_move_into_box_rejects_disabled_category() {
        let tmp = tempfile::tempdir().unwrap();
        let desktop = tmp.path().join("Desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        let cfg = cfg_with_storage(tmp.path());
        let f = desktop.join("a.txt").to_string_lossy().to_string();
        std::fs::write(desktop.join("a.txt"), "x").unwrap();
        // docs 分类默认启用；禁用后应拒绝收纳
        let mut cfg2 = cfg.clone();
        for c in &mut cfg2.categories {
            if c.id == "docs" {
                c.enabled = false;
            }
        }
        let r = move_into_box_in(&cfg2, &desktop, std::slice::from_ref(&f), "docs");
        assert!(r.moved.is_empty());
        assert_eq!(r.failed.len(), 1);
        assert!(desktop.join("a.txt").exists(), "源文件不应被动过");
        // 启用时正常
        let r2 = move_into_box_in(&cfg, &desktop, &[f], "docs");
        assert_eq!(r2.moved.len(), 1);
    }

    #[test]
    fn test_move_into_box_same_parent_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let desktop = tmp.path().join("Desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        let cfg = cfg_with_storage(tmp.path());
        let storage = scan::resolve_storage_root(&cfg);
        let docs = storage.join("文档");
        std::fs::create_dir_all(&docs).unwrap();
        let p = docs.join("a.txt").to_string_lossy().to_string();
        std::fs::write(docs.join("a.txt"), "x").unwrap();
        let r = move_into_box_in(&cfg, &desktop, &[p], "docs");
        assert_eq!(r.skipped, 1, "同盒内移动应跳过而非报错: {r:?}");
        assert!(r.moved.is_empty() && r.failed.is_empty());
        assert!(docs.join("a.txt").exists());
    }
}

