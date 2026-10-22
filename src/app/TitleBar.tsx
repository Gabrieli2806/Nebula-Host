/**
 * The window's own title bar.
 *
 * The window is undecorated, so this is both chrome and identity: the orbit
 * mark, the wordmark, and the machine being hosted from. There is no account to show.
 *
 * Hover on the window keys is pure CSS (`.window-button` in global.css); close
 * follows the Windows convention so it never borrows a status colour.
 *
 * When the frame has a sidebar (AppShell, not first run), the sidebar's
 * collapse toggle sits at the very left, above the rail it controls.
 */

import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState, type ReactNode } from "react";

import { useI18n } from "@/i18n";
import { IS_MOBILE, IS_WEB, signOutWeb, webSession } from "@/lib/web";
import { Icon, Mark } from "@/ui";

import { NodeSwitcher } from "./NodeSwitcher";
import "@/features/web/WebPair.css";

export interface SidebarToggle {
  collapsed: boolean;
  onToggle: () => void;
  /** id of the sidebar element, for aria-controls. */
  controls: string;
}

export function TitleBar({ sidebar }: { sidebar?: SidebarToggle } = {}) {
  if (IS_WEB) return <WebTitleBar sidebar={sidebar} />;
  if (IS_MOBILE) return <MobileTitleBar sidebar={sidebar} />;
  return <WindowTitleBar sidebar={sidebar} />;
}

/** The web panel's bar: brand, the machine it manages, and sign out. */
function WebTitleBar({ sidebar }: { sidebar?: SidebarToggle }) {
  const { t } = useI18n();
  const session = webSession();
  return (
    <header className="titlebar titlebar-web">
      <div className="titlebar-brand">
        {sidebar ? <SidebarButton {...sidebar} /> : null}
        <Mark size={16} />
        <span className="wordmark">{t("app.name")}</span>
        {session ? (
          <>
            <span className="titlebar-sep" aria-hidden="true">
              /
            </span>
            <span className="titlebar-web-node">{session.name}</span>
          </>
        ) : null}
      </div>
      {session ? (
        <div className="flex flex-none items-center pr-2">
          <button type="button" className="titlebar-web-signout" onClick={signOutWeb}>
            <Icon name="external" size={13} />
            <span>{t("web.signOut")}</span>
          </button>
        </div>
      ) : null}
    </header>
  );
}

/** The phone app: no window to move or close, the machine switcher stays. */
function MobileTitleBar({ sidebar }: { sidebar?: SidebarToggle }) {
  const { t } = useI18n();
  return (
    <header className="titlebar titlebar-mobile">
      <div className="titlebar-brand">
        {sidebar ? <SidebarButton {...sidebar} /> : null}
        <Mark size={16} />
        <span className="wordmark">{t("app.name")}</span>
        <span className="titlebar-sep" aria-hidden="true">
          /
        </span>
        <NodeSwitcher />
      </div>
    </header>
  );
}

function WindowTitleBar({ sidebar }: { sidebar?: SidebarToggle }) {
  const { t } = useI18n();
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
    <header className="titlebar drag-region">
      <div className="titlebar-brand">
        {sidebar ? <SidebarButton {...sidebar} /> : null}
        <Mark size={16} />
        <span className="wordmark">{t("app.name")}</span>
        {/* Always there: the way to another machine must not depend on the
            one being managed answering. */}
        <span className="titlebar-sep" aria-hidden="true">
          /
        </span>
        <NodeSwitcher />
      </div>

      <div className="no-drag flex flex-none items-stretch">
        <WindowButton label={t("app.minimize")} onClick={() => void window.minimize()}>
          <Icon name="minimize" size={14} />
        </WindowButton>
        <WindowButton
          label={maximized ? t("app.restore") : t("app.maximize")}
          onClick={() => void window.toggleMaximize()}
        >
          <Icon name={maximized ? "restore" : "maximize"} size={maximized ? 14 : 12} />
        </WindowButton>
        <WindowButton label={t("app.close")} variant="close" onClick={() => void window.close()}>
          <Icon name="close" size={14} />
        </WindowButton>
      </div>
    </header>
  );
}

function SidebarButton({ collapsed, onToggle, controls }: SidebarToggle) {
  const { t } = useI18n();
  const label = collapsed ? t("nav.expandSidebar") : t("nav.collapseSidebar");
  return (
    <button
      type="button"
      className="titlebar-toggle no-drag"
      aria-label={label}
      aria-expanded={!collapsed}
      aria-controls={controls}
      aria-keyshortcuts="Control+B"
      title={`${label} (Ctrl+B)`}
      onClick={onToggle}
    >
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
        <rect x="2.5" y="3" width="11" height="10" rx="2" stroke="currentColor" strokeWidth="1.3" />
        <path d="M6.5 3v10" stroke="currentColor" strokeWidth="1.3" />
        {collapsed ? null : (
          <path d="M3.4 4h3.1v8H3.4z" fill="currentColor" opacity="0.35" />
        )}
      </svg>
    </button>
  );
}

function WindowButton({
  label,
  onClick,
  variant,
  children,
}: {
  label: string;
  onClick: () => void;
  variant?: "close";
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      className="window-button"
      data-variant={variant}
      aria-label={label}
      title={label}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
