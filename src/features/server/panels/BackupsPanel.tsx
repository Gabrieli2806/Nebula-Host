/**
 * Backups as a timeline.
 *
 * Restore points read newest-first with the date said plainly, because the
 * question being asked is always "how far back do I have to go". The rail's
 * newest marker is lit in the accent; older ones are outlined.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { formatBytes, formatDateTime } from "@/lib/format";
import type { Backup, InstanceView } from "@/types/api";
import { Button, Dialog, EmptyState, ErrorNotice, Panel, Skeleton } from "@/ui";

export function BackupsPanel({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const [restoring, setRestoring] = useState<Backup | null>(null);
  const [deleting, setDeleting] = useState<Backup | null>(null);

  const backups = useQuery({
    queryKey: keys.backups(instance.id),
    queryFn: () => api.backups.list(instance.id),
  });

  const invalidate = () => client.invalidateQueries({ queryKey: keys.backups(instance.id) });

  const create = useMutation({
    mutationFn: () => api.backups.create(instance.id),
    onSuccess: invalidate,
  });
  const remove = useMutation({
    mutationFn: (backupId: string) => api.backups.remove(instance.id, backupId),
    onSuccess: () => {
      setDeleting(null);
      return invalidate();
    },
  });
  const restore = useMutation({
    mutationFn: (backupId: string) => api.backups.restore(instance.id, backupId),
    onSuccess: () => {
      setRestoring(null);
      // The folder is now the backup's: its size, its files and its addons.
      void client.invalidateQueries({ queryKey: keys.diskUsage(instance.id) });
      void client.invalidateQueries({ queryKey: ["files", instance.id] });
      void client.invalidateQueries({ queryKey: keys.addons(instance.id) });
    },
  });

  const closeDelete = () => {
    setDeleting(null);
    remove.reset();
  };

  return (
    <Panel
      title={
        backups.data?.length ? (
          <span className="panel-count">
            {backups.data.length === 1
              ? t("backups.count_one")
              : t("backups.count", { count: backups.data.length })}
          </span>
        ) : undefined
      }
      actions={
        <Button size="sm" icon="archive" disabled={create.isPending} onClick={() => create.mutate()}>
          {create.isPending ? t("backups.creating") : t("backups.create")}
        </Button>
      }
    >
      {create.error ? (
        <div className="p-4">
          <ErrorNotice error={create.error} />
        </div>
      ) : null}

      {backups.isLoading ? (
        <div className="flex flex-col gap-3 p-4" aria-hidden="true">
          {[0, 1, 2].map((index) => (
            <Skeleton key={index} className="h-10" />
          ))}
        </div>
      ) : null}

      {backups.error ? (
        <div className="p-4">
          <ErrorNotice error={backups.error} onRetry={() => void backups.refetch()} />
        </div>
      ) : null}

      {backups.data?.length === 0 ? (
        <EmptyState icon="archive" title={t("backups.empty")} hint={t("backups.emptyHint")} />
      ) : null}

      {backups.data?.length ? (
        <ol className="backup-timeline">
          {backups.data.map((backup, index) => (
            <li
              key={backup.id}
              className="backup-entry"
              data-newest={index === 0 ? "" : undefined}
              data-last={index === backups.data.length - 1 ? "" : undefined}
            >
              {/* The rail: a spine of markers so the list reads as a history. */}
              <span className="backup-rail" aria-hidden="true">
                <span className="backup-marker" />
              </span>

              <div className="backup-body">
                <div className="flex min-w-0 flex-col gap-0.5">
                  <p className="flex min-w-0 items-center gap-2">
                    <span className="backup-when truncate">
                      {backup.label ?? formatDateTime(backup.createdAt, language)}
                    </span>
                    {index === 0 ? (
                      <span className="tag" data-tone="accent">
                        {t("backups.latest")}
                      </span>
                    ) : null}
                  </p>
                  <p className="backup-meta">
                    {backup.label ? <span>{formatDateTime(backup.createdAt, language)}</span> : null}
                    <span className="mono">{formatBytes(backup.sizeBytes)}</span>
                  </p>
                </div>
                <div className="flex flex-none items-center gap-1.5">
                  <Button size="sm" icon="history" onClick={() => setRestoring(backup)}>
                    {t("backups.restore")}
                  </Button>
                  <Button
                    size="sm"
                    variant="quiet"
                    icon="trash"
                    iconOnly
                    className="row-action"
                    onClick={() => setDeleting(backup)}
                  >
                    {t("actions.delete")}
                  </Button>
                </div>
              </div>
            </li>
          ))}
        </ol>
      ) : null}

      <Dialog
        open={restoring !== null}
        onClose={() => setRestoring(null)}
        title={t("backups.restoreConfirm", {
          name: restoring ? formatDateTime(restoring.createdAt, language) : "",
        })}
        footer={
          <>
            <Button variant="quiet" onClick={() => setRestoring(null)}>
              {t("actions.cancel")}
            </Button>
            <Button
              variant="danger"
              disabled={restore.isPending}
              onClick={() => restoring && restore.mutate(restoring.id)}
            >
              {t("backups.restore")}
            </Button>
          </>
        }
      >
        <p className="text-[color:var(--text-muted)]">
          {t("backups.restoreWarning", {
            date: restoring ? formatDateTime(restoring.createdAt, language) : "",
          })}
        </p>
        {restore.error ? (
          <div className="mt-4">
            <ErrorNotice error={restore.error} />
          </div>
        ) : null}
      </Dialog>

      <Dialog
        open={deleting !== null}
        onClose={closeDelete}
        width={460}
        title={t("backups.deleteConfirm", {
          date: deleting ? formatDateTime(deleting.createdAt, language) : "",
        })}
        footer={
          <>
            <Button variant="quiet" onClick={closeDelete}>
              {t("actions.cancel")}
            </Button>
            <Button
              variant="danger"
              icon="trash"
              disabled={remove.isPending}
              onClick={() => deleting && remove.mutate(deleting.id)}
            >
              {t("actions.delete")}
            </Button>
          </>
        }
      >
        <p className="text-[color:var(--text-muted)]">{t("backups.deleteWarning")}</p>
        {remove.error ? (
          <div className="mt-4">
            <ErrorNotice error={remove.error} compact />
          </div>
        ) : null}
      </Dialog>
    </Panel>
  );
}
