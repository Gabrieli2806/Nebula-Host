/**
 * One server, as a card.
 *
 * The card answers the three questions someone actually opens this app to ask:
 * is it up, who is on it, and what do I tell my friends to type.
 *
 * Anatomy: the
 * identity tile (muted while the server is idle), the name with its
 * lamp-and-word status, the build tags, the address, then the facts row and
 * the one action that matters.
 *
 * The whole card opens the server: the name is the real button and its hit
 * area is stretched over the card, so there is one tab stop for "open" and
 * the address and the Start/Stop button stay separate controls on top.
 */

import { useDistribution, useInstanceActions } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatDuration, formatMegabytes } from "@/lib/format";
import type { InstanceView } from "@/types/api";
import { TransferNote } from "@/features/server/transfer/TransferBanner";
import { movedTo } from "@/features/server/transfer/machines";
import {
  Button,
  CopyAddress,
  DistributionTag,
  ErrorNotice,
  Icon,
  ServerTile,
  StatusChip,
  identityProps,
  isIdle,
  statusHint,
} from "@/ui";

/** Allocated memory the way it was chosen: "4 GB", not "4.0 GB". */
function formatAllocation(mb: number): string {
  return mb >= 1024 && mb % 1024 === 0 ? `${mb / 1024} GB` : formatMegabytes(mb);
}

/** Above this share a usage bar turns amber. */
const USAGE_WARN = 90;

