// 收纳岛：屏幕顶部灵动岛式状态胶囊（借鉴开源项目「岛仔」：信息轻轻出现在顶部，
// 平时收起、鼠标靠近展开）。数据来自 scan-changed/boxes-changed 全量刷新，
// organize-report 到达时播放"活动"动画——灵动岛的精髓。
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { OrganizeReport, ScanPayload } from "../api";
import { esc, svg } from "../util";

let scan: ScanPayload | null = null;
let expanded = false;
let lastSig = "";
let collapseTimer = 0;
let activityTimer = 0;

export function initIsland(): void {
  const app = document.getElementById("app")!;
  app.innerHTML = `
    <div class="island" id="island">
      <div class="island-pill" id="island-pill" title="收纳岛 · 悬停查看收纳区 · 点击打开主面板">
        <span class="island-ico">${svg.inbox}</span>
        <span class="island-label">收纳岛</span>
        <span class="island-stored" id="island-stored">…</span>
        <span class="island-badge" id="island-pending" hidden>0</span>
        <span class="island-ok" id="island-ok" hidden>${svg.check}</span>
        <span class="island-activity" id="island-activity" hidden></span>
      </div>
      <div class="island-panel" id="island-panel" hidden>
        <div class="island-cats" id="island-cats"></div>
        <div class="island-sep"></div>
        <div class="island-todo-title" id="island-todo-title">待整理</div>
        <div class="island-todo" id="island-todo"></div>
        <div class="island-actions">
          <button id="island-organize" title="把桌面待整理文件按分类归位">${svg.inbox}<span>一键整理</span></button>
          <button id="island-undo" title="撤销最近一次收纳批次">${svg.undo}<span>撤销上次</span></button>
          <button id="island-restore" title="收纳区全部还原到桌面">${svg.close}<span>全部还原</span></button>
          <button id="island-open" title="打开收纳文件夹">${svg.folder}<span>收纳夹</span></button>
        </div>
      </div>
    </div>`;

  bind();
  void listen("scan-changed", () => void debouncedRefresh());
  void listen("boxes-changed", () => void debouncedRefresh());
  void listen("organize-report", (e) => onReport(e.payload as OrganizeReport));
  void refresh();
}

let timer = 0;
function debouncedRefresh(): void {
  clearTimeout(timer);
  timer = window.setTimeout(() => void refresh(), 200);
}

async function refresh(): Promise<void> {
  try {
    scan = await api.scan();
  } catch {
    return;
  }
  render();
}

function bind(): void {
  const root = document.getElementById("island")!;
  const pill = document.getElementById("island-pill")!;

  // 悬停展开（岛仔式"靠近即展开"）：窗口由后端 resize，顶边与中心 x 锚定不动。
  // mouseenter 先取消待执行的收起，防止 resize 边缘竞态造成闪烁。
  pill.addEventListener("mouseenter", () => void setExpanded(true));
  pill.addEventListener("click", () => void api.showMainPanel().catch(() => {}));
  root.addEventListener("mouseleave", scheduleCollapse);
  // 点击别处/窗口失焦时收起（双保险）
  window.addEventListener("blur", scheduleCollapse);
  document.getElementById("island-panel")!.addEventListener("click", (e) => {
    const el = e.target as HTMLElement;
    const cat = el.closest<HTMLElement>(".island-cat");
    if (cat?.dataset.id) {
      void api.showBox(cat.dataset.id).catch(() => {});
      return;
    }
    const todo = el.closest<HTMLElement>(".island-todo-item");
    if (todo?.dataset.path) {
      void api.organizePaths([todo.dataset.path], todo.dataset.cat ?? "inbox").catch(() => {});
    }
  });
  document.getElementById("island-organize")!.addEventListener("click", () =>
    void api.organize().catch(() => {}),
  );
  document.getElementById("island-undo")!.addEventListener("click", () =>
    void api.undoLastOrganize().catch(() => {}),
  );
  document.getElementById("island-restore")!.addEventListener("click", () =>
    void api.restoreAll().catch(() => {}),
  );
  document.getElementById("island-open")!.addEventListener("click", () => {
    if (scan) void api.open(scan.storageRoot).catch(() => {});
  });
}

