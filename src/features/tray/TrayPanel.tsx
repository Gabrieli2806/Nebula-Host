/**
 * The Windows tray panel: what is running on this PC and on the machines it
 * is paired with, at a glance, with the action each server can take. It
 * lives in its own hidden window (src-tauri tray.rs) and shows itself once
 * measured. This PC is asked directly, whichever machine the main window is
 * managing; paired machines over their own API, like the main window does.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { I18nProvider, useI18n } from "@/i18n";
import { activeRemote, remoteCall, remoteNodes, setActiveNode, type RemoteNode } from "@/lib/connection";
import type { InstanceStatus, InstanceView } from "@/types/api";
import { Icon, type IconName } from "@/ui";

import "./TrayPanel.css";

const running = (status: InstanceStatus) => status === "online" || status === "starting";

/** How many servers the panel lists; the rest are in the app. */
const MAX_SHOWN = 6;

/** A server and the machine it is on (null: this PC). */
interface Entry {
  server: InstanceView;
  node: RemoteNode | null;
}

/** Those that need attention first (up, or changing, or crashed), then the most recently started. */
function arranged(entries: Entry[]): Entry[] {
  const idle = ({ server }: Entry) => (server.status === "offline" || server.status === "sleeping" ? 1 : 0);
  return [...entries].sort(
    (a, b) =>
      idle(a) - idle(b) ||
      (b.server.last_started_at ?? "").localeCompare(a.server.last_started_at ?? "") ||
      a.server.name.localeCompare(b.server.name),
  );
}

function Item({ icon, label, onClick }: { icon: IconName; label: string; onClick: () => void }) {
  return (
    <button type="button" className="tray-item" onClick={onClick}>
      <Icon name={icon} size={15} />
      <span className="tray-item-label">{label}</span>
    </button>
  );
}

function Panel() {
  const { t } = useI18n();
  const [local, setLocal] = useState<Entry[] | null>(null);
  const [remote, setRemote] = useState<Entry[]>([]);
  const [unreachable, setUnreachable] = useState<string[]>([]);
  const root = useRef<HTMLDivElement>(null);
  // Only the first time: after that the backend shows it on the click itself.
  const showNext = useRef(true);

  const refreshLocal = useCallback(async () => {
    const list = await invoke<InstanceView[]>("instances_list");
    setLocal(list.map((server) => ({ server, node: null })));
  }, []);

  // Paired machines answer at their own pace; this PC is not kept waiting.
  const refreshRemote = useCallback(async () => {
    const nodes = remoteNodes();
    const answers = await Promise.allSettled(
      nodes.map((node) => remoteCall<InstanceView[]>(node, "instances_list").then((list) => list.map((server) => ({ server, node })))),
    );
    setRemote(answers.flatMap((answer) => (answer.status === "fulfilled" ? answer.value : [])));
    setUnreachable(nodes.filter((_, index) => answers[index]?.status === "rejected").map((node) => node.name));
  }, []);

  useEffect(() => {
    void refreshLocal();
    void refreshRemote();
    const open = listen("nebula:tray-open", () => {
      void refreshLocal();
      void refreshRemote();
    });
    const changes = listen<{ type: string }>("nebula:event", (event) => {
      if (event.payload.type === "status") void refreshLocal();
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
  }, [refreshLocal, refreshRemote]);

  // Every refresh is a new list: measure it.
  useEffect(() => {
    if (!local || !root.current) return;
    void invoke("tray_panel_fit", { height: root.current.offsetHeight, show: showNext.current });
    showNext.current = false;
  }, [local, remote, unreachable]);

  const hideThen = (command: string, args?: Record<string, unknown>) => {
    void invoke("tray_panel_hide");
    void invoke(command, args);
  };

  const act = ({ server, node }: Entry, action: "start" | "stop" | "restart") => {
    if (!node) {
      void invoke("tray_server_action", { action, instanceId: server.id });
      return;
    }
    // A paired machine sends no events here: look again once it has moved.
    void remoteCall(node, `instance_${action}`, { instanceId: server.id })
      .catch(() => undefined)
      .then(() => window.setTimeout(() => void refreshRemote(), 1500));
  };

  /** The main window shows the machine the server is on, switching to it if needed. */
  const open = ({ server, node }: Entry) => {
    const target = node?.id ?? "local";
    const reload = (activeRemote()?.id ?? "local") !== target;
    if (reload) setActiveNode(target);
    hideThen("tray_open", { settings: false, instanceId: server.id, remote: node !== null, reload });
  };

  const all = [...(local ?? []), ...remote];
  const online = all.filter(({ server }) => running(server.status)).length;
  const localOnline = (local ?? []).some(({ server }) => running(server.status));
  const shown = arranged(all).slice(0, MAX_SHOWN);

  return (
    <div ref={root} className="tray-panel">
      <header className="tray-head">
        <strong>{online ? t("tray.summary", { online, total: all.length }) : t("tray.summaryNone")}</strong>
      </header>

      <div className="tray-divider" />

      {all.length === 0 ? <p className="tray-empty">{t("tray.none")}</p> : null}
      <ul className="tray-servers">
        {shown.map((entry) => {
          const { server, node } = entry;
          const up = running(server.status);
          const busy = server.status === "stopping";
          return (
            <li key={`${node?.id ?? "local"}:${server.id}`} className="tray-server">
              <button type="button" className="tray-server-open" title={t("tray.openServer", { name: server.name })} onClick={() => open(entry)}>
                <span className="tray-dot" data-status={server.status} aria-hidden="true" />
                <span className="tray-server-text">
                  <span className="tray-server-name">{server.name}</span>
                  <span className="tray-server-meta">
                    {t(`status.${server.status}`)}
                    {server.status === "online" ? ` · ${server.players_online}/${server.players_max}` : ""}
                    {node ? ` · ${node.name}` : ""}
                  </span>
                </span>
              </button>
              {up ? (
                <button type="button" className="tray-action" title={t("actions.restart")} aria-label={t("actions.restart")} onClick={() => act(entry, "restart")}>
                  <Icon name="restart" size={14} />
                </button>
              ) : null}
              <button
                type="button"
                className="tray-action"
                disabled={busy}
                title={t(up ? "actions.stop" : "actions.start")}
                aria-label={t(up ? "actions.stop" : "actions.start")}
                onClick={() => act(entry, up ? "stop" : "start")}
              >
                <Icon name={up ? "stop" : "play"} size={14} />
              </button>
            </li>
          );
        })}
      </ul>

      {all.length > shown.length ? (
        <Item icon="external" label={t("tray.seeAll", { count: all.length })} onClick={() => hideThen("tray_open", { settings: false })} />
      ) : null}
      {unreachable.map((name) => (
        <p key={name} className="tray-empty">{t("tray.unreachable", { name })}</p>
      ))}

      <div className="tray-divider" />

      {localOnline ? <Item icon="stop" label={t("tray.stopAll")} onClick={() => void invoke("tray_server_action", { action: "stop_all", instanceId: "" })} /> : null}
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
