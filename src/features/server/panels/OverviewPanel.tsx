/**
 * The landing panel for a server: is it up, who is on it, what do I tell my
 * friends to type — then the things you reach for straight after.
 */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";

import { keys, useInstanceActions } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { formatDateTime, formatDuration, formatMegabytes } from "@/lib/format";
import type { InstanceView } from "@/types/api";
import { Button, Dialog, ErrorNotice, Icon, Panel, StatusChip, statusHint } from "@/ui";

export function OverviewPanel({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const navigate = useNavigate();
  const actions = useInstanceActions();

  const [copied, setCopied] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleteFiles, setDeleteFiles] = useState(false);

  const backup = useMutation({
    mutationFn: () => api.backups.create(instance.id),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.backups(instance.id) }),
  });

  const hint = statusHint(instance.status);
  const automation = instance.automation;

  const copyAddress = async () => {
    if (!instance.address) return;
    await navigator.clipboard.writeText(instance.address);
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  return (
    <div className="flex flex-col gap-4">
      {/* The state, said once, large */}
      <section className="panel flex flex-col gap-4 p-5">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <StatusChip status={instance.status} />
            {hint ? (
              <span className="text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
                {t(hint)}
              </span>
            ) : null}
          </div>
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              icon="archive"
              disabled={backup.isPending}
              onClick={() => backup.mutate()}
            >
              {backup.isPending ? t("backups.creating") : t("overview.backupNow")}
            </Button>
            <Button
              size="sm"
              icon="external"
              onClick={async () => {
                const folder = await api.files.reveal(instance.id);
                const { openPath } = await import("@tauri-apps/plugin-opener");
                await openPath(folder);
              }}
            >
              {t("overview.openFolder")}
            </Button>
          </div>
        </div>

        <dl className="grid grid-cols-2 gap-4 sm:grid-cols-4">
          <Stat
            label={t("servers.players")}
            value={
              instance.status === "online"
                ? `${instance.players_online}${instance.players_max ? ` / ${instance.players_max}` : ""}`
                : "—"
            }
          />
          <Stat label={t("servers.uptime")} value={formatDuration(instance.uptime_seconds)} />
          <Stat
            label={t("servers.memory")}
            value={instance.memory_used_mb ? formatMegabytes(instance.memory_used_mb) : "—"}
          />
          <Stat
            label={t("overview.tickRate")}
            value={instance.tick_rate ? instance.tick_rate.toFixed(1) : "—"}
          />
        </dl>
      </section>

      {/* The one thing people actually come here to copy */}
      <Panel title={t("overview.address")}>
        <div className="p-4">
          {instance.address ? (
            <button
              type="button"
              onClick={() => void copyAddress()}
              className="flex w-full items-center justify-between gap-3 rounded-[var(--radius)] border border-[color:var(--line)] px-4 py-3 text-left transition-colors hover:border-[color:var(--line-strong)]"
              style={{ background: "var(--surface-sunken)" }}
            >
              <span className="mono truncate text-[length:var(--text-lg)]">{instance.address}</span>
              <span className="flex flex-none items-center gap-2 text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
                <Icon name={copied ? "check" : "external"} size={14} />
                {copied ? t("actions.copied") : t("overview.copyHint")}
              </span>
            </button>
          ) : (
            <p className="text-[color:var(--text-muted)]">{t("overview.noAddress")}</p>
          )}
        </div>
      </Panel>

      <div className="grid gap-4 lg:grid-cols-2">
        <Panel title={t("overview.automation")}>
          <ul className="flex flex-col gap-2 p-4">
            {automation.sleep_when_empty ? (
              <Fact
                icon="clock"
                text={`${t("settings.sleepWhenEmpty")} — ${t("settings.sleepAfter")} ${automation.sleep_after_minutes} ${t("settings.minutes")}`}
              />
            ) : null}
            {automation.wake_on_join ? <Fact icon="moon" text={t("settings.wakeOnJoin")} /> : null}
            {automation.restart_after_crash ? (
              <Fact icon="restart" text={t("settings.restartAfterCrash")} />
            ) : null}
            {automation.schedules.length ? (
              <Fact
                icon="clock"
                text={`${t("settings.schedules")}: ${automation.schedules.length}`}
              />
            ) : null}
            {!automation.sleep_when_empty &&
            !automation.wake_on_join &&
            !automation.restart_after_crash &&
            !automation.schedules.length ? (
              <li className="text-[color:var(--text-muted)]">{t("overview.automationNone")}</li>
            ) : null}
          </ul>
        </Panel>

        <Panel title={t("servers.version")}>
          <dl className="flex flex-col gap-3 p-4">
            <Row label={t("create.distribution")} value={instance.build.distribution} />
            <Row label={t("create.version")} value={instance.build.version} />
            {instance.build.loader_version ? (
              <Row label={t("create.loaderVersion")} value={instance.build.loader_version} />
            ) : null}
            <Row label={t("create.memory")} value={formatMegabytes(instance.resources.memory_mb)} />
            <Row label={t("create.port")} value={String(Object.values(instance.ports)[0] ?? "—")} />
            <Row label={t("overview.created")} value={formatDateTime(instance.created_at, language)} />
            <Row label={t("overview.folder")} value={instance.path} mono />
          </dl>
        </Panel>
      </div>

      {backup.error ? <ErrorNotice error={backup.error} /> : null}

      <div>
        <Button variant="danger" icon="trash" onClick={() => setConfirmDelete(true)}>
          {t("overview.deleteServer")}
        </Button>
      </div>

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

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-1">
      <dt className="label">{label}</dt>
      <dd className="mono text-[length:var(--text-xl)]">{value}</dd>
    </div>
  );
}

function Row({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <dt className="flex-none text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
        {label}
      </dt>
      <dd
        className={`selectable min-w-0 truncate text-right text-[length:var(--text-sm)] ${mono ? "mono" : ""}`}
        title={value}
      >
        {value}
      </dd>
    </div>
  );
}

function Fact({ icon, text }: { icon: "clock" | "moon" | "restart"; text: string }) {
  return (
    <li className="flex items-center gap-3">
      <span className="text-[color:var(--accent)]">
        <Icon name={icon} size={14} />
      </span>
      <span className="text-[length:var(--text-sm)]">{text}</span>
    </li>
  );
}
