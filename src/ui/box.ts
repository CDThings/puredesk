// 桌面收纳盒小组件：无边框透明、置底钉住，双击收起/展开，可拖入文件
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { api } from "../api";
import type { BoxSnapshot, DesktopItem } from "../api";
import { openMenu } from "../menu";
import { confirmText, promptText } from "./dialog";
import { esc, extBadge, fmtTime, svg } from "../util";

let boxId = "";
let box: BoxSnapshot | null = null;
let allBoxes: BoxSnapshot[] = [];
let collapsed = false;
let icons: Record<string, string | null> = {};
let lastRenderSig = "";
/** 多选状态（Ctrl/Shift+click 累积），跨重渲染保留 */
let selectedPaths = new Set<string>();
/** 当前收纳盒图标基准大小（px）；随全局配置同步，改变后写回后端 */
let iconSize = 30;

export function initBox(id: string): void {
  boxId = id;
  const app = document.getElementById("app")!;
  app.innerHTML = `
    <div class="bw" id="bw">
      <header class="bw-head" id="bw-head" title="拖动移动 · 双击收起/展开">
        <button class="bw-btn" id="bw-organize" title="收纳桌面上属于此分类的文件">${svg.inbox}</button>
        <button class="bw-btn" id="bw-folder" title="打开文件夹">${svg.folder}</button>
        <span class="bw-dot" id="bw-dot"></span>
        <span class="bw-title" id="bw-title">…</span>
        <span class="bw-count" id="bw-count"></span>
        <button class="bw-btn bw-iconbtn" id="bw-icon-minus" title="缩小图标">${svg.iconMinus}</button>
        <button class="bw-btn bw-iconbtn" id="bw-icon-plus" title="放大图标">${svg.iconPlus}</button>
        <span class="bw-spacer"></span>
        <button class="bw-btn" id="bw-more" title="盒子菜单">${svg.more}</button>
        <button class="bw-btn" id="bw-collapse" title="收起/展开">${svg.minus}</button>
        <button class="bw-btn" id="bw-hide" title="隐藏此盒">${svg.close}</button>
      </header>
      <div class="bw-body" id="bw-body">
        <div class="bw-grid" id="bw-grid"></div>
        <div class="bw-empty" id="bw-empty">空空如也<br>点左上 📥 收纳桌面同类文件<br>或把文件拖进这里</div>
      </div>
      <div class="bw-mask" id="bw-mask">松开以收纳</div>
      <div class="bw-batch" id="bw-batch" hidden>
        <button class="bw-batch-btn danger" id="bw-batch-del">${svg.trash}<span></span></button>
        <button class="bw-batch-btn" id="bw-batch-move">移入其他盒…</button>
        <button class="bw-batch-btn" id="bw-batch-restore">${svg.undo}<span>还原</span></button>
      </div>
      <div class="bw-resize" id="bw-resize" title="拖拽调整大小">${svg.resize}</div>
      <div class="bw-toast" id="bw-toast"></div>
    </div>`;

  bind();
  void listen("boxes-changed", () => void debouncedRefresh());
  void refresh();
}

let timer = 0;
function debouncedRefresh(): void {
  clearTimeout(timer);
  timer = window.setTimeout(() => void refresh(), 200);
}

async function refresh(): Promise<void> {
  try {
    const s = await api.scan();
    allBoxes = s.boxes;
    box = s.boxes.find((b) => b.id === boxId) ?? null;
    const cfg = await api.getConfig();
    const cfgBox = cfg.boxes.find((b) => b.id === boxId);
    collapsed = cfgBox?.collapsed ?? false;
    iconSize = cfg.iconSize ?? 30;
    applyIconSize();
  } catch {
    return;
  }
  render();
  void loadIcons();
}

/** 把当前 iconSize 应用到 CSS 变量 --icon（盒内图标 & 徽章基准大小） */
function applyIconSize(): void {
  const root = document.documentElement;
  root.style.setProperty("--icon", `${iconSize}px`);
}

function changeIconSize(delta: number): void {
  const next = iconSize + delta;
  if (next < 18 || next > 64) return;
  iconSize = next;
  applyIconSize();
  void api.setBoxIconSize(next);
}

async function loadIcons(): Promise<void> {
  if (!box || !box.items.length) return;
  try {
    const got = await api.icons(box.items.map((i) => i.path).slice(0, 200));
    icons = { ...icons, ...got };
    renderGrid();
  } catch {
    /* 保持徽章 */
  }
}

function hexToRgba(hex: string, alpha: number): string {
  const m = hex.replace("#", "");
  const v = m.length === 3 ? m.split("").map((c) => c + c).join("") : m;
  const n = parseInt(v, 16);
  const r = (n >> 16) & 255;
  const g = (n >> 8) & 255;
  const b = n & 255;
  return `rgba(${r},${g},${b},${alpha})`;
}

