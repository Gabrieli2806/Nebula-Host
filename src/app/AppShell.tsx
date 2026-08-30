/**
 * The frame every screen sits in.
 *
 * One sidebar, and it belongs to whatever you are looking at: the app's own
 * navigation on the server list, and **the server's own navigation** once you
 * open one — its name, its state, its start button, then its panels. The node
 * panel stays pinned at the bottom throughout, because the machine is the
 * identity here and it never stops being relevant.
 */

import { useEffect } from "react";
import { NavLink, Outlet, useMatch, useNavigate } from "react-router-dom";

import { useBackendEvents, useInstance, useInstanceActions, useInstances, useNode, useNodeHealth } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatMegabytes } from "@/lib/format";
import { Button, Icon, Ring, StatusDot, type IconName } from "@/ui";
import { TitleBar } from "./TitleBar";

/** The panels a server has, in the order they are worked through. */
export const SERVER_PANELS: { id: string; icon: IconName; labelKey: string; group: "operate" | "manage" }[] = [
  { id: "overview", icon: "servers", labelKey: "overview.title", group: "operate" },
  { id: "console", icon: "terminal", labelKey: "console.title", group: "operate" },
  { id: "files", icon: "folder", labelKey: "files.title", group: "operate" },
  { id: "players", icon: "users", labelKey: "players.title", group: "manage" },
  { id: "addons", icon: "puzzle", labelKey: "addons.title", group: "manage" },
  { id: "backups", icon: "archive", labelKey: "backups.title", group: "manage" },
  { id: "settings", icon: "sliders", labelKey: "settings.title", group: "manage" },
];

export function AppShell() {
  const navigate = useNavigate();
  const { data: node, isSuccess } = useNode();
  const { data: instances } = useInstances();
  const match = useMatch("/servers/:instanceId/:panel");
  const instanceId = match?.params.instanceId ?? null;

  useBackendEvents();

  useEffect(() => {
    // A node that has never been named and has no servers is a first run.
    if (isSuccess && node && instances?.length === 0 && !node.paired_devices.length) {
      const seen = localStorage.getItem("nebula.firstRunDone");
      if (!seen) navigate("/welcome", { replace: true });
    }
  }, [isSuccess, node, instances, navigate]);

  return (
    <div className="flex h-full flex-col">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        <nav
          className="flex flex-none flex-col gap-3 overflow-y-auto border-r border-[color:var(--line)] p-3"
          style={{ width: "var(--sidebar-width)", background: "var(--surface-sunken)" }}
        >
          {instanceId ? <ServerNav instanceId={instanceId} /> : <AppNav />}
          <div className="flex-1" />
          <NodePanel />
        </nav>

        <main className="min-w-0 flex-1 overflow-auto" style={{ background: "var(--canvas)" }}>
          <Outlet />
        </main>
      </div>
    </div>
  );
}

function AppNav() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-0.5">
      <NavItem to="/servers" icon="servers" label={t("nav.servers")} />
      <NavItem to="/settings" icon="sliders" label={t("nav.settings")} />
    </div>
  );
}

