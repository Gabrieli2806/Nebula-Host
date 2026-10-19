/**
 * The landing panel for a server, and the only place its identity is shown.
 *
 * Top to bottom: the header card (tile, name, status, build tags, the two
 * housekeeping actions, then the address in the size it deserves), the
 * instance's own resources (each number once), what fills its folder, its
 * details and its automation. Start / Stop / Restart live in the sidebar, so
 * they are not repeated here.
 */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate } from "react-router-dom";

import {
  keys,
  useDiskUsage,
  useDistribution,
  useInstanceActions,
} from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { isRemote } from "@/lib/connection";
import { IS_WEB } from "@/lib/web";
import {
  formatBytes,
  formatCount,
  formatDateTime,
  formatDuration,
  formatMegabytes,
  formatRelative,
} from "@/lib/format";
import type { DiskUsage, InstanceView } from "@/types/api";
import { TransferBanner } from "@/features/server/transfer/TransferBanner";
import { movedTo } from "@/features/server/transfer/machines";
import { TransferProgressStrip } from "@/features/server/transfer/TransferStrip";
import { TransferDialog } from "@/features/server/transfer/TransferDialog";
import {
  Button,
  CopyAddress,
  Dialog,
  DistributionTag,
  ErrorNotice,
  Icon,
  Panel,
  ServerTile,
  Skeleton,
  StatusChip,
  identityProps,
  isIdle,
  statusHint,
} from "@/ui";

/** Below this the server is falling behind: say so in the starting colour. */
const TICK_RATE_WARN = 18;
/** Minecraft's target tick rate; the tick-rate bar is drawn against it. */
const TICK_RATE_TARGET = 20;
/** CPU share of the machine, and memory share of the allocation, worth flagging. */
const LOAD_WARN = 90;
/** Entries listed under Storage. */
const STORAGE_ROWS = 6;

/** Allocated memory the way it was chosen: "4 GB", not "4096 MB" or "4.0 GB". */
function formatAllocation(mb: number): string {
  return mb >= 1024 && mb % 1024 === 0 ? `${mb / 1024} GB` : formatMegabytes(mb);
}

/** "1.8 GB" split into its number and its unit, so the unit can step down. */
function splitUnit(text: string): [string, string | undefined] {
  const [value, unit] = text.split(" ");
  return [value ?? text, unit];
}