function render(): void {
  // 数据无变化时跳过重渲染（避免闪烁与无谓开销）
  const sig = box
    ? `${box.name}|${box.count}|${collapsed}|${box.items.map((i) => i.path).join(",")}`
    : "stopped";
  if (sig === lastRenderSig && document.getElementById("bw-grid")?.childElementCount) {
    return;
  }
  lastRenderSig = sig;
  if (!box) {
    document.getElementById("bw-title")!.textContent = "盒子已停用";
    document.getElementById("bw-grid")!.innerHTML = "";
    clearSelection();
    const empty = document.getElementById("bw-empty") as HTMLElement;
    empty.style.display = "block";
    empty.innerHTML = "该分类已被停用<br>可在主面板设置中重新开启";
    return;
  }
  document.getElementById("bw-dot")!.style.background = box.color;
  document.getElementById("bw-title")!.textContent = box.name;
  document.getElementById("bw-count")!.textContent = String(box.count);
  // 纯手动策展盒（无扩展名规则）不显示「收纳同类」按钮
  document.getElementById("bw-organize")!.style.display = box.custom ? "none" : "";
  document.getElementById("bw")!.dataset.collapsed = String(collapsed);
  document.getElementById("bw-collapse")!.innerHTML = collapsed ? svg.plus : svg.minus;
  // 分类色染色（对齐参考图的彩色半透明盒子；底部压深保证文字可读）
  const el = document.getElementById("bw")!;
  el.style.background = `linear-gradient(180deg, ${hexToRgba(box.color, 0.45)} 0%, ${hexToRgba(box.color, 0.26)} 45%, rgba(13,17,25,0.85) 100%)`;
  el.style.borderColor = hexToRgba(box.color, 0.45);
  renderGrid();
}

function renderGrid(): void {
  if (!box) return;
  const grid = document.getElementById("bw-grid")!;
  const empty = document.getElementById("bw-empty")!;
  if (!box.items.length) {
    grid.innerHTML = "";
    selectedPaths.clear();
    empty.style.display = "block";
    updateBatchBar();
    return;
  }
  empty.style.display = "none";
  grid.innerHTML = box.items.map(itemHtml).join("");
  // 重渲染后重新应用选中态（剪掉已不存在的条目）
  const live = new Set(box.items.map((i) => i.path));
  selectedPaths = new Set([...selectedPaths].filter((p) => live.has(p)));
  for (const el of grid.querySelectorAll<HTMLElement>("[data-path]")) {
    if (selectedPaths.has(el.dataset.path!)) el.classList.add("sel");
  }
  updateBatchBar();
}

/** 根据选中数量显隐批量操作栏 */
function updateBatchBar(): void {
  const bar = document.getElementById("bw-batch");
  if (!bar) return;
  const n = selectedPaths.size;
  bar.hidden = n < 1;
  bar.classList.toggle("show", n >= 1);
  const del = document.getElementById("bw-batch-del")!.querySelector("span")!;
  del.textContent = n > 1 ? `删除 ${n} 项` : "删除";
}

function clearSelection(): void {
  selectedPaths.clear();
  document.querySelectorAll(".bw-item.sel").forEach((x) => x.classList.remove("sel"));
  updateBatchBar();
}

function itemHtml(item: DesktopItem): string {
  const ic = icons[item.path];
  const icon = ic
    ? `<img class="ficon" src="${ic}" alt="" draggable="false">`
    : extBadge(item, box?.color ?? "#8a97a6");
  return `
    <div class="bw-item" data-path="${esc(item.path)}" title="${esc(item.name)} · ${esc(fmtTime(item.mtimeMs))}">
      ${icon}
      <div class="bw-item-name">${esc(item.name)}</div>
    </div>`;
}

