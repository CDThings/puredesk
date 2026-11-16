// 主面板：概览（待整理 / 收纳盒）、一键整理、搜索、设置
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "../api";
import type { BoxSnapshot, Config, DesktopItem, ScanPayload } from "../api";
import { openMenu } from "../menu";
import { promptText } from "./dialog";
import { esc, extBadge, fmtSize, fmtTime, svg } from "../util";

let scan: ScanPayload | null = null;
let config: Config | null = null;
let icons: Record<string, string | null> = {};
let query = "";
let tab: "home" | "settings" = "home";
let autostart = false;
let busy = false;

export function initMain(): void {
  const app = document.getElementById("app")!;
  app.innerHTML = `
    <div class="app">
      <header class="titlebar" data-drag>
        <div class="brand"><span class="logo"></span><b>PureDesk</b><span class="sub">桌面收纳</span></div>
        <div class="searchbox">${svg.search}<input id="q" type="text" placeholder="搜索桌面与盒子…" spellcheck="false"></div>
        <div class="winbtns">
          <button id="btn-min" title="最小化">${svg.minus}</button>
          <button id="btn-close" title="关闭到托盘">${svg.close}</button>
        </div>
      </header>
      <div class="tabs">
        <button data-tab="home" class="active">概览</button>
        <button data-tab="settings">设置</button>
      </div>
      <div class="page" id="page-home">
        <section class="warn-banner" id="warn-banner" hidden>
          <div class="warn-text">
            <b>⚠️ 当前以「低完整性」运行，无法写入桌面</b>
            <span>原因：Windows SmartScreen 放行启动的进程会继承低权限令牌。两种解决：</span>
            <span>① Windows 安全中心 → 基于声誉的保护 → 关闭「检查应用和文件」后重启本应用；</span>
            <span>② 在下方「收纳存储位置」改到非桌面路径（如 E:\PureDesk 存储）。</span>
          </div>
        </section>
        <section class="warn-banner" id="onedrive-banner" hidden>
          <div class="warn-text">
            <b>⚠️ 桌面已被 OneDrive / 网盘重定向</b>
            <span>你的桌面路径不在本机（<span id="onedrive-path"></span>）。整理类软件与网盘同步共存易冲突（文件被“收走”或双向搬运）。建议：</span>
            <span>① OneDrive 设置 → 同步和备份 → 管理备份 → 停止“桌面”备份，把桌面恢复到本机路径；</span>
            <span>② 或在「设置」把收纳存储位置改到非桌面路径，并谨慎使用自动整理。</span>
          </div>
        </section>
        <section class="hero">
          <div class="stats" id="stats"></div>
          <div class="actions">
            <label class="switch" title="新文件自动归入收纳盒">
              <input type="checkbox" id="auto-toggle"><i></i><span>自动整理</span>
            </label>
            <button id="btn-undo" title="把最近一次整理/拖入盒子的文件搬回原位">${svg.undo} 撤销上次整理</button>
            <button id="btn-restore-all" title="把所有盒子里的文件全部放回桌面">${svg.undo} 全部还原</button>
            <button class="primary" id="btn-organize">${svg.bolt} 一键整理</button>
          </div>
        </section>
        <section class="sec" id="sec-results"></section>
        <section class="sec" id="sec-boxes"></section>
      </div>
      <div class="page" id="page-settings" hidden></div>
      <footer class="statusbar"><span id="status"></span></footer>
    </div>
    <div id="toast"></div>`;

  bindStatic();
  void listen("scan-changed", () => void debouncedRefresh());
  void listen("organize-report", (e) => {
    const r = e.payload as { moved: unknown[]; failed: { path: string }[] };
    const moved = r?.moved?.length ?? 0;
    const failed = r?.failed?.length ?? 0;
    if (moved > 0) toast(`已自动整理 ${moved} 项${failed ? `，${failed} 项失败` : ""}`);
  });
  void refresh();
  void api.getAutostart().then((v) => {
    autostart = v;
    if (tab === "settings") renderSettings();
  });
  // 低完整性自检（SmartScreen 放行导致写不进桌面时给出明确指引）
  void api
    .runtimeInfo()
    .then((info) => {
      const banner = document.getElementById("warn-banner");
      if (banner && info.lowIntegrity) {
        banner.hidden = false;
        document.querySelector(".app")!.classList.add("low-il");
      }
      // 桌面被 OneDrive/网盘重定向：整理与同步共存易冲突，提前警告
      if (info.desktopRedirected) {
        const od = document.getElementById("onedrive-banner");
        if (od) {
          od.hidden = false;
          const p = document.getElementById("onedrive-path");
          if (p) p.textContent = info.desktopDir;
        }
      }
    })
    .catch(() => undefined);
}

