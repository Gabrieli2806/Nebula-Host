/**
 * One server, as a table row: the density for ten or twenty of them.
 *
 * Fixed-height rows with the identity tile, a lamp and its word, and the
 * manifest's name for the build (not the raw id). The name is the row's
 * button and its hit area covers the row, so a click anywhere opens the
 * server; the Start/Stop button sits on top as its own control.
 */

import { useDistribution, useInstanceActions } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatDuration, formatMegabytes } from "@/lib/format";
import type { InstanceView } from "@/types/api";
import { TransferTag } from "@/features/server/transfer/active";
import { movedTo } from "@/features/server/transfer/machines";
import { Button, DistributionTag, ErrorNotice, ServerTile, StatusDot, identityProps, isIdle } from "@/ui";

/** Allocated memory the way it was chosen: "4 GB", not "4.0 GB". */
function formatAllocation(mb: number): string {
  return mb >= 1024 && mb % 1024 === 0 ? `${mb / 1024} GB` : formatMegabytes(mb);
}

export function ServerRow({ instance, onOpen }: { instance: InstanceView; onOpen: () => void }) {
  const { t } = useI18n();
  const actions = useInstanceActions();
  const distribution = useDistribution(instance);
  const accent = distribution?.accent;

  const running = instance.status === "online" || instance.status === "starting";
  // A process exists, so CPU and memory readings mean something.
  const live = running || instance.status === "stopping";
  // Stop stays available while a server is starting (as in the rail), so a
  // stuck startup can be put out from the list.
  const busy = actions.start.isPending || actions.stop.isPending || instance.status === "stopping";
  const failure = actions.start.error ?? actions.stop.error;
  // An older copy left behind by a transfer does not start from here.
  const movedNode = movedTo(instance);
  const actionLabel = running
    ? t("actions.stop")
    : instance.status === "sleeping"
      ? t("actions.wake")
      : t("actions.start");

  const toggle = () => {
    if (running) {
      actions.start.reset();
      actions.stop.mutate(instance.id);
    } else {
      actions.stop.reset();
      actions.start.mutate(instance.id);
    }
  };

  return (
    <>
      <tr
        data-instance-id={instance.id}
        className="server-row"
        {...identityProps(accent)}
        data-idle={isIdle(instance.status) ? "" : undefined}
        data-failed={failure ? "" : undefined}
        onDoubleClick={onOpen}
      >
        <td>
          <span className="flex min-w-0 items-center gap-2.5">
            <ServerTile
              name={instance.name}
              identity={accent}
              status={instance.status}
              image={instance.icon_url}
              size="sm"
            />
            <button type="button" className="server-row-open" onClick={onOpen} title={instance.name}>
              {instance.name}
            </button>
            {/* In a narrow table the status column folds into this lamp. */}
            <span className="server-row-lamp">
              <StatusDot status={instance.status} label={t(`status.${instance.status}`)} />
            </span>
          </span>
        </td>
        <td data-col="status">
          <span className="server-row-status">
            <StatusDot status={instance.status} />
            {t(`status.${instance.status}`)}
          </span>
        </td>
        <td>
          <span className="flex items-center gap-2">
            <DistributionTag distribution={distribution} fallback={instance.build.distribution} />
            <span className="mono server-row-muted">{instance.build.version}</span>
            <TransferTag instanceId={instance.id} />
            {movedNode ? (
              <span className="tag" title={t("transfer.cardMoved", { node: movedNode })}>
                {t("transfer.tagOlder")}
              </span>
            ) : null}
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
          <span className="server-row-of">{` / ${formatAllocation(instance.resources.memory_mb)}`}</span>
        </td>
        <td className="readout server-row-num" data-numeric="" data-col="uptime">
          {formatDuration(instance.uptime_seconds)}
        </td>
        <td className="server-row-action">
          <Button
            size="sm"
            icon={running ? "stop" : "play"}
            iconOnly
            disabled={busy || (!running && movedNode !== null)}
            onClick={toggle}
          >
            {`${actionLabel}: ${instance.name}`}
          </Button>
        </td>
      </tr>
      {failure ? (
        <tr className="server-row-error">
          <td colSpan={8}>
            <ErrorNotice error={failure} compact />
          </td>
        </tr>
      ) : null}
    </>
  );
}