function bind(): void {
  const win = getCurrentWebviewWindow();

  // 标题栏拖动 / 双击收起
  const head = document.getElementById("bw-head")!;
  head.addEventListener("pointerdown", (e) => {
    const t = e.target as HTMLElement;
    if (t.closest("button")) return;
    if (e.button !== 0) return;
    void win.startDragging();
  });
  head.addEventListener("dblclick", (e) => {
    if ((e.target as HTMLElement).closest("button")) return;
    void api.setCollapsed(boxId, !collapsed);
  });

  document.getElementById("bw-collapse")!.addEventListener("click", () =>
    void api.setCollapsed(boxId, !collapsed),
  );
  document.getElementById("bw-hide")!.addEventListener("click", () => void api.hideBox(boxId));
  document.getElementById("bw-icon-minus")!.addEventListener("click", () => changeIconSize(-4));
  document.getElementById("bw-icon-plus")!.addEventListener("click", () => changeIconSize(4));
  // 右下角手柄：拖拽调整盒子窗口大小（startResizeDragging → 后端 Resized 事件落盘 w/h）
  const handle = document.getElementById("bw-resize")!;
  handle.addEventListener("pointerdown", (e) => {
    e.stopPropagation();
    if (e.button !== 0) return;
    void win.startResizeDragging("SouthEast");
  });
  document.getElementById("bw-more")!.addEventListener("click", (e) => {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    boxMenu(r.left, r.bottom + 4);
  });
  document.getElementById("bw-folder")!.addEventListener("click", () => {
    if (box) void api.open(box.folder);
  });
  document.getElementById("bw-organize")!.addEventListener("click", () => {
    void api
      .organizeCategory(boxId)
      .then((r) => {
        const moved = r.moved.length;
        const failed = r.failed.length;
        showBoxToast(
          moved > 0 ? `已收纳 ${moved} 项${failed ? `，失败 ${failed}` : ""}` : "桌面上没有同类文件",
        );
      })
      .catch((e) => showBoxToast(`失败: ${e}`));
  });

  // 批量操作栏（选中 ≥1 项时出现）
  document.getElementById("bw-batch-del")!.addEventListener("click", () => {
    const paths = [...selectedPaths];
    if (!paths.length) return;
    void api
      .deleteToTrash(paths)
      .then((r) => {
        showBoxToast(
          r.failed.length
            ? `已删除 ${r.moved.length} 项，${r.failed.length} 项失败（详见日志）`
            : `已删除 ${r.moved.length} 项（回收站）`,
        );
        clearSelection();
      })
      .catch((e) => showBoxToast(`删除失败: ${e}`));
  });
  document.getElementById("bw-batch-move")!.addEventListener("click", () =>
    openMoveModal([...selectedPaths]),
  );
  document.getElementById("bw-batch-restore")!.addEventListener("click", () => {
    const paths = [...selectedPaths];
    if (!paths.length) return;
    void api
      .restore(paths)
      .then((r) => {
        showBoxToast(`已还原 ${r.moved.length} 项到桌面`);
        clearSelection();
      })
      .catch((e) => showBoxToast(`还原失败: ${e}`));
  });

  // 条目：双击打开、右键菜单（grid 用事件委托，重渲染不影响）
  document.getElementById("bw-grid")!.addEventListener("dblclick", (e) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-path]");
    if (el) void api.open(el.dataset.path!);
  });
  document.getElementById("bw-grid")!.addEventListener("click", (e) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-path]");
    if (!el) {
      clearSelection(); // 点空白处取消全部选中
      return;
    }
    const path = el.dataset.path!;
    if (e.ctrlKey || e.metaKey) {
      // Ctrl/⌘ 点：累积切换选择
      if (selectedPaths.has(path)) {
        selectedPaths.delete(path);
        el.classList.remove("sel");
      } else {
        selectedPaths.add(path);
        el.classList.add("sel");
      }
      updateBatchBar();
    } else if (e.shiftKey) {
      // Shift 点：并入当前项（不取消其他已选）
      selectedPaths.add(path);
      el.classList.add("sel");
      updateBatchBar();
    } else {
      // 普通点击：单选
      clearSelection();
      selectedPaths.add(path);
      el.classList.add("sel");
      updateBatchBar();
    }
  });
  document.getElementById("bw-grid")!.addEventListener("contextmenu", (e) => {
    const el = (e.target as HTMLElement).closest<HTMLElement>("[data-path]");
    if (!el) return;
    e.preventDefault();
    itemMenu(el.dataset.path!, e.clientX, e.clientY);
  });

  // 拖入收纳：原生 wry 事件 + 自注入 OLE 拖放目标（双保险）
  const mask = document.getElementById("bw-mask")!;
  const handleDroppedPaths = (paths: string[]): void => {
    void api
      .organizePaths(paths, boxId)
      .then((r) => {
        if (r.moved.length) mask.textContent = `已收纳 ${r.moved.length} 项`;
        setTimeout(() => (mask.textContent = "松开以收纳"), 1500);
      })
      .catch(() => undefined);
  };
  void win.onDragDropEvent((ev) => {
    const p = ev.payload as { type: string; paths?: string[] };
    void api.logEvent(`drag-drop event on box-${boxId}: type=${p.type} paths=${JSON.stringify(p.paths ?? [])}`);
    if (p.type === "enter" || p.type === "over") {
      mask.classList.add("show");
    } else {
      mask.classList.remove("show");
      if (p.type === "drop" && p.paths && p.paths.length) handleDroppedPaths(p.paths);
    }
  });
  // 兜底拖放由后端 IDropTarget 直接收纳并广播 boxes-changed，本窗口监听刷新即可
}