let refreshTimer = 0;
function debouncedRefresh(): void {
  clearTimeout(refreshTimer);
  refreshTimer = window.setTimeout(() => void refresh(), 250);
}

async function refresh(): Promise<void> {
  try {
    const [s, c] = await Promise.all([api.scan(), api.getConfig()]);
    scan = s;
    config = c;
  } catch (e) {
    setStatus(`加载失败: ${e}`);
    return;
  }
  render();
  void loadIcons();
}

async function loadIcons(): Promise<void> {
  if (!scan) return;
  const paths = [
    ...scan.items.map((i) => i.path),
    ...scan.boxes.flatMap((b) => b.items.map((i) => i.path)),
  ].slice(0, 300);
  if (!paths.length) return;
  try {
    const got = await api.icons(paths);
    icons = { ...icons, ...got };
    renderResults();
    renderBoxes();
  } catch {
    /* 图标加载失败则保持徽章 */
  }
}

function bindStatic(): void {
  const win = getCurrentWindow();
  document.querySelector<HTMLElement>(".titlebar")!.addEventListener("pointerdown", (e) => {
    const t = e.target as HTMLElement;
    if (t.closest("button, input")) return;
    if (e.button !== 0) return;
    void win.startDragging();
  });
  document.getElementById("btn-min")!.addEventListener("click", () => void win.minimize());
  document.getElementById("btn-close")!.addEventListener("click", () => void win.close());

  document.querySelectorAll<HTMLButtonElement>(".tabs button").forEach((b) => {
    b.addEventListener("click", () => {
      tab = b.dataset.tab as "home" | "settings";
      document.querySelectorAll<HTMLButtonElement>(".tabs button").forEach((x) =>
        x.classList.toggle("active", x === b),
      );
      document.getElementById("page-home")!.hidden = tab !== "home";
      document.getElementById("page-settings")!.hidden = tab !== "settings";
      if (tab === "settings") renderSettings();
    });
  });

  document.getElementById("q")!.addEventListener("input", (e) => {
    query = (e.target as HTMLInputElement).value.trim();
    renderResults();
  });

  document.getElementById("btn-organize")!.addEventListener("click", () => void doOrganize());
  document.getElementById("btn-undo")!.addEventListener("click", () => void doUndo());
  document.getElementById("btn-restore-all")!.addEventListener("click", () => void doRestoreAll());

  document.getElementById("auto-toggle")!.addEventListener("change", async (e) => {
    if (!config) return;
    const on = (e.target as HTMLInputElement).checked;
    config = { ...config, autoOrganize: on };
    try {
      await api.saveConfig(config);
      toast(on ? "已开启自动整理" : "已关闭自动整理");
    } catch (err) {
      toast(`保存失败: ${err}`);
    }
  });

  // 事件委托：待整理 / 搜索结果 / 盒子卡片
  const home = document.getElementById("page-home")!;
  home.addEventListener("click", (e) => {
    const chip = (e.target as HTMLElement).closest<HTMLElement>("[data-open]");
    if (chip) {
      void api.open(chip.dataset.open!);
      return;
    }
    const boxBtn = (e.target as HTMLElement).closest<HTMLElement>("[data-box-action]");
    if (boxBtn) {
      const [action, id] = boxBtn.dataset.boxAction!.split(":");
      if (action === "show") void api.showBox(id);
      if (action === "hide") void api.hideBox(id);
      if (action === "folder") {
        const box = scan?.boxes.find((b) => b.id === id);
        if (box) void api.open(box.folder);
      }
      if (action === "organize") {
        void api.organizeCategory(id).then(reportToast);
      }
    }
  });
  home.addEventListener("contextmenu", (e) => {
    const chip = (e.target as HTMLElement).closest<HTMLElement>("[data-ctx-item]");
    if (chip) {
      e.preventDefault();
      itemMenu(chip.dataset.ctxItem!, e.clientX, e.clientY);
      return;
    }
    const card = (e.target as HTMLElement).closest<HTMLElement>("[data-ctx-box]");
    if (card) {
      e.preventDefault();
      boxMenu(card.dataset.ctxBox!, e.clientX, e.clientY);
    }
  });
}

