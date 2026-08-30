/**
 * Backups as a timeline.
 *
 * Restore points read newest-first with the date said plainly, because the
 * question being asked is always "how far back do I have to go".
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { formatBytes, formatDateTime } from "@/lib/format";
import type { Backup, InstanceView } from "@/types/api";
import { Button, Dialog, EmptyState, ErrorNotice, Panel } from "@/ui";

export function BackupsPanel({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const [restoring, setRestoring] = useState<Backup | null>(null);

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
    onSuccess: invalidate,
  });
  const restore = useMutation({
    mutationFn: (backupId: string) => api.backups.restore(instance.id, backupId),
    onSuccess: () => setRestoring(null),
  });

  return (
    <Panel
      title={t("backups.title")}
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

      {backups.data?.length === 0 ? (
        <EmptyState icon="archive" title={t("backups.empty")} hint={t("backups.emptyHint")} />
      ) : null}

      {backups.data?.length ? (
        <ol className="p-4">
          {backups.data.map((backup, index) => (
            <li key={backup.id} className="flex gap-4">
              {/* The rail: a spine of dots so the list reads as a history. */}
              <div className="flex flex-none flex-col items-center">
                <span
                  className="mt-3 h-2 w-2 rounded-full"
                  style={{ background: index === 0 ? "var(--accent)" : "var(--line-strong)" }}
                />
                {index < backups.data.length - 1 ? (
                  <span className="w-px flex-1" style={{ background: "var(--line)" }} />
                ) : null}
              </div>

              <div className="flex flex-1 items-center justify-between gap-4 border-b border-[color:var(--line)] py-3 last:border-0">
                <div className="min-w-0">
                  <p className="truncate font-medium">
                    {backup.label ?? formatDateTime(backup.createdAt, language)}
                  </p>
                  <p className="mono text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
                    {formatBytes(backup.sizeBytes)}
                  </p>
                </div>
                <div className="flex flex-none items-center gap-2">
                  <Button size="sm" onClick={() => setRestoring(backup)}>
                    {t("backups.restore")}
                  </Button>
                  <Button
                    size="sm"
                    variant="quiet"
                    icon="trash"
                    iconOnly
                    onClick={() => remove.mutate(backup.id)}
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
        <p>
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
    </Panel>
  );
}
