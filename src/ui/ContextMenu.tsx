/**
 * A right-click menu in the Nebula look, replacing the webview's own
 * (Back / Reload / Inspect…), which means nothing in a desktop app.
 *
 * `useContextMenu()` gives an `open(event, items)` to call from an
 * `onContextMenu` handler and the `menu` element to render once. Keyboard:
 * arrows move, Enter activates, Escape closes; clicking outside closes.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState, type MouseEvent, type ReactNode } from "react";

import { Icon, type IconName } from "./Icon";

export type MenuItem =
  | { kind?: "item"; label: string; icon?: IconName; danger?: boolean; disabled?: boolean; onSelect: () => void }
  | { kind: "separator" };

export function useContextMenu() {
  const [state, setState] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);

  const open = useCallback((event: MouseEvent, items: MenuItem[]) => {
    event.preventDefault();
    event.stopPropagation();
    setState({ x: event.clientX, y: event.clientY, items });
  }, []);
  const close = useCallback(() => setState(null), []);

  const menu: ReactNode = state ? <Menu x={state.x} y={state.y} items={state.items} onClose={close} /> : null;
  return { open, close, menu };
}

function Menu({ x, y, items, onClose }: { x: number; y: number; items: MenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });

  // Keep the menu inside the window.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    setPos({
      left: Math.min(x, window.innerWidth - width - 8),
      top: Math.min(y, window.innerHeight - height - 8),
    });
    el.querySelector<HTMLButtonElement>("button:not([disabled])")?.focus();
  }, [x, y]);

  useEffect(() => {
    const away = (event: Event) => {
      if (!ref.current?.contains(event.target as Node)) onClose();
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>("button:not([disabled])") ?? [])];
        const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const next = event.key === "ArrowDown" ? (at + 1) % buttons.length : (at - 1 + buttons.length) % buttons.length;
        buttons[next]?.focus();
      }
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("contextmenu", away);
    document.addEventListener("keydown", key);
    window.addEventListener("blur", onClose);
    window.addEventListener("resize", onClose);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("contextmenu", away);
      document.removeEventListener("keydown", key);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("resize", onClose);
    };
  }, [onClose]);

  return (
    <div ref={ref} className="context-menu" role="menu" style={{ left: pos.left, top: pos.top }}>
      {items.map((item, index) =>
        item.kind === "separator" ? (
          <div key={`sep-${index}`} className="context-menu-sep" role="separator" />
        ) : (
          <button
            key={item.label}
            type="button"
            role="menuitem"
            className="context-menu-item"
            data-danger={item.danger ? "" : undefined}
            disabled={item.disabled}
            onClick={() => {
              onClose();
              item.onSelect();
            }}
          >
            <span className="context-menu-icon" aria-hidden="true">
              {item.icon ? <Icon name={item.icon} size={14} /> : null}
            </span>
            {item.label}
          </button>
        ),
      )}
    </div>
  );
}

/**
 * Hide the webview's own menu everywhere except where it is useful: text
 * fields (cut/copy/paste) and selected text. Mount once at the app root.
 */
export function useSuppressNativeContextMenu() {
  useEffect(() => {
    const handler = (event: globalThis.MouseEvent) => {
      const target = event.target as HTMLElement | null;
      const editable = target?.closest("input, textarea, [contenteditable='true']");
      const hasSelection = (window.getSelection()?.toString() ?? "").length > 0;
      if (!editable && !hasSelection) event.preventDefault();
    };
    document.addEventListener("contextmenu", handler);
    return () => document.removeEventListener("contextmenu", handler);
  }, []);
}