function itemByPath(path: string): { item: DesktopItem; boxName?: string } | null {
  const p = scan?.items.find((i) => i.path === path);
  if (p) return { item: p };
  for (const b of scan?.boxes ?? []) {
    const q = b.items.find((i) => i.path === path);
    if (q) return { item: q, boxName: b.name };
  }
  return null;
}

function itemMenu(path: string, x: number, y: number): void {
  const found = itemByPath(path);
  if (!found) return;
  const { item, boxName } = found;
  const entries: Parameters<typeof openMenu>[2] = [];
  entries.push({ label: "打开", action: () => void api.open(item.path) });
  entries.push({ label: "在资源管理器中显示", action: () => void api.reveal(item.path) });
  entries.push({
    label: "重命名",
    action: () => {
      void promptText("重命名", item.name).then((name) => {
        if (!name || name === item.name) return;
        void api
          .renameItem(item.path, name)
          .then(() => toast(`已重命名为「${name}」`))
          .catch((e) => toast(`重命名失败: ${e}`));
      });
    },
  });
  entries.push({
    label: "删除（进回收站）",
    danger: true,
    action: () =>
      void api
        .deleteToTrash([item.path])
        .then((r) => toast(r.failed.length ? "删除失败（详见日志）" : "已删除到回收站"))
        .catch((e) => toast(`删除失败: ${e}`)),
  });
  if (boxName) {
    entries.push({ sep: true });
    entries.push({
      label: `从「${boxName}」还原到桌面`,
      action: () => void api.restore([item.path]).then(reportToast),
    });
  } else if (!item.public) {
    entries.push({ sep: true });
    for (const b of scan?.boxes ?? []) {
      entries.push({
        label: `移入「${b.name}」`,
        action: () => void api.organizePaths([item.path], b.id).then(reportToast),
      });
    }
  }
  openMenu(x, y, entries);
}

function boxMenu(id: string, x: number, y: number): void {
  const box = scan?.boxes.find((b) => b.id === id);
  if (!box) return;
  const visible = config?.boxes.find((b) => b.id === id)?.visible ?? true;
  openMenu(x, y, [
    { label: "打开盒子文件夹", action: () => void api.open(box.folder) },
    {
      label: visible ? "隐藏收纳盒" : "显示收纳盒",
      action: () => void (visible ? api.hideBox(id) : api.showBox(id)),
    },
    {
      label: "把桌面同类文件整理进来",
      action: () => {
        const paths = (scan?.items ?? [])
          .filter((i) => !i.public && i.category === id)
          .map((i) => i.path);
        if (!paths.length) {
          toast("桌面上没有可归入此盒的文件");
          return;
        }
        void api.organizePaths(paths, id).then(reportToast);
      },
    },
  ]);
}

async function doOrganize(): Promise<void> {
  if (busy) return;
  busy = true;
  const btn = document.getElementById("btn-organize")!;
  btn.classList.add("loading");
  setStatus("整理中…");
  try {
    const r = await api.organize();
    reportToast(r);
  } catch (e) {
    toast(`整理失败: ${e}`);
  } finally {
    busy = false;
    btn.classList.remove("loading");
    setStatus("");
  }
}

async function doUndo(): Promise<void> {
  if (busy) return;
  busy = true;
  const btn = document.getElementById("btn-undo")!;
  btn.classList.add("loading");
  try {
    const r = await api.undoLastOrganize();
    const moved = r.moved?.length ?? 0;
    if (moved > 0) {
      toast(`已撤销：${moved} 项搬回原位${r.failed.length ? `，${r.failed.length} 项失败` : ""}`);
    } else if (r.skipped > 0) {
      toast("没有可撤销的整理记录");
    } else {
      toast("撤销完成（没有需要搬回的文件）");
    }
  } catch (e) {
    toast(`撤销失败: ${e}`);
  } finally {
    busy = false;
    btn.classList.remove("loading");
  }
}

async function doRestoreAll(): Promise<void> {
  if (busy) return;
  busy = true;
  const btn = document.getElementById("btn-restore-all")!;
  btn.classList.add("loading");
  setStatus("还原中…");
  try {
    const r = await api.restoreAll();
    const moved = r.moved?.length ?? 0;
    toast(
      moved > 0
        ? `已还原 ${moved} 项到桌面${r.failed.length ? `，${r.failed.length} 项失败` : ""}`
        : "盒子里没有可还原的文件",
    );
  } catch (e) {
    toast(`还原失败: ${e}`);
  } finally {
    busy = false;
    btn.classList.remove("loading");
    setStatus("");
  }
}