/** The sidebar a server owns while it is open. */
function ServerNav({ instanceId }: { instanceId: string }) {
  const { t } = useI18n();
  const instance = useInstance(instanceId);
  const actions = useInstanceActions();

  if (!instance) return <AppNav />;

  const running = instance.status === "online" || instance.status === "starting";
  const busy = actions.start.isPending || actions.stop.isPending;

  return (
    <div className="flex flex-col gap-4">
      <NavLink
        to="/servers"
        className="flex items-center gap-2 px-1 text-[length:var(--text-xs)] text-[color:var(--text-faint)] hover:text-[color:var(--text)]"
      >
        <Icon name="chevron-left" size={13} />
        {t("nav.backToServers")}
      </NavLink>

      <div className="flex flex-col gap-3 rounded-[var(--radius-lg)] border border-[color:var(--line)] p-3">
        <div className="flex items-start gap-2.5">
          <span className="mt-1.5">
            <StatusDot status={instance.status} />
          </span>
          <div className="min-w-0">
            <p className="truncate font-semibold leading-tight">{instance.name}</p>
            <p className="text-[length:var(--text-xs)] text-[color:var(--text-muted)]">
              {t(`status.${instance.status}`)}
            </p>
          </div>
        </div>

        <div className="flex gap-2">
          {running ? (
            <Button
              size="sm"
              icon="stop"
              className="flex-1"
              disabled={busy}
              onClick={() => actions.stop.mutate(instance.id)}
            >
              {t("actions.stop")}
            </Button>
          ) : (
            <Button
              size="sm"
              variant="primary"
              icon="play"
              className="flex-1"
              disabled={busy}
              onClick={() => actions.start.mutate(instance.id)}
            >
              {instance.status === "sleeping" ? t("actions.wake") : t("actions.start")}
            </Button>
          )}
          {running ? (
            <Button
              size="sm"
              icon="restart"
              iconOnly
              disabled={busy}
              onClick={() => actions.restart.mutate(instance.id)}
            >
              {t("actions.restart")}
            </Button>
          ) : null}
        </div>
      </div>

      {(["operate", "manage"] as const).map((group) => (
        <div key={group} className="flex flex-col gap-0.5">
          <p className="label px-3 pb-1">{t(`nav.${group}`)}</p>
          {SERVER_PANELS.filter((panel) => panel.group === group).map((panel) => (
            <NavItem
              key={panel.id}
              to={`/servers/${instanceId}/${panel.id}`}
              icon={panel.icon}
              label={t(panel.labelKey)}
            />
          ))}
        </div>
      ))}
    </div>
  );
}

function NavItem({ to, icon, label }: { to: string; icon: IconName; label: string }) {
  return (
    <NavLink
      to={to}
      className="flex h-9 items-center gap-3 rounded-[var(--radius)] px-3 text-[length:var(--text-sm)] transition-colors"
      style={({ isActive }) =>
        isActive
          ? { background: "var(--accent-dim)", color: "var(--accent)", fontWeight: 600 }
          : { color: "var(--text-muted)" }
      }
    >
      <Icon name={icon} />
      {label}
    </NavLink>
  );
}

/** The machine, its load, and how to reach it. */
function NodePanel() {
  const { t } = useI18n();
  const { data: node } = useNode();
  const { data: health } = useNodeHealth();

  return (
    <div className="panel flex flex-none flex-col gap-3 p-3.5">
      <div className="flex items-center justify-between">
        <span className="label">{t("node.thisMachine")}</span>
        <span
          className="h-1.5 w-1.5 rounded-full"
          style={{ background: health ? "var(--status-online)" : "var(--status-offline)" }}
        />
      </div>

      <p className="truncate text-[length:var(--text-sm)] font-medium">{node?.name ?? "…"}</p>

      {health ? (
        <div className="flex flex-col gap-3">
          <div className="flex items-start justify-between gap-1">
            <Ring label={t("node.cpu")} value={health.cpu_percent} />
            <Ring
              label={t("node.memory")}
              value={health.memory_used_mb}
              max={health.memory_total_mb}
              detail={formatMegabytes(health.memory_total_mb)}
            />
            <Ring
              label={t("node.disk")}
              value={health.disk_used_gb}
              max={health.disk_total_gb}
              detail={`${health.disk_total_gb} GB`}
              colour="var(--accent-line)"
            />
          </div>
          <div className="flex items-center justify-between pt-0.5 text-[length:var(--text-xs)]">
            <span className="text-[color:var(--text-faint)]">{t("node.address")}</span>
            <span className="mono selectable text-[color:var(--text-muted)]">
              {health.lan_address ?? t("node.offline")}
            </span>
          </div>
        </div>
      ) : null}
    </div>
  );
}