function itemMenu(path: string, x: number, y: number): void {
  const item = box?.items.find((i) => i.path === path);
  if (!item) return;
  const entries: Parameters<typeof openMenu>[2] = [
    { label: "打开", action: () => void api.open(item.path) },
    { label: "在资源管理器中显示", action: () => void api.reveal(item.path) },
  ];
  if (allBoxes.length > 1) {
    entries.push({ sep: true });
    entries.push({ label: "移入其他盒…", action: () => openMoveModal([item.path]) });
  }
  entries.push({ sep: true });
  entries.push({
    label: "重命名",
    action: () => {
      void promptText("重命名", item.name).then((name) => {
        if (!name || name === item.name) return;
        void api
          .renameItem(item.path, name)
          .then(() => showBoxToast(`已重命名为「${name}」`))
          .catch((e) => showBoxToast(`重命名失败: ${e}`));
      });
    },
  });
  entries.push({
    label: "还原到桌面",
    action: () =>
      void api
        .restore([item.path])
        .then((r) => showBoxToast(`已还原 ${r.moved.length} 项到桌面`))
        .catch((e) => showBoxToast(`还原失败: ${e}`)),
  });
  entries.push({
    label: "删除（进回收站）",
    danger: true,
    action: () =>
      void api
        .deleteToTrash([item.path])
        .then((r) => showBoxToast(r.failed.length ? "删除失败（详见日志）" : "已删除到回收站"))
        .catch((e) => showBoxToast(`删除失败: ${e}`)),
  });
  openMenu(x, y, entries);
}

/** 盒子头部「⋮」菜单 */
function boxMenu(x: number, y: number): void {
  const entries: Parameters<typeof openMenu>[2] = [
    { label: "编辑盒子", action: () => void api.showMainPanel() },
  ];
  if (box && !box.builtIn) {
    entries.push({ sep: true });
    entries.push({
      label: "删除盒子（文件还原到桌面）",
      danger: true,
      action: () => {
        void confirmText(`删除盒子「${box!.name}」？盒内 ${box!.count} 项文件将还原到桌面。`, "删除").then(
          (ok) => {
            if (!ok) return;
            void api
              .deleteBox(boxId)
              .then(() => showBoxToast("盒子已删除，文件已还原"))
              .catch((e) => showBoxToast(`删除失败: ${e}`));
          },
        );
      },
    });
  }
  openMenu(x, y, entries);
}

/** 「移入其他盒」模态：列出除当前盒外的所有盒子，点击即批量移动 */
function openMoveModal(paths: string[]): void {
  if (!paths.length || !box) return;
  const others = allBoxes.filter((b) => b.id !== boxId);
  if (!others.length) {
    showBoxToast("没有其他盒子可移入");
    return;
  }
  const overlay = document.createElement("div");
  overlay.className = "modal-overlay";
  overlay.innerHTML = `
    <div class="modal-box" style="width: 320px;">
      <div class="modal-title">移入其他盒（${paths.length} 项）</div>
      <div class="move-list">
        ${others
          .map(
            (b) =>
              `<button class="move-item" data-id="${esc(b.id)}">
                <span class="dot" style="background:${esc(b.color)}"></span>
                <b>${esc(b.name)}</b><em>${b.count}</em>
              </button>`,
          )
          .join("")}
      </div>
      <div class="modal-btns"><button data-act="cancel">取消</button></div>
    </div>`;
  document.body.appendChild(overlay);
  const close = (): void => {
    overlay.remove();
    document.removeEventListener("keydown", onKey, true);
  };
  const onKey = (e: KeyboardEvent): void => {
    if (e.key === "Escape") close();
  };
  overlay.addEventListener("click", (e) => {
    const t = e.target as HTMLElement;
    if (t.dataset.act === "cancel" || t === overlay) close();
    const target = t.closest<HTMLElement>(".move-item");
    if (target) {
      close();
      void api
        .organizePaths(paths, target.dataset.id!)
        .then((r) => {
          showBoxToast(`已移入「${others.find((b) => b.id === target.dataset.id)?.name ?? "?"}」${r.moved.length} 项`);
          clearSelection();
        })
        .catch((err) => showBoxToast(`移动失败: ${err}`));
    }
  });
  document.addEventListener("keydown", onKey, true);
}

let toastTimer = 0;
function showBoxToast(msg: string): void {
  const el = document.getElementById("bw-toast");
  if (!el) return;
  el.textContent = msg;
  el.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => el.classList.remove("show"), 2400);
}