function reportToast(r: { moved: unknown[]; failed: { error: string }[]; skipped?: number }): void {
  const moved = r.moved?.length ?? 0;
  const failed = r.failed?.length ?? 0;
  let msg = moved > 0 ? `已整理 ${moved} 项` : "没有需要整理的文件";
  if (failed > 0) {
    const first = r.failed[0]?.error ?? "";
    msg += `，失败 ${failed} 项：${first.length > 48 ? first.slice(0, 48) + "…" : first}`;
    msg += `（详见 puredesk.log）`;
  }
  toast(msg);
}

function iconHtml(item: DesktopItem, color: string): string {
  const ic = icons[item.path];
  if (ic) {
    return `<img class="ficon" src="${ic}" alt="" draggable="false">`;
  }
  return extBadge(item, color);
}

function catColor(id: string | null): string {
  const c = config?.categories.find((x) => x.id === id);
  return c?.color ?? "#8a97a6";
}

function catName(id: string | null): string {
  const c = config?.categories.find((x) => x.id === id);
  return c?.name ?? "未分类";
}

function chipHtml(item: DesktopItem): string {
  const color = catColor(item.category);
  const meta = [item.isDir ? "文件夹" : fmtSize(item.size), fmtTime(item.mtimeMs), item.public ? "公共" : ""]
    .filter(Boolean)
    .join(" · ");
  return `
    <div class="chip" data-open="${esc(item.path)}" data-ctx-item="${esc(item.path)}" title="${esc(item.path)}">
      ${iconHtml(item, color)}
      <div class="chip-main">
        <div class="chip-name">${esc(item.name)}</div>
        <div class="chip-meta">${esc(meta)} · <span style="color:${color}">${esc(catName(item.category))}</span></div>
      </div>
    </div>`;
}

function render(): void {
  if (!scan || !config) return;
  const pending = scan.items;
  const inBoxes = scan.boxes.reduce((n, b) => n + b.count, 0);
  const visibleBoxes = scan.boxes.length;
  document.getElementById("stats")!.innerHTML = `
    <div class="stat"><b>${pending.length}</b><span>待整理</span></div>
    <div class="stat"><b>${inBoxes}</b><span>已收纳</span></div>
    <div class="stat"><b>${visibleBoxes}</b><span>收纳盒</span></div>`;
  const auto = document.getElementById("auto-toggle") as HTMLInputElement | null;
  if (auto) auto.checked = config.autoOrganize;
  renderResults();
  renderBoxes();
  setStatus(`桌面：${scan.desktopDir}　收纳区：${scan.storageRoot}`);
}

function renderResults(): void {
  if (!scan) return;
  const sec = document.getElementById("sec-results")!;
  if (query) {
    const q = query.toLowerCase();
    const hits: { item: DesktopItem; box?: BoxSnapshot }[] = [];
    for (const b of scan.boxes) {
      for (const i of b.items) if (i.name.toLowerCase().includes(q)) hits.push({ item: i, box: b });
    }
    for (const i of scan.items) if (i.name.toLowerCase().includes(q)) hits.push({ item: i });
    sec.innerHTML = `<h2>搜索 “${esc(query)}” · ${hits.length} 项</h2>` +
      (hits.length
        ? `<div class="chip-grid">${hits.map((h) => chipHtml(h.item)).join("")}</div>`
        : `<div class="empty">没有匹配的文件</div>`);
    return;
  }
  sec.innerHTML = `<h2>待整理 <em>${scan.items.length}</em></h2>` +
    (scan.items.length
      ? `<div class="chip-grid">${scan.items.map(chipHtml).join("")}</div>`
      : `<div class="empty">桌面很干净 ✓ 新文件${config?.autoOrganize ? "会自动归入收纳盒" : "可点击「一键整理」归位"}</div>`);
}

