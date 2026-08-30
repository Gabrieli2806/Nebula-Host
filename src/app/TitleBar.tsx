/**
 * The window's own title bar.
 *
 * The window is undecorated, so this is both chrome and identity: the machine
 * being hosted from, not a user avatar. There is no account to show.
 */

import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";

import { useI18n } from "@/i18n";
import { useNode } from "@/hooks/useBackend";
import { Icon } from "@/ui";

export function TitleBar() {
  const { t } = useI18n();
  const { data: node } = useNode();
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const window = getCurrentWindow();
    void window.isMaximized().then(setMaximized);
    const unlisten = window.onResized(() => {
      void window.isMaximized().then(setMaximized);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  const window = getCurrentWindow();

  return (
    <header
      className="drag-region flex flex-none items-center justify-between border-b border-[color:var(--line)] pl-4"
      style={{ height: "var(--titlebar-height)", background: "var(--surface-sunken)" }}
    >
      <div className="flex items-center gap-2.5">
        <Mark />
        <span className="text-[length:var(--text-sm)] font-semibold">{t("app.name")}</span>
        {node ? (
          <>
            <span className="text-[color:var(--text-faint)]">/</span>
            <span className="text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
              {node.name}
            </span>
          </>
        ) : null}
      </div>

      <div className="no-drag flex items-center">
        <WindowButton label={t("app.minimize")} onClick={() => void window.minimize()}>
          <Icon name="minimize" size={14} />
        </WindowButton>
        <WindowButton label={t("app.maximize")} onClick={() => void window.toggleMaximize()}>
          <Icon name={maximized ? "archive" : "maximize"} size={12} />
        </WindowButton>
        <WindowButton label={t("app.close")} danger onClick={() => void window.close()}>
          <Icon name="close" size={14} />
        </WindowButton>
      </div>
    </header>
  );
}

function WindowButton({
  label,
  onClick,
  danger = false,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      onClick={onClick}
      className="flex h-[var(--titlebar-height)] w-[46px] items-center justify-center text-[color:var(--text-muted)] transition-colors hover:text-[color:var(--text)]"
      style={{ ["--hover" as string]: danger ? "var(--status-crashed)" : "var(--ink-700)" }}
      onMouseEnter={(event) => {
        event.currentTarget.style.background = danger
          ? "var(--status-crashed)"
          : "var(--ink-700)";
        if (danger) event.currentTarget.style.color = "#fff";
      }}
      onMouseLeave={(event) => {
        event.currentTarget.style.background = "transparent";
        event.currentTarget.style.color = "";
      }}
    >
      {children}
    </button>
  );
}

/** The mark: an ellipse and a point, drawn once and used everywhere. */
export function Mark({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 26 26" fill="none" aria-hidden="true">
      <circle cx="13" cy="13" r="3.2" fill="var(--accent)" />
      <ellipse
        cx="13"
        cy="13"
        rx="11.2"
        ry="5"
        stroke="var(--accent)"
        strokeOpacity="0.55"
        strokeWidth="1.1"
        transform="rotate(-22 13 13)"
      />
      <ellipse
        cx="13"
        cy="13"
        rx="7.4"
        ry="10.8"
        stroke="var(--accent)"
        strokeOpacity="0.28"
        strokeWidth="1.1"
        transform="rotate(14 13 13)"
      />
    </svg>
  );
}
