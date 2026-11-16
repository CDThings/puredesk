//! 全局共享状态：配置读写锁、图标缓存、几何脏标记、置底钉住的代次计数。

use crate::config::{self, Config};
use crate::icons::IconCache;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

pub struct AppState {
    pub config: RwLock<Config>,
    pub icons: IconCache,
    /// 盒子窗口拖动后几何信息有未落盘的修改
    pub geom_dirty: AtomicBool,
    /// 拖动代次：拖动结束（事件停止）后才把窗口重新钉到底层
    pub pin_gen: AtomicU64,
    /// 在此时刻之前，自动整理被抑制（例如刚执行“还原到桌面”）
    suppress_until: Mutex<Option<Instant>>,
    /// 序列化所有整理/移动操作，避免并发扫描+移动产生竞态
    move_lock: Mutex<()>,
}

/// 毒化锁容错读取（panic 后也能继续运行，绝不因锁中毒而崩溃）
pub fn read_lock<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|p| p.into_inner())
}

pub fn write_lock<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|p| p.into_inner())
}

pub fn mutex_lock<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|p| p.into_inner())
}

impl AppState {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(config::load()),
            icons: IconCache::new(),
            geom_dirty: AtomicBool::new(false),
            pin_gen: AtomicU64::new(0),
            suppress_until: Mutex::new(None),
            move_lock: Mutex::new(()),
        }
    }

    pub fn config_read(&self) -> RwLockReadGuard<'_, Config> {
        read_lock(&self.config)
    }

    pub fn config_write(&self) -> RwLockWriteGuard<'_, Config> {
        write_lock(&self.config)
    }

    /// 整理/移动操作的互斥入口（防并发竞态：双重扫描、重名竞争）
    pub fn move_guard(&self) -> MutexGuard<'_, ()> {
        mutex_lock(&self.move_lock)
    }

    pub fn suppress_auto_organize(&self, secs: u64) {
        let mut g = self.suppress_until.lock().unwrap();
        *g = Some(Instant::now() + Duration::from_secs(secs));
    }

    pub fn auto_organize_suppressed(&self) -> bool {
        match *self.suppress_until.lock().unwrap() {
            Some(t) => Instant::now() < t,
            None => false,
        }
    }

    /// 把当前内存中的配置落盘
    pub fn persist_config(&self) -> Result<(), String> {
        let cfg = self.config_read().clone();
        config::save(&cfg)
    }

    /// 配置文件的合并保存：仅在标记为脏时写盘（供几何刷新线程调用）
    pub fn flush_if_dirty(&self) {
        if self.geom_dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            let cfg = self.config_read().clone();
            let _ = config::save(&cfg);
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