function renderBoxes(): void {
  if (!scan || !config) return;
  const sec = document.getElementById("sec-boxes")!;
  const visibleSet = new Set(config.boxes.filter((b) => b.visible).map((b) => b.id));
  sec.innerHTML = `
    <div class="sec-head"><h2>收纳盒 <em>${scan.boxes.length}</em></h2>
      <div style="display:flex;gap:6px">
        ${scan.boxes.length === 0 ? `<button class="mini-btn" id="btn-rebuild-boxes">${svg.undo} 重建盒子窗口</button>` : ""}
        <button class="mini-btn" id="btn-new-box" title="新建盒子：选个名字，勾选文件类型（可选）">${svg.plus} 新建</button></div></div>
    <div class="box-cards">
      ${scan.boxes
        .map((b) => {
          const visible = visibleSet.has(b.id);
          return `
          <div class="box-card" data-ctx-box="${esc(b.id)}">
            <span class="dot" style="background:${esc(b.color)}"></span>
            <div class="box-main">
              <div class="box-name">${esc(b.name)} <em>${b.count}</em></div>
              <div class="box-path" title="${esc(b.folder)}">${esc(b.folder)}</div>
            </div>
            <div class="box-btns">
              ${b.custom || b.id === "inbox" ? "" : `<button title="把桌面上同类文件收进此盒" data-box-action="organize:${esc(b.id)}">${svg.inbox}</button>`}
              <button title="${visible ? "隐藏收纳盒" : "显示收纳盒"}" data-box-action="${visible ? "hide" : "show"}:${esc(b.id)}">${svg.eye}</button>
              <button title="打开文件夹" data-box-action="folder:${esc(b.id)}">${svg.folder}</button>
            </div>
          </div>`;
        })
        .join("")}
    </div>`;
  document.getElementById("btn-new-box")!.addEventListener("click", () => openCreateBoxModal());
  const rebuildBtn = document.getElementById("btn-rebuild-boxes");
  if (rebuildBtn) {
    rebuildBtn.addEventListener("click", () =>
      void api.rebuildBoxes().then(() => toast("盒子窗口已重建")).catch((e) => toast(`重建失败: ${e}`)),
    );
  }
}

function renderSettings(): void {
  if (!config) return;
  const page = document.getElementById("page-settings")!;
  const cats = config.categories
    .map((c) => {
      const locked = c.matchDir || c.isOther;
      return `
      <div class="cat-row" data-cat="${esc(c.id)}">
        <label class="ck" title="启用此分类"><input type="checkbox" data-f="enabled" ${c.enabled ? "checked" : ""}></label>
        <input type="color" class="color" data-f="color" value="${esc(c.color)}" title="盒子颜色">
        <input class="in name" data-f="name" value="${esc(c.name)}" placeholder="名称">
        <input class="in folder" data-f="folder" value="${esc(c.folder)}" placeholder="文件夹名">
        <input class="in exts" data-f="exts" value="${esc(c.exts.join(", "))}" placeholder="${locked ? "按类型自动匹配" : "扩展名，逗号分隔；留空=纯手动"}" ${locked ? "disabled" : ""}>
        <label class="ck" title="显示桌面收纳盒"><input type="checkbox" data-f="showBox" ${c.showBox ? "checked" : ""}><span>盒子</span></label>
        <button class="del" data-del="${esc(c.id)}" title="${c.builtIn ? "内置分类不可删除" : "删除此盒子（文件还原到桌面）"}" ${c.builtIn ? "disabled" : ""}>🗑</button>
      </div>`;
    })
    .join("");
  page.innerHTML = `
    <h2>通用</h2>
    <div class="form-row">
      <label>收纳存储位置</label>
      <input class="in wide" id="set-storage" value="${esc(config.storageRoot ?? "")}" placeholder="默认：桌面\\桌面整理（可改为任意绝对路径）" spellcheck="false">
    </div>
    <div class="form-toggles">
      <label class="switch"><input type="checkbox" id="set-auto" ${config.autoOrganize ? "checked" : ""}><i></i><span>自动整理新文件</span></label>
      <label class="switch"><input type="checkbox" id="set-public" ${config.organizePublicDesktop ? "checked" : ""}><i></i><span>整理公共桌面</span></label>
      <label class="switch"><input type="checkbox" id="set-autostart" ${autostart ? "checked" : ""}><i></i><span>开机自启</span></label>
      <label class="switch"><input type="checkbox" id="set-hide-folder" ${config.hideStorageFolder ? "checked" : ""}><i></i><span>隐藏桌面上的收纳文件夹</span></label>
      <label class="sel-wrap">关闭主面板
        <select class="in" id="set-close">
          <option value="minimize" ${config.closeAction !== "exit" ? "selected" : ""}>最小化到托盘</option>
          <option value="exit" ${config.closeAction === "exit" ? "selected" : ""}>结束进程</option>
        </select>
      </label>
    </div>
    <h2>盒子与分类 <em>扩展名=自动归类规则；留空的盒子=纯手动自由分组</em></h2>
    <div class="cat-head"><span>启用</span><span>色</span><span>名称</span><span>文件夹</span><span>扩展名</span><span>盒子</span><span></span></div>
    ${cats}
    <div class="row-btns">
      <button class="primary" id="btn-save">保存设置</button>
      <button id="btn-add-box">➕ 新建自定义盒子</button>
      <button id="btn-reset-layout">重置盒子布局</button>
    </div>
    <h2>排除规则 <em>每行一条，支持 * 与 ? 通配</em></h2>
    <textarea class="in" id="set-excludes" rows="4" spellcheck="false">${esc(config.excludes.join("\n"))}</textarea>
    <h2>关于</h2>
    <p class="about">PureDesk v0.2.1 —— 轻量、纯净、本地的桌面整理工具。<br>
    文件实际收纳在「存储位置」下的盒子文件夹中，收纳盒只是它们的漂亮入口；关闭软件文件也不会丢。<br>
    自定义盒子：手动拖入/移入快捷方式与文件，自由命名分组（如 工作 / 开发 / 娱乐）；重命名后文件自动跟随迁移。<br>
    「还原到桌面」会绕过自动整理 6 秒，防止刚还原的文件被立即搬回。</p>`;

  document.getElementById("btn-save")!.addEventListener("click", () => void saveSettings());
  document.getElementById("btn-reset-layout")!.addEventListener("click", async () => {
    await api.resetBoxLayout();
    toast("盒子布局已重置");
  });
  document.getElementById("btn-add-box")!.addEventListener("click", () => openCreateBoxModal());
  page.querySelectorAll<HTMLButtonElement>("button[data-del]:not([disabled])").forEach((b) => {
    b.addEventListener("click", () => {
      const id = b.dataset.del!;
      void api
        .deleteBox(id)
        .then(async (r) => {
          toast(r.moved.length > 0 ? `已还原 ${r.moved.length} 项到桌面并删除盒子` : "已删除空盒子");
          await refresh();
          renderSettings();
        })
        .catch((e) => toast(`删除失败: ${e}`));
    });
  });
}

