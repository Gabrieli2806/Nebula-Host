/**
 * The Windows tray panel: what is running on this PC, at a glance, with the
 * action each server can take. It lives in its own hidden window (src-tauri
 * tray.rs) and shows itself once measured. It calls the backend directly: the
 * main window may be managing another machine, but the tray means this PC.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { I18nProvider, useI18n } from "@/i18n";
import type { InstanceStatus, InstanceView } from "@/types/api";
import { Icon, type IconName } from "@/ui";

import "./TrayPanel.css";

const running = (status: InstanceStatus) => status === "online" || status === "starting";

/** How many servers the panel lists; the rest are in the app. */
const MAX_SHOWN = 6;

/** Those that need attention first (up, or changing, or crashed), then the most recently started. */
function arranged(servers: InstanceView[]): InstanceView[] {
  const active = (server: InstanceView) => (server.status === "offline" || server.status === "sleeping" ? 1 : 0);
  return [...servers].sort(
    (a, b) => active(a) - active(b) || (b.last_started_at ?? "").localeCompare(a.last_started_at ?? "") || a.name.localeCompare(b.name),
  );
}

function Item({ icon, label, detail, onClick }: { icon: IconName; label: string; detail?: string; onClick: () => void }) {
  return (
    <button type="button" className="tray-item" onClick={onClick}>
      <Icon name={icon} size={15} />
      <span className="tray-item-label">{label}</span>
      {detail ? <span className="tray-item-detail">{detail}</span> : null}
    </button>
  );
}

function Panel() {
  const { t } = useI18n();
  const [servers, setServers] = useState<InstanceView[] | null>(null);
  const root = useRef<HTMLDivElement>(null);
  // Only the first time: after that the backend shows it on the click itself.
  const showNext = useRef(true);

  const refresh = useCallback(async () => {
    setServers(await invoke<InstanceView[]>("instances_list"));
  }, []);

  useEffect(() => {
    void refresh();
    const open = listen("nebula:tray-open", () => void refresh());
    const changes = listen<{ type: string }>("nebula:event", (event) => {
      if (event.payload.type === "status") void refresh();
    });
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") void invoke("tray_panel_hide");
    };
    window.addEventListener("keydown", escape);
    return () => {
      window.removeEventListener("keydown", escape);
      void open.then((stop) => stop());
      void changes.then((stop) => stop());
    };
  }, [refresh]);

  // Every refresh is a new list: measure it.
  useEffect(() => {
    if (!servers || !root.current) return;
    void invoke("tray_panel_fit", { height: root.current.offsetHeight, show: showNext.current });
    showNext.current = false;
  }, [servers]);

  const hideThen = (command: string, args?: Record<string, unknown>) => {
    void invoke("tray_panel_hide");
    void invoke(command, args);
  };

  const list = servers ?? [];
  const online = list.filter((server) => running(server.status)).length;
  const shown = arranged(list).slice(0, MAX_SHOWN);

  return (
    <div ref={root} className="tray-panel">
      <header className="tray-head">
        <strong>{online ? t("tray.summary", { online, total: list.length }) : t("tray.summaryNone")}</strong>
      </header>

      <div className="tray-divider" />

      {list.length === 0 ? <p className="tray-empty">{t("tray.none")}</p> : null}
      <ul className="tray-servers">
        {shown.map((server) => {
          const up = running(server.status);
          const busy = server.status === "stopping";
          return (
            <li key={server.id} className="tray-server">
              <button
                type="button"
                className="tray-server-open"
                title={t("tray.openServer", { name: server.name })}
                onClick={() => hideThen("tray_open", { settings: false, instanceId: server.id })}
              >
                <span className="tray-dot" data-status={server.status} aria-hidden="true" />
                <span className="tray-server-text">
                  <span className="tray-server-name">{server.name}</span>
                  <span className="tray-server-meta">
                    {t(`status.${server.status}`)}
                    {server.status === "online" ? ` · ${server.players_online}/${server.players_max}` : ""}
                  </span>
                </span>
              </button>
              {up ? (
                <button type="button" className="tray-action" title={t("actions.restart")} aria-label={t("actions.restart")} onClick={() => void invoke("tray_server_action", { action: "restart", instanceId: server.id })}>
                  <Icon name="restart" size={14} />
                </button>
              ) : null}
              <button
                type="button"
                className="tray-action"
                disabled={busy}
                title={t(up ? "actions.stop" : "actions.start")}
                aria-label={t(up ? "actions.stop" : "actions.start")}
                onClick={() => void invoke("tray_server_action", { action: up ? "stop" : "start", instanceId: server.id })}
              >
                <Icon name={up ? "stop" : "play"} size={14} />
              </button>
            </li>
          );
        })}
      </ul>

      {list.length > shown.length ? (
        <Item icon="external" label={t("tray.seeAll", { count: list.length })} onClick={() => hideThen("tray_open", { settings: false })} />
      ) : null}

      <div className="tray-divider" />

      {online ? <Item icon="stop" label={t("tray.stopAll")} onClick={() => void invoke("tray_server_action", { action: "stop_all", instanceId: "" })} /> : null}
      <Item icon="sliders" label={t("tray.settings")} onClick={() => hideThen("tray_open", { settings: true })} />
      <Item icon="external" label={t("tray.open")} onClick={() => hideThen("tray_open", { settings: false })} />
      <Item icon="power" label={t("tray.quit")} onClick={() => void invoke("tray_quit")} />
    </div>
  );
}

export function TrayPanel() {
  return (
    <I18nProvider>
      <Panel />
    </I18nProvider>
  );
}
