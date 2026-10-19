/**
 * Every machine at once: the servers of all of them in one list, or each
 * machine with its load (CPU, memory, disk), uptime and servers.
 *
 * Reached from the machine switcher in the title bar. Opening a server on
 * another machine switches the window to it, like choosing it in the menu.
 */

import { useMemo } from "react";
import { useSearchParams } from "react-router-dom";

import { useI18n } from "@/i18n";
import { formatDuration, formatMegabytes } from "@/lib/format";
import type { InstanceView } from "@/types/api";
import { Button, Icon, Meter, Segmented, ServerTile, StatusDot } from "@/ui";

import { manageMachine, openOnMachine, useFleet, type FleetEntry } from "./fleet";
import "./machines.css";

type View = "servers" | "machines";

export function MachinesScreen() {
  const { t } = useI18n();
  const [params, setParams] = useSearchParams();
  const view: View = params.get("view") === "machines" ? "machines" : "servers";
  const fleet = useFleet();

  return (
    <div className="machines-screen flex max-w-[var(--content-max)] flex-col gap-6 p-[var(--gutter)]">
      <header className="flex flex-wrap items-center justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="display servers-node">{view === "servers" ? t("fleet.allServers") : t("fleet.machines")}</h1>
          <p className="m-0 text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
            {t("fleet.subtitle", { count: fleet.length })}
          </p>
        </div>
        <Segmented<View>
          label={t("fleet.view")}
          value={view}
          onChange={(next) => setParams({ view: next }, { replace: true })}
          options={[
            { id: "servers", label: t("fleet.tabServers"), icon: "servers" },
            { id: "machines", label: t("fleet.tabMachines"), icon: "monitor" },
          ]}
        />
      </header>

      {view === "servers" ? <AllServers fleet={fleet} /> : <MachineCards fleet={fleet} />}
    </div>
  );
}

// -- every server on every machine -------------------------------------------