export function OverviewPanel({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const navigate = useNavigate();
  const actions = useInstanceActions();
  const distribution = useDistribution(instance);

  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [transferOpen, setTransferOpen] = useState(false);
  // An older copy left behind by a transfer moves only by "Bring it back".
  const olderCopy = movedTo(instance) !== null;

  const backup = useMutation({
    mutationFn: () => api.backups.create(instance.id),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.backups(instance.id) }),
  });

  const openFolder = useMutation({
    mutationFn: () => api.files.openFolder(instance.id),
  });

  const hint = statusHint(instance.status);
  const automation = instance.automation;
  const crashed = instance.status === "crashed";
  const identity = identityProps(distribution?.accent);
  const actionError = backup.error ?? openFolder.error;
  const port = Object.values(instance.ports)[0];
  const automationParts = [
    automation.sleep_when_empty
      ? t("overview.automationSleep", { minutes: automation.sleep_after_minutes })
      : null,
    automation.wake_on_join ? t("overview.automationWake") : null,
    automation.restart_after_crash ? t("overview.automationRestart") : null,
    automation.schedules.length
      ? t("overview.automationSchedules", { count: automation.schedules.length })
      : null,
  ].filter((part): part is string => part !== null);
  const automationSummary = automationParts.length
    ? automationParts.join(" · ")
    : t("overview.automationNone");

  return (
    <div className="overview">
      <TransferBanner instance={instance} />

      {/* The one header for this server. Nothing else on the screen repeats
          its name, status or build. */}
      <header
        className="plate overview-head"
        data-status={instance.status}
        data-idle={isIdle(instance.status) ? "" : undefined}
        {...identity}
      >
        <div className="overview-head-top">
          <div className="flex min-w-0 flex-[1_1_auto] items-center gap-4">
            <ServerTile
              name={instance.name}
              identity={distribution?.accent}
              status={instance.status}
              image={instance.icon_url}
              size="lg"
            />
            <div className="flex min-w-0 flex-col gap-2">
              <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1.5">
                <h1 className="display overview-name min-w-0 truncate" title={instance.name}>
                  {instance.name}
                </h1>
                <StatusChip status={instance.status} />
              </div>
              <div className="flex flex-wrap items-center gap-1.5">
                <DistributionTag distribution={distribution} fallback={instance.build.distribution} />
                <span className="tag">{instance.build.version}</span>
                {instance.build.loader_version ? (
                  <span className="tag">{instance.build.loader_version}</span>
                ) : null}
              </div>
            </div>
          </div>

          <div className="overview-head-actions">
            {/* On a phone the sidebar (and its Start button) is a closed
                drawer, so the power action sits here too. */}
            {olderCopy ? null : instance.status === "online" || instance.status === "starting" ? (
              <Button
                size="sm"
                icon="stop"
                className="narrow-only"
                disabled={instance.status === "starting" || actions.stop.isPending}
                onClick={() => actions.stop.mutate(instance.id)}
              >
                {t("actions.stop")}
              </Button>
            ) : (
              <Button
                size="sm"
                variant="primary"
                icon="play"
                className="narrow-only"
                disabled={instance.status === "stopping" || actions.start.isPending}
                onClick={() => actions.start.mutate(instance.id)}
              >
                {instance.status === "sleeping" ? t("actions.wake") : t("actions.start")}
              </Button>
            )}
            <Button
              size="sm"
              icon="archive"
              disabled={backup.isPending}
              onClick={() => backup.mutate()}
            >
              {backup.isPending ? t("backups.creating") : t("overview.backupNow")}
            </Button>
            {IS_WEB ? null : (
              <Button
                size="sm"
                icon="external"
                disabled={openFolder.isPending || isRemote()}
                title={isRemote() ? t("remote.localOnly") : undefined}
                onClick={() => openFolder.mutate()}
              >
                {t("overview.openFolder")}
              </Button>
            )}
            {olderCopy || IS_WEB ? null : (
              <Button
                size="sm"
                icon="send"
                disabled={instance.status === "stopping"}
                onClick={() => setTransferOpen(true)}
              >
                {t("transfer.action")}
              </Button>
            )}
          </div>
        </div>

        {hint || crashed ? (
          <div className="overview-head-note">
            <p className="min-w-0 max-w-[60ch]">{hint ? t(hint) : null}</p>
            {/* After a crash the console says why; Start is in the sidebar. */}
            {crashed ? (
              <Button size="sm" icon="terminal" onClick={() => navigate(`/servers/${instance.id}/console`)}>
                {t("overview.openConsole")}
              </Button>
            ) : null}
          </div>
        ) : null}

        {actionError ? <ErrorNotice error={actionError} compact /> : null}

        <TransferProgressStrip instanceId={instance.id} />

        {/* An older copy cannot start, and players now join the machine it
            moved to (the banner above names it): no address to hand out. */}
        {olderCopy ? null : (
          <CopyAddress
            size="lg"
            value={instance.address}
            label={t("overview.address")}
            hint={t("overview.copyHint")}
            emptyText={t("overview.noAddress")}
          />
        )}
        {!olderCopy && instance.public_address ? (
          <CopyAddress
            value={instance.public_address}
            label={t("overview.publicAddress")}
            hint={t("overview.publicAddressHint")}
          />
        ) : null}
      </header>

      <Resources instance={instance} />

      <div className="overview-columns">
        <Storage instanceId={instance.id} />

        <div className="flex min-w-0 flex-col gap-5">
          <Panel title={t("overview.details")}>
            <dl className="overview-facts">
              {/* The address above already carries the port; it is listed
                  here only when there is no address to read it from. */}
              {port !== undefined && !instance.address ? (
                <Row label={t("create.port")} value={String(port)} face="readout" />
              ) : null}
              <Row label={t("overview.created")} value={formatDateTime(instance.created_at, language)} />
              <Row
                label={t("servers.lastPlayed")}
                value={
                  instance.last_player_seen_at
                    ? formatDateTime(instance.last_player_seen_at, language)
                    : t("servers.never")
                }
              />
              <Row label={t("overview.folder")} value={instance.path} face="mono" />
              {/* Settings owns the automation switches; this is one quiet
                  line that says what is on and leads there. */}
              <div className="overview-fact">
                <dt className="flex-none text-[color:var(--text-muted)]">{t("overview.automation")}</dt>
                <dd className="min-w-0 text-right leading-snug">
                  <Link
                    to={`/servers/${instance.id}/settings`}
                    className="overview-fact-link"
                    title={`${automationSummary} · ${t("overview.automationEdit")}`}
                  >
                    {automationSummary}
                  </Link>
                </dd>
              </div>
            </dl>
          </Panel>

        </div>
      </div>

      <div className="overview-danger">
        <Button variant="danger" icon="trash" onClick={() => setConfirmDelete(true)}>
          {t("overview.deleteServer")}
        </Button>
      </div>

      <TransferDialog
        instance={instance}
        open={transferOpen}
        onClose={() => setTransferOpen(false)}
        onReopen={() => setTransferOpen(true)}
      />

      <Dialog
        open={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        title={t("overview.deleteConfirm", { name: instance.name })}
        footer={
          <>
            <Button variant="quiet" onClick={() => setConfirmDelete(false)}>
              {t("actions.cancel")}
            </Button>
            <Button
              variant="danger"
              disabled={actions.remove.isPending}
              onClick={() =>
                actions.remove.mutate(
                  { id: instance.id, deleteFiles },
                  { onSuccess: () => navigate("/servers") },
                )
              }
            >
              {t("actions.delete")}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4">
          <p>{t("overview.deleteWarning")}</p>
          {/* Removing from the app and deleting a world are different acts, so
              they are different clicks. */}
          <label className="flex items-center gap-3">
            <input
              type="checkbox"
              checked={deleteFiles}
              onChange={(event) => setDeleteFiles(event.target.checked)}
            />
            <span>{t("overview.deleteFiles")}</span>
          </label>
          {actions.remove.error ? <ErrorNotice error={actions.remove.error} /> : null}
        </div>
      </Dialog>
    </div>
  );
}

// -- resources ---------------------------------------------------------------

/**
 * What this server is using right now, each number once: CPU (share of the
 * machine), memory (used of allocated), disk (its folder), players, tick
 * rate and uptime. Machine-wide numbers stay on the dashboard.
 */
function Resources({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const disk = useDiskUsage(instance.id);

  const live = instance.status === "online" || instance.status === "starting" || instance.status === "stopping";
  const online = instance.status === "online";
  const allocated = instance.resources.memory_mb;
  const memoryUsed = live && instance.memory_used_mb > 0 ? instance.memory_used_mb : null;
  const memoryShare = memoryUsed && allocated > 0 ? (memoryUsed / allocated) * 100 : 0;
  const cpu = live ? Math.max(0, Math.min(100, instance.cpu_percent)) : null;
  const ticks = online && instance.tick_rate ? instance.tick_rate : null;
  const slowTicks = Boolean(ticks && ticks < TICK_RATE_WARN);
  const startedAt = instance.last_started_at ?? null;

  return (
    <Panel title={t("overview.resources")} bodyClassName="resources-body">
      <dl className="resources">
        <Resource
          label={t("overview.cpu")}
          value={cpu === null ? null : `${cpu < 10 ? cpu.toFixed(1) : Math.round(cpu)} %`}
          detail={t("overview.cpuOfMachine")}
          bar={cpu}
          warn={cpu !== null && cpu >= LOAD_WARN}
        />
        <Resource
          label={t("servers.memory")}
          value={memoryUsed === null ? null : formatMegabytes(memoryUsed)}
          detail={t("overview.memoryOf", { total: formatAllocation(allocated) })}
          bar={memoryUsed === null ? null : memoryShare}
          warn={memoryShare >= LOAD_WARN}
        />
        <Resource
          label={t("node.disk")}
          value={disk.data ? formatBytes(disk.data.totalBytes) : null}
          loading={disk.isLoading}
          detail={
            disk.data
              ? t("overview.diskFiles", { count: formatCount(disk.data.fileCount, language) })
              : t("overview.diskFolder")
          }
        />
        <Resource
          label={t("servers.players")}
          value={online ? String(instance.players_online) : null}
          of={online && instance.players_max ? `/${instance.players_max}` : undefined}
          detail={t("overview.playersNow")}
          bar={online && instance.players_max ? (instance.players_online / instance.players_max) * 100 : null}
        />
        <Resource
          label={t("overview.tickRate")}
          value={ticks === null ? null : ticks.toFixed(1)}
          detail={slowTicks ? t("overview.tickRateSlow") : t("overview.tickRateTarget")}
          bar={ticks === null ? null : (ticks / TICK_RATE_TARGET) * 100}
          warn={slowTicks}
        />
        <Resource
          label={t("servers.uptime")}
          value={online ? formatDuration(instance.uptime_seconds) : null}
          detail={
            !startedAt
              ? online
                ? ""
                : t("overview.neverStarted")
              : online
                ? t("overview.uptimeSince", { time: formatDateTime(startedAt, language) })
                : // Relative ("2 h ago") so it fits the cell; the full date is the tooltip.
                  t("overview.lastStarted", { time: formatRelative(startedAt, language) })
          }
          detailTitle={startedAt ? formatDateTime(startedAt, language) : undefined}
        />
      </dl>
    </Panel>
  );
}

function Resource({
  label,
  value,
  of,
  detail,
  bar = null,
  warn = false,
  loading = false,
  detailTitle,
}: {
  label: string;
  /** null renders an em dash (not running, or not measured). */
  value: string | null;
  /** A quieter suffix such as "/20". */
  of?: string;
  detail: string;
  /** 0–100 fill for the thin bar under the value; null hides it. */
  bar?: number | null;
  warn?: boolean;
  loading?: boolean;
  detailTitle?: string;
}) {
  const [number, unit] = value ? splitUnit(value) : ["—", undefined];
  const fill = bar === null ? null : Math.max(0, Math.min(100, bar));

  return (
    <div className="resource" data-warn={warn ? "" : undefined}>
      <dt className="resource-label">{label}</dt>
      <dd className="resource-value readout">
        {loading ? (
          <Skeleton className="h-[26px] w-[72px] rounded-[var(--radius-sm)]" />
        ) : (
          <>
            {warn ? <span className="resource-mark" aria-hidden="true" /> : null}
            {number}
            {unit ? <span className="resource-unit">{unit}</span> : null}
            {of ? <span className="resource-of">{of}</span> : null}
          </>
        )}
      </dd>
      <dd className="resource-detail" title={detailTitle ?? (detail || undefined)}>
        {detail}
      </dd>
      {fill !== null ? (
        <dd className="resource-bar" aria-hidden="true">
          <span style={{ width: `${fill}%` }} />
        </dd>
      ) : null}
    </div>
  );
}

// -- storage -----------------------------------------------------------------

/** What fills the server's folder: the biggest top-level entries. */
function Storage({ instanceId }: { instanceId: string }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const disk = useDiskUsage(instanceId);

  const refresh = useMutation({
    mutationFn: () => api.instances.diskUsage(instanceId, true),
    onSuccess: (data) => client.setQueryData<DiskUsage>(keys.diskUsage(instanceId), data),
  });

  const data = disk.data;
  const rows = data?.largest.slice(0, STORAGE_ROWS) ?? [];
  const total = Math.max(1, data?.totalBytes ?? 1);

  return (
    <Panel
      title={t("overview.storage")}
      actions={
        <Button
          size="sm"
          variant="quiet"
          icon="refresh"
          iconOnly
          disabled={refresh.isPending || disk.isLoading}
          onClick={() => refresh.mutate()}
        >
          {t("actions.refresh")}
        </Button>
      }
    >
      {disk.isLoading ? (
        <div className="flex flex-col gap-3 px-5 py-4">
          {[0, 1, 2, 3].map((index) => (
            <Skeleton key={index} className="h-[18px] rounded-[var(--radius-sm)]" />
          ))}
        </div>
      ) : disk.error && !data ? (
        <div className="px-5 py-4">
          <ErrorNotice error={disk.error} compact onRetry={() => void disk.refetch()} />
        </div>
      ) : rows.length === 0 ? (
        <p className="px-5 py-4 text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
          {t("overview.storageEmpty")}
        </p>
      ) : (
        <ul className="storage-list" data-busy={refresh.isPending ? "" : undefined}>
          {rows.map((entry) => (
            <StorageRow
              key={entry.name}
              name={entry.name}
              isDir={entry.isDir}
              bytes={entry.bytes}
              share={(entry.bytes / total) * 100}
            />
          ))}
        </ul>
      )}
      {refresh.error ? (
        <div className="px-5 pb-4">
          <ErrorNotice error={refresh.error} compact />
        </div>
      ) : null}
    </Panel>
  );
}

function StorageRow({
  name,
  isDir,
  bytes,
  share,
}: {
  name: string;
  isDir: boolean;
  bytes: number;
  share: number;
}) {
  return (
    <li className="storage-row">
      <span className="storage-icon" aria-hidden="true">
        <Icon name={isDir ? "folder" : "file"} size={14} />
      </span>
      <span className="storage-name mono selectable" title={name}>
        {name}
      </span>
      <span className="storage-size readout">{formatBytes(bytes)}</span>
      <span className="storage-bar" aria-hidden="true">
        <span style={{ width: `${Math.max(1, Math.min(100, share))}%` }} />
      </span>
    </li>
  );
}

// -- small parts -------------------------------------------------------------

function Row({
  label,
  value,
  face = "text",
}: {
  label: string;
  value: string;
  /** text for words, mono for versions and paths, readout for machine numbers. */
  face?: "text" | "mono" | "readout";
}) {
  return (
    <div className="overview-fact">
      <dt className="flex-none text-[color:var(--text-muted)]">{label}</dt>
      <dd className={`selectable min-w-0 truncate text-right ${face === "text" ? "" : face}`} title={value}>
        {value}
      </dd>
    </div>
  );
}