function scheduleCollapse(): void {
  clearTimeout(collapseTimer);
  collapseTimer = window.setTimeout(() => void setExpanded(false), 150);
}

async function setExpanded(next: boolean): Promise<void> {
  if (next) clearTimeout(collapseTimer); // 悬停即取消收起
  if (expanded === next) return;
  expanded = next;
  try {
    await api.islandExpand(next);
  } catch {
    return; // 窗口可能已被关闭/隐藏
  }
  renderPanel();
}

/** organize-report 到达：胶囊"活动"动画 + 短暂结果反馈（灵动岛的 activity） */
function onReport(r: OrganizeReport): void {
  const el = document.getElementById("island-activity")!;
  const pill = document.getElementById("island-pill")!;
  el.hidden = false;
  el.textContent = r.failed.length
    ? `已整理 ${r.moved.length} · ${r.failed.length} 项失败`
    : `已整理 ${r.moved.length} 个`;
  pill.classList.toggle("island-warn", r.failed.length > 0);
  pill.classList.remove("island-anim");
  void pill.offsetWidth; // 重启动画
  pill.classList.add("island-anim");
  clearTimeout(activityTimer);
  activityTimer = window.setTimeout(() => {
    el.hidden = true;
    pill.classList.remove("island-anim", "island-warn");
    void refresh();
  }, 2400);
  void refresh();
}

function render(): void {
  if (!scan) return;
  const stored = scan.boxes.reduce((a, b) => a + b.count, 0);
  const pending = scan.items.length;
  const sig =
    `${stored}|${pending}|` +
    scan.boxes.map((b) => `${b.id}:${b.count}`).join(",") +
    `|${scan.items.slice(0, 6).map((i) => i.path).join(",")}`;
  if (sig === lastSig) {
    renderPanel();
    return;
  }
  lastSig = sig;
  document.getElementById("island-stored")!.textContent = `已收纳 ${stored}`;
  const badge = document.getElementById("island-pending")!;
  const ok = document.getElementById("island-ok")!;
  badge.hidden = pending === 0;
  badge.textContent = String(pending);
  ok.hidden = pending > 0;
  document.getElementById("island-todo-title")!.textContent =
    pending > 0 ? `待整理 ${pending}` : "待整理";
  renderPanel();
}

function renderPanel(): void {
  if (!scan) return;
  const panel = document.getElementById("island-panel")!;
  panel.hidden = !expanded;
  if (!expanded) return;

  document.getElementById("island-cats")!.innerHTML = scan.boxes.length
    ? scan.boxes
        .map(
          (b) => `
      <button class="island-cat" data-id="${esc(b.id)}" title="显示「${esc(b.name)}」收纳盒">
        <i style="background:${esc(b.color)}"></i>
        <span>${esc(b.name)}</span>
        <b>${b.count}</b>
      </button>`,
        )
        .join("")
    : `<div class="island-empty">还没有启用的收纳盒</div>`;

  const todoRoot = document.getElementById("island-todo")!;
  todoRoot.innerHTML = scan.items.length
    ? scan.items
        .slice(0, 5)
        .map(
          (i) => `
      <button class="island-todo-item" data-path="${esc(i.path)}" data-cat="${esc(i.category ?? "inbox")}"
        title="点击收纳到 ${esc(i.category ?? "收纳盒")}">
        <span>${esc(i.name)}</span>
      </button>`,
        )
        .join("") +
      (scan.items.length > 5 ? `<div class="island-todo-more">还有 ${scan.items.length - 5} 项…</div>` : "")
    : `<div class="island-empty">桌面很干净 ✓</div>`;
}