function AllServers({ fleet }: { fleet: FleetEntry[] }) {
  const { t } = useI18n();
  const rows = useMemo(
    () =>
      fleet.flatMap((machine) => (machine.instances ?? []).map((instance) => ({ machine, instance }))).sort((a, b) => {
        // Running first, then by name.
        const live = (i: InstanceView) => (i.status === "online" || i.status === "starting" ? 0 : 1);
        return live(a.instance) - live(b.instance) || a.instance.name.localeCompare(b.instance.name);
      }),
    [fleet],
  );
  const silent = fleet.filter((machine) => !machine.loading && !machine.reachable);

  return (
    <div className="flex flex-col gap-3">
      {silent.length ? (
        <p className="fleet-warning">
          <Icon name="warning" size={14} />
          {t("fleet.unreachable", { names: silent.map((m) => m.name).join(", ") })}
        </p>
      ) : null}
      <div className="panel-flush servers-table-card">
        <table className="servers-table fleet-table">
          <thead>
            <tr>
              <th scope="col" className="label">{t("servers.title")}</th>
              <th scope="col" className="label">{t("fleet.machine")}</th>
              <th scope="col" className="label">{t("servers.status")}</th>
              <th scope="col" className="label" data-numeric="">{t("servers.players")}</th>
              <th scope="col" className="label" data-numeric="" data-col="cpu">CPU</th>
              <th scope="col" className="label" data-numeric="">{t("node.memory")}</th>
              <th scope="col" className="label" data-numeric="" data-col="uptime">{t("fleet.uptime")}</th>
              <th scope="col" className="label" data-col="address">{t("fleet.address")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ machine, instance }) => {
              const live = instance.status === "online" || instance.status === "starting" || instance.status === "stopping";
              return (
                <tr
                  key={`${machine.key}:${instance.id}`}
                  className="server-row"
                  onDoubleClick={() => openOnMachine(machine, instance.id)}
                >
                  <td>
                    <span className="flex min-w-0 items-center gap-2.5">
                      <ServerTile name={instance.name} status={instance.status} image={instance.icon_url} size="sm" />
                      <button
                        type="button"
                        className="server-row-open"
                        onClick={() => openOnMachine(machine, instance.id)}
                        title={instance.name}
                      >
                        {instance.name}
                      </button>
                    </span>
                  </td>
                  <td>
                    <span className="fleet-machine-cell">
                      <Icon name={machine.remote ? "servers" : "monitor"} size={13} />
                      {machine.name}
                    </span>
                  </td>
                  <td>
                    <span className="server-row-status">
                      <StatusDot status={instance.status} />
                      {t(`status.${instance.status}`)}
                    </span>
                  </td>
                  <td className="readout server-row-num" data-numeric="">
                    {instance.status === "online"
                      ? `${instance.players_online}${instance.players_max ? `/${instance.players_max}` : ""}`
                      : "—"}
                  </td>
                  <td className="readout server-row-num" data-numeric="" data-col="cpu">
                    {live ? `${Math.round(Math.min(100, Math.max(0, instance.cpu_percent)))}%` : "—"}
                  </td>
                  <td className="readout server-row-num" data-numeric="">
                    {live && instance.memory_used_mb ? formatMegabytes(instance.memory_used_mb) : "—"}
                  </td>
                  <td className="readout server-row-num" data-numeric="" data-col="uptime">
                    {formatDuration(instance.uptime_seconds)}
                  </td>
                  <td className="mono server-row-muted" data-col="address">
                    {instance.address ?? "—"}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        {rows.length === 0 ? (
          <p className="fleet-empty">{fleet.some((m) => m.loading) ? t("fleet.loading") : t("fleet.noServers")}</p>
        ) : null}
      </div>
    </div>
  );
}

// -- one card per machine ----------------------------------------------------

function MachineCards({ fleet }: { fleet: FleetEntry[] }) {
  return (
    <div className="fleet-grid">
      {fleet.map((machine) => (
        <MachineCard key={machine.key} machine={machine} />
      ))}
    </div>
  );
}

function MachineCard({ machine }: { machine: FleetEntry }) {
  const { t } = useI18n();
  const health = machine.health;
  const instances = machine.instances ?? [];
  const running = instances.filter((i) => i.status === "online" || i.status === "starting");
  const players = running.reduce((sum, i) => sum + (i.status === "online" ? i.players_online : 0), 0);
  const allocated = running.reduce((sum, i) => sum + i.resources.memory_mb, 0);

  return (
    <article className="panel fleet-card" data-managed={machine.managed ? "" : undefined}>
      <header className="fleet-card-head">
        <span className="fleet-lamp" data-state={machine.loading ? "wait" : machine.reachable ? "up" : "down"} />
        <div className="flex min-w-0 flex-col">
          <h2 className="fleet-card-title">{machine.name}</h2>
          <span className="fleet-card-sub">
            {machine.remote ? machine.address : t("remote.thisPc")}
            {health?.os ? ` · ${health.os}` : ""}
          </span>
        </div>
        {machine.managed ? <span className="tag">{t("fleet.managing")}</span> : null}
      </header>

      {!machine.reachable && !machine.loading ? (
        <p className="fleet-warning">
          <Icon name="warning" size={14} />
          {t("fleet.noAnswer")}
        </p>
      ) : (
        <>
          <div className="fleet-meters">
            <Meter
              label={health?.cpu_count ? t("fleet.cpuCores", { count: health.cpu_count }) : "CPU"}
              detail={health ? `${Math.round(health.cpu_percent)}%` : "—"}
              value={health?.cpu_percent ?? 0}
            />
            <Meter
              label={t("node.memory")}
              detail={health ? `${formatMegabytes(health.memory_used_mb)} / ${formatMegabytes(health.memory_total_mb)}` : "—"}
              value={health?.memory_used_mb ?? 0}
              max={health?.memory_total_mb ?? 1}
            />
            <Meter
              label={t("node.disk")}
              detail={health ? `${health.disk_used_gb} / ${health.disk_total_gb} GB` : "—"}
              value={health?.disk_used_gb ?? 0}
              max={health?.disk_total_gb ?? 1}
            />
          </div>

          <dl className="fleet-facts">
            <div>
              <dt>{t("fleet.servers")}</dt>
              <dd className="readout">
                {running.length}/{instances.length}
              </dd>
            </div>
            <div>
              <dt>{t("servers.players")}</dt>
              <dd className="readout">{players}</dd>
            </div>
            <div>
              <dt>{t("fleet.allocated")}</dt>
              <dd className="readout">{allocated ? formatMegabytes(allocated) : "—"}</dd>
            </div>
            <div>
              <dt>{t("fleet.machineUptime")}</dt>
              <dd className="readout">{health?.uptime_seconds ? formatDuration(health.uptime_seconds) : "—"}</dd>
            </div>
          </dl>

          {running.length ? (
            <ul className="fleet-running">
              {running.map((instance) => (
                <li key={instance.id}>
                  <button type="button" onClick={() => openOnMachine(machine, instance.id)}>
                    <StatusDot status={instance.status} />
                    <span className="fleet-running-name">{instance.name}</span>
                    <span className="readout fleet-running-meta">
                      {instance.status === "online" ? `${instance.players_online} · ` : ""}
                      {formatDuration(instance.uptime_seconds)}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
        </>
      )}

      <footer className="fleet-card-foot">
        <Button size="sm" variant={machine.managed ? "quiet" : "default"} icon="servers" onClick={() => manageMachine(machine)}>
          {machine.managed ? t("fleet.openServers") : t("fleet.manage")}
        </Button>
      </footer>
    </article>
  );
}