async function saveSettings(): Promise<void> {
  if (!config) return;
  const page = document.getElementById("page-settings")!;
  const cfg: Config = JSON.parse(JSON.stringify(config));
  cfg.storageRoot = (page.querySelector<HTMLInputElement>("#set-storage")!.value.trim() || null);
  cfg.autoOrganize = page.querySelector<HTMLInputElement>("#set-auto")!.checked;
  cfg.organizePublicDesktop = page.querySelector<HTMLInputElement>("#set-public")!.checked;
  cfg.hideStorageFolder = page.querySelector<HTMLInputElement>("#set-hide-folder")!.checked;
  cfg.closeAction = page.querySelector<HTMLSelectElement>("#set-close")!.value === "exit" ? "exit" : "minimize";
  const wantAutostart = page.querySelector<HTMLInputElement>("#set-autostart")!.checked;
  cfg.excludes = page
    .querySelector<HTMLTextAreaElement>("#set-excludes")!.value
    .split(/\r?\n/)
    .map((s) => s.trim())
    .filter(Boolean);
  for (const row of page.querySelectorAll<HTMLElement>(".cat-row")) {
    const c = cfg.categories.find((x) => x.id === row.dataset.cat);
    if (!c) continue;
    c.enabled = row.querySelector<HTMLInputElement>('[data-f="enabled"]')!.checked;
    c.color = row.querySelector<HTMLInputElement>('[data-f="color"]')!.value || c.color;
    c.name = row.querySelector<HTMLInputElement>('[data-f="name"]')!.value.trim() || c.id;
    c.folder = row.querySelector<HTMLInputElement>('[data-f="folder"]')!.value.trim() || c.id;
    const extsRaw = row.querySelector<HTMLInputElement>('[data-f="exts"]')!.value;
    if (!row.querySelector<HTMLInputElement>('[data-f="exts"]')!.disabled) {
      c.exts = extsRaw
        .split(/[,,\s、]+/)
        .map((s) => s.replace(/^\./, "").trim().toLowerCase())
        .filter(Boolean);
    }
    c.showBox = row.querySelector<HTMLInputElement>('[data-f="showBox"]')!.checked;
  }
  try {
    await api.saveConfig(cfg);
    if (wantAutostart !== autostart) {
      await api.setAutostart(wantAutostart);
      autostart = wantAutostart;
    }
    toast("设置已保存");
    await refresh();
  } catch (e) {
    toast(`保存失败: ${e}`);
  }
}

