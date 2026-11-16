// 轻量右键菜单
export interface MenuEntry {
  label?: string;
  danger?: boolean;
  action?: () => void;
  sep?: boolean;
}

let el: HTMLDivElement | null = null;

export function closeMenu(): void {
  if (el) {
    el.remove();
    el = null;
    document.removeEventListener("pointerdown", onOutside, true);
  }
}

function onOutside(ev: PointerEvent): void {
  if (el && !el.contains(ev.target as Node)) closeMenu();
}

export function openMenu(x: number, y: number, entries: MenuEntry[]): void {
  closeMenu();
  el = document.createElement("div");
  el.className = "ctx-menu";
  for (const e of entries) {
    if (e.sep) {
      const s = document.createElement("div");
      s.className = "ctx-sep";
      el.appendChild(s);
      continue;
    }
    const b = document.createElement("button");
    b.className = "ctx-item" + (e.danger ? " danger" : "");
    b.textContent = e.label ?? "";
    b.addEventListener("click", () => {
      closeMenu();
      e.action?.();
    });
    el.appendChild(b);
  }
  document.body.appendChild(el);
  const rect = el.getBoundingClientRect();
  el.style.left = `${Math.max(4, Math.min(x, window.innerWidth - rect.width - 8))}px`;
  el.style.top = `${Math.max(4, Math.min(y, window.innerHeight - rect.height - 8))}px`;
  document.addEventListener("pointerdown", onOutside, true);
  window.addEventListener("blur", closeMenu, { once: true });
}

// 全局：Escape / 右键空白处关闭
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeMenu();
});
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("[data-ctx]")) closeMenu();
});
