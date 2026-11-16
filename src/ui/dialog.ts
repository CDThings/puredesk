// 轻量模态输入对话框（重命名等场景），返回 Promise<string | null>
import { esc } from "../util";

export function promptText(title: string, initial: string): Promise<string | null> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    overlay.innerHTML = `
      <div class="modal-box">
        <div class="modal-title">${esc(title)}</div>
        <input class="in modal-input" type="text" spellcheck="false">
        <div class="modal-btns">
          <button data-act="cancel">取消</button>
          <button data-act="ok" class="primary">确定</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    const input = overlay.querySelector<HTMLInputElement>(".modal-input")!;
    input.value = initial;
    const dot = initial.lastIndexOf(".");
    input.focus();
    input.setSelectionRange(0, dot > 0 ? dot : initial.length);

    const close = (value: string | null): void => {
      overlay.remove();
      document.removeEventListener("keydown", onKey, true);
      resolve(value);
    };
    const ok = (): void => {
      const v = input.value.trim();
      close(v || null);
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Enter") ok();
      else if (e.key === "Escape") close(null);
    };
    overlay.addEventListener("click", (e) => {
      const t = (e.target as HTMLElement).dataset.act;
      if (t === "ok") ok();
      if (t === "cancel" || e.target === overlay) close(null);
    });
    document.addEventListener("keydown", onKey, true);
  });
}

export function confirmText(message: string, okLabel = "确定"): Promise<boolean> {
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    overlay.innerHTML = `
      <div class="modal-box">
        <div class="modal-title">${esc(message)}</div>
        <div class="modal-btns">
          <button data-act="cancel">取消</button>
          <button data-act="ok" class="primary">${esc(okLabel)}</button>
        </div>
      </div>`;
    document.body.appendChild(overlay);
    const close = (v: boolean): void => {
      overlay.remove();
      document.removeEventListener("keydown", onKey, true);
      resolve(v);
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") close(false);
    };
    overlay.addEventListener("click", (e) => {
      const t = (e.target as HTMLElement).dataset.act;
      if (t === "ok") close(true);
      if (t === "cancel" || e.target === overlay) close(false);
    });
    document.addEventListener("keydown", onKey, true);
  });
}