function setStatus(s: string): void {
  const el = document.getElementById("status");
  if (el) el.textContent = s;
}

let toastTimer = 0;
function toast(msg: string): void {
  const el = document.getElementById("toast");
  if (!el) return;
  el.textContent = msg;
  el.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => el.classList.remove("show"), 2600);
}

// ── 新建盒子模态：命名 + 扩展名预设辅助决策 + 立即收纳 ──
const BOX_PRESETS: { label: string; exts: string[] }[] = [
  { label: "📄 文档", exts: ["doc", "docx", "pdf", "txt", "md", "xls", "xlsx", "ppt", "pptx", "csv"] },
  { label: "🖼 图片", exts: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] },
  { label: "🎬 视频", exts: ["mp4", "mkv", "avi", "mov"] },
  { label: "🎵 音乐", exts: ["mp3", "flac", "wav", "aac", "m4a"] },
  { label: "🗜 压缩包", exts: ["zip", "rar", "7z", "tar", "gz"] },
  { label: "🌐 网址/快捷方式", exts: ["url", "lnk"] },
];

function openCreateBoxModal(): void {
  const selected = new Set<string>();
  const overlay = document.createElement("div");
  overlay.className = "modal-overlay";
  overlay.innerHTML = `
    <div class="modal-box" style="width: 380px;">
      <div class="modal-title">新建盒子</div>
      <input class="in modal-input" id="cb-name" type="text" placeholder="盒子名称，如：工作 / 开发 / 娱乐" spellcheck="false">
      <div class="cb-hint">选择扩展名后，桌面上的同类文件会自动归入此盒（可多选，留空则纯手动整理）</div>
      <div class="cb-chips" id="cb-chips">
        ${BOX_PRESETS.map(
          (p, i) => `<button class="chip-t" data-i="${i}">${p.label}</button>`,
        ).join("")}
      </div>
      <div class="cb-exts" id="cb-exts"></div>
      <label class="switch" style="margin-top: 10px;">
        <input type="checkbox" id="cb-auto" checked><i></i><span>创建后立即收纳桌面同类文件</span>
      </label>
      <div class="modal-btns">
        <button data-act="cancel">取消</button>
        <button data-act="ok" class="primary">创建</button>
      </div>
    </div>`;
  document.body.appendChild(overlay);
  const nameInput = overlay.querySelector<HTMLInputElement>("#cb-name")!;
  nameInput.focus();

  const renderExts = (): void => {
    overlay.querySelector<HTMLDivElement>("#cb-exts")!.textContent = selected.size
      ? [...selected].map((e) => "." + e).join("  ")
      : "（纯手动整理，不自动归类）";
  };
  overlay.querySelectorAll<HTMLButtonElement>(".chip-t").forEach((chip) => {
    chip.addEventListener("click", () => {
      const preset = BOX_PRESETS[Number(chip.dataset.i)];
      const on = chip.classList.toggle("on");
      for (const e of preset.exts) {
        if (on) selected.add(e);
        else selected.delete(e);
      }
      renderExts();
    });
  });

  const close = (): void => {
    overlay.remove();
    document.removeEventListener("keydown", onKey, true);
  };
  const ok = (): void => {
    const name = nameInput.value.trim();
    if (!name) {
      nameInput.focus();
      return;
    }
    close();
    void api
      .createBox(name, [...selected], overlay.querySelector<HTMLInputElement>("#cb-auto")!.checked)
      .then(async () => {
        toast(`盒子「${name}」已创建`);
        await refresh();
        renderSettings();
      })
      .catch((e) => toast(`创建失败: ${e}`));
  };
  const onKey = (e: KeyboardEvent): void => {
    if (e.key === "Enter") ok();
    else if (e.key === "Escape") close();
  };
  overlay.addEventListener("click", (e) => {
    const t = (e.target as HTMLElement).dataset.act;
    if (t === "ok") ok();
    if (t === "cancel" || e.target === overlay) close();
  });
  document.addEventListener("keydown", onKey, true);
}