export function ServerCard({ instance, onOpen }: { instance: InstanceView; onOpen: () => void }) {
  const { t } = useI18n();
  const actions = useInstanceActions();
  const distribution = useDistribution(instance);

  const hint = statusHint(instance.status);
  // Stop stays available while a server is starting (as in the rail), so a
  // stuck startup can be put out from the list.
  const busy = actions.start.isPending || actions.stop.isPending || instance.status === "stopping";
  // An older copy left behind by a transfer does not start from here.
  const movedNode = movedTo(instance);
  const accent = distribution?.accent;
  const running = instance.status === "online" || instance.status === "starting";
  // A process exists, so CPU and memory readings mean something.
  const live = running || instance.status === "stopping";
  const cpu = Math.min(100, Math.max(0, instance.cpu_percent));
  const allocated = instance.resources.memory_mb;
  // The last start/stop that failed, shown under the address.
  const failure = actions.start.error ?? actions.stop.error;

  const start = () => {
    actions.stop.reset();
    actions.start.mutate(instance.id);
  };
  const stop = () => {
    actions.start.reset();
    actions.stop.mutate(instance.id);
  };

  return (
    <article
      data-instance-id={instance.id}
      className="panel card-interactive server-card"
      {...identityProps(accent)}
      data-idle={isIdle(instance.status) ? "" : undefined}
      data-status={instance.status}
    >
      <header className="flex items-start gap-3.5">
        <ServerTile name={instance.name} identity={accent} status={instance.status} image={instance.icon_url} />

        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="server-card-title">
            <h3 className="pt-[1px]">
              <button
                type="button"
                className="server-card-open"
                onClick={onOpen}
                title={instance.name}
              >
                {instance.name}
              </button>
            </h3>
            <StatusChip status={instance.status} className="flex-none" />
          </div>

          {/* One line, always: every card in a row keeps the same header
              height, so the addresses line up. What does not fit is the
              least important (the loader build), and it is cut short. */}
          <div className="server-card-tags">
            <DistributionTag distribution={distribution} fallback={instance.build.distribution} />
            {movedNode ? <span className="tag">{t("transfer.tagOlder")}</span> : null}
            {!movedNode && instance.forked_from ? <span className="tag">{t("transfer.tagForked")}</span> : null}
            <span className="tag">{instance.build.version}</span>
            {instance.build.loader_version ? (
              <span className="tag server-card-tag-trim" title={instance.build.loader_version}>
                {instance.build.loader_version}
              </span>
            ) : null}
          </div>
        </div>
      </header>

      {/* The address sits at the same offset on every card in a row; the
          state hint (sleeping, starting, crashed) comes after it. */}
      <div className="server-card-raised flex flex-col gap-2">
        {/* An older copy has no address worth sharing: the server now
            answers on the machine it moved to. */}
        {movedNode ? (
          <TransferNote instance={instance} />
        ) : (
          <CopyAddress
            value={instance.address}
            emptyText={t("servers.noAddress")}
            className="server-card-address"
          />
        )}
        {failure ? <ErrorNotice error={failure} compact /> : null}
      </div>

      {hint && !movedNode ? <p className="server-card-hint">{t(hint)}</p> : null}

      {/* While a process exists: what it is using, CPU as a share of the
          machine and memory against what it was given. */}
      {live ? (
        <div className="card-usage">
          <Usage
            label={t("node.cpu")}
            value={`${Math.round(cpu)}%`}
            share={cpu}
          />
          <Usage
            label={t("servers.memory")}
            value={instance.memory_used_mb ? formatMegabytes(instance.memory_used_mb) : "—"}
            of={formatAllocation(allocated)}
            share={allocated > 0 ? (instance.memory_used_mb / allocated) * 100 : 0}
          />
        </div>
      ) : null}

      {/* The foot: the numbers that change, in one line, under a hairline. */}
      <div className="fact-strip server-card-foot">
        <div className="server-facts">
          <Fact
            icon="users"
            label={t("servers.players")}
            value={
              instance.status === "online"
                ? `${instance.players_online}${instance.players_max ? `/${instance.players_max}` : ""}`
                : "—"
            }
          />
          <span className="rule" data-fact="uptime" aria-hidden="true" />
          <Fact
            fact="uptime"
            icon="clock"
            label={t("servers.uptime")}
            value={formatDuration(instance.uptime_seconds)}
          />
          {live ? null : (
            <>
              <span className="rule" aria-hidden="true" />
              <Fact icon="cpu" label={t("servers.memory")} value={formatAllocation(allocated)} />
            </>
          )}
        </div>

        <div className="server-card-raised server-card-action">
          {running ? (
            <Button
              size="sm"
              icon="stop"
              disabled={busy}
              onClick={stop}
              aria-label={`${t("actions.stop")}: ${instance.name}`}
            >
              {t("actions.stop")}
            </Button>
          ) : (
            <Button
              size="sm"
              icon="play"
              disabled={busy || movedNode !== null}
              title={movedNode ? t("transfer.startDisabled", { node: movedNode }) : undefined}
              onClick={start}
              aria-label={`${instance.status === "sleeping" ? t("actions.wake") : t("actions.start")}: ${instance.name}`}
            >
              {instance.status === "sleeping" ? t("actions.wake") : t("actions.start")}
            </Button>
          )}
        </div>
      </div>
    </article>
  );
}

function Fact({
  fact,
  icon,
  label,
  value,
}: {
  fact?: string;
  icon: "users" | "clock" | "cpu";
  label: string;
  value: string;
}) {
  return (
    <span className="server-fact" data-fact={fact} title={label}>
      <Icon name={icon} size={13} />
      <span className="sr-only">{label}</span>
      <span className="readout">{value}</span>
    </span>
  );
}

function Usage({
  label,
  value,
  of,
  share,
}: {
  label: string;
  value: string;
  of?: string;
  /** 0–100. */
  share: number;
}) {
  const fill = Math.min(100, Math.max(0, share));
  return (
    <div className="card-usage-item" data-warn={fill >= USAGE_WARN ? "" : undefined}>
      <div className="card-usage-line">
        <span className="card-usage-label">{label}</span>
        <span className="readout card-usage-value">
          {value}
          {of ? <span className="card-usage-of">{` / ${of}`}</span> : null}
        </span>
      </div>
      <span
        className="card-usage-bar"
        role="meter"
        aria-label={label}
        aria-valuenow={Math.round(fill)}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <span style={{ width: `${fill}%` }} />
      </span>
    </div>
  );
}
