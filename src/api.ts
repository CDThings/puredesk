// 后端命令的类型化封装
import { invoke } from "@tauri-apps/api/core";

export interface Category {
  id: string;
  name: string;
  folder: string;
  color: string;
  exts: string[];
  matchDir: boolean;
  isOther: boolean;
  enabled: boolean;
  showBox: boolean;
  custom: boolean;
  /** 内置预设分类（不可删除）；缺省 false 视为用户盒子 */
  builtIn?: boolean;
}

export interface BoxCfg {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
  collapsed: boolean;
  visible: boolean;
}

export interface Config {
  storageRoot: string | null;
  autoOrganize: boolean;
  organizePublicDesktop: boolean;
  closeAction: "minimize" | "exit";
  hideStorageFolder: boolean;
  /** 收纳盒图标基准大小（px）；缺省=30 */
  iconSize?: number | null;
  categories: Category[];
  excludes: string[];
  boxes: BoxCfg[];
}

export interface DesktopItem {
  name: string;
  path: string;
  isDir: boolean;
  ext: string;
  category: string | null;
  size: number;
  mtimeMs: number;
  public: boolean;
}

export interface BoxSnapshot {
  id: string;
  name: string;
  folder: string;
  color: string;
  count: number;
  items: DesktopItem[];
  custom: boolean;
  builtIn?: boolean;
}

export interface ScanPayload {
  items: DesktopItem[];
  boxes: BoxSnapshot[];
  storageRoot: string;
  desktopDir: string;
}

export interface OrganizeReport {
  moved: { from: string; to: string; category: string }[];
  failed: { path: string; error: string }[];
  skipped: number;
}

export const api = {
  scan: () => invoke<ScanPayload>("scan_desktop"),
  runtimeInfo: () =>
    invoke<{ integrity: string; lowIntegrity: boolean; desktopDir: string; desktopRedirected: boolean }>("get_runtime_info"),
  organize: () => invoke<OrganizeReport>("organize_desktop"),
  organizeCategory: (boxId: string) =>
    invoke<OrganizeReport>("organize_category", { boxId }),
  deleteToTrash: (paths: string[]) =>
    invoke<OrganizeReport>("delete_to_trash", { paths }),
  renameItem: (path: string, newName: string) =>
    invoke<string>("rename_item", { path, newName }),
  organizePaths: (paths: string[], boxId: string) =>
    invoke<OrganizeReport>("organize_paths", { paths, boxId }),
  restore: (paths: string[]) => invoke<OrganizeReport>("restore_paths", { paths }),
  /** 一键还原全部：所有启用盒子的内容回到桌面根 */
  restoreAll: () => invoke<OrganizeReport>("restore_all"),
  /** 撤销最近一次收纳批次（反向搬回原位） */
  undoLastOrganize: () => invoke<OrganizeReport>("undo_last_organize"),
  open: (path: string) => invoke<void>("open_path", { path }),
  reveal: (path: string) => invoke<void>("reveal_path", { path }),
  icons: (paths: string[]) =>
    invoke<Record<string, string | null>>("get_icons", { paths }),
  logEvent: (message: string) => invoke<void>("log_event", { message }),
  getConfig: () => invoke<Config>("get_config"),
  saveConfig: (config: Config) => invoke<void>("save_config", { config }),
  createBox: (name: string, exts: string[], autoCollect: boolean) =>
    invoke<Category>("create_box", { name, exts, autoCollect }),
  deleteBox: (id: string) => invoke<OrganizeReport>("delete_box", { id }),
  resetBoxLayout: () => invoke<void>("reset_box_layout"),
  /** 重建盒子窗口（启动失败或被关闭后恢复） */
  rebuildBoxes: () => invoke<void>("rebuild_boxes"),
  showBox: (id: string) => invoke<void>("show_box", { id }),
  hideBox: (id: string) => invoke<void>("hide_box", { id }),
  showMainPanel: () => invoke<void>("show_main_panel"),
  setCollapsed: (id: string, collapsed: boolean) =>
    invoke<void>("set_box_collapsed", { id, collapsed }),
  /** 持久化盒子窗口宽高（逻辑像素） */
  setBoxSize: (id: string, w: number, h: number) =>
    invoke<void>("set_box_size", { id, w, h }),
  /** 全局收纳盒图标基准大小（px）；0=重置默认 */
  setBoxIconSize: (size: number) => invoke<void>("set_box_icon_size", { size }),
  setAutostart: (enable: boolean) => invoke<void>("set_autostart", { enable }),
  getAutostart: () => invoke<boolean>("get_autostart"),
  hideMain: () => invoke<void>("hide_main"),
  quit: () => invoke<void>("quit_app"),
};
