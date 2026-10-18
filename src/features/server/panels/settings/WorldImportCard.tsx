/**
 * Import a world: a singleplayer save or the worlds of a previous server.
 * On this PC from a `.zip` or straight from its folder; on a remote machine
 * from a `.zip`, streamed there first. The current world is backed up before
 * it is replaced, and the server must be stopped.
 */

import { useEffect, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";

import "./world-import.css";
import { useI18n } from "@/i18n";
import { isRemote } from "@/lib/connection";
import { api, onAppEvent, onLocalUpload } from "@/lib/ipc";
import type { InstanceView, WorldImportResult } from "@/types/api";
import { Button, Dialog, ErrorNotice, Icon, Panel } from "@/ui";

type Stage = { phase: "upload" | "backup" | "unpack" | "place" | "done"; done: number; total: number };

/** Live progress for one import: the local upload (remote only), then the
 *  backup / unpack / place phases reported by the machine doing it. */
function useImportProgress(instanceId: string, active: boolean): Stage | null {
  const [stage, setStage] = useState<Stage | null>(null);
  useEffect(() => {
    if (!active) return;
    const stopUpload = onLocalUpload(({ sent, total }) => setStage({ phase: "upload", done: sent, total }));
    const stopEvents = onAppEvent((event) => {
      if (event.type === "worldImport" && event.instanceId === instanceId) setStage(event.progress);
    });
    return () => {
      stopUpload();
      stopEvents();
      setStage(null);
    };
  }, [instanceId, active]);
  return active ? stage : null;
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(0, Math.round(bytes / 1024))} KB`;
}

function ImportProgress({ stage }: { stage: Stage | null }) {
  const { t } = useI18n();
  const phase = stage?.phase ?? "backup";
  const determinate = !!stage && stage.total > 0 && (phase === "upload" || phase === "unpack");
  const pct = determinate ? Math.min(100, Math.round((stage!.done / stage!.total) * 100)) : null;
  return (
    <div className="world-import-progress" role="status" aria-live="polite">
      <div className="world-import-progress-head">
        <span>{t(`worldImport.phase_${phase}`)}</span>
        <span className="world-import-progress-num">
          {determinate ? `${formatBytes(stage!.done)} / ${formatBytes(stage!.total)} · ${pct}%` : ""}
        </span>
      </div>
      <div
        className="world-import-bar"
        data-indeterminate={determinate ? undefined : ""}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct ?? undefined}
        aria-label={t(`worldImport.phase_${phase}`)}
      >
        <span style={{ width: determinate ? `${pct}%` : undefined }} />
      </div>
    </div>
  );
}

export function WorldImportCard({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const remote = isRemote();
  const [source, setSource] = useState<{ path: string; kind: "zip" | "folder" } | null>(null);
  const [done, setDone] = useState<WorldImportResult | null>(null);
  const running = ["online", "starting", "stopping"].includes(instance.status);

  const run = useMutation({
    mutationFn: ({ path, skipBackup }: { path: string; skipBackup: boolean }) =>
      api.worlds.import(instance.id, path, skipBackup),
    onSuccess: (result) => {
      setSource(null);
      setDone(result);
      void client.invalidateQueries({ queryKey: keys.backups(instance.id) });
      void client.invalidateQueries({ queryKey: keys.instances });
    },
  });

  const pick = async (kind: "zip" | "folder") => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open(
      kind === "zip"
        ? { multiple: false, filters: [{ name: "ZIP", extensions: ["zip"] }], title: t("worldImport.pickZip") }
        : { multiple: false, directory: true, title: t("worldImport.pickFolder") },
    );
    if (typeof picked === "string") {
      run.reset();
      setDone(null);
      setSource({ path: picked, kind });
    }
  };

  const stage = useImportProgress(instance.id, run.isPending);
  const dims = (list: string[]) => list.map((d) => t(`worldImport.dim_${d}`)).join(", ");

  return (
    <Panel title={t("worldImport.title")}>
      <div className="flex flex-col gap-3 px-5 py-4">
        <p className="m-0 max-w-[68ch] text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
          {remote ? t("worldImport.introRemote") : t("worldImport.intro")}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <Button icon="upload" disabled={running || run.isPending} onClick={() => void pick("zip")}>
            {t("worldImport.fromZip")}
          </Button>
          {remote ? null : (
            <Button icon="folder" disabled={running || run.isPending} onClick={() => void pick("folder")}>
              {t("worldImport.fromFolder")}
            </Button>
          )}
          {running ? (
            <span className="inline-flex items-center gap-1.5 text-[length:var(--text-xs)] text-[color:var(--status-starting)]">
              <Icon name="warning" size={12} /> {t("worldImport.stopFirst")}
            </span>
          ) : null}
        </div>
        {done ? (
          <p className="m-0 inline-flex items-start gap-2 text-[length:var(--text-sm)] text-[color:var(--status-online)]">
            <Icon name="check" size={14} />
            <span>
              {t("worldImport.done", { source: done.sourceName, level: done.levelName, dims: dims(done.dimensions) })}
            </span>
          </p>
        ) : null}
        {run.error ? <ErrorNotice error={run.error} compact /> : null}
      </div>

      {source ? (
        <Dialog
          open
          onClose={() => (run.isPending ? undefined : setSource(null))}
          title={t("worldImport.confirmTitle")}
          footer={
            <>
              <Button disabled={run.isPending} onClick={() => setSource(null)}>
                {t("actions.cancel")}
              </Button>
              <Button
                variant="danger"
                disabled={run.isPending}
                onClick={() => run.mutate({ path: source.path, skipBackup: true })}
              >
                {t("worldImport.withoutBackup")}
              </Button>
              <Button
                variant="primary"
                disabled={run.isPending}
                onClick={() => run.mutate({ path: source.path, skipBackup: false })}
              >
                {run.isPending ? (remote ? t("worldImport.uploading") : t("worldImport.importing")) : t("worldImport.confirm")}
              </Button>
            </>
          }
        >
          <div className="flex flex-col gap-3">
            <code className="selectable break-all text-[length:var(--text-xs)]">{source.path}</code>
            <p className="m-0 text-[length:var(--text-sm)] text-[color:var(--text-muted)]">{t("worldImport.confirmBody")}</p>
            <p className="m-0 text-[length:var(--text-xs)] text-[color:var(--text-faint)]">{t("worldImport.withoutBackupHelp")}</p>
            {run.isPending ? <ImportProgress stage={stage} /> : null}
            {run.error ? <ErrorNotice error={run.error} compact /> : null}
          </div>
        </Dialog>
      ) : null}
    </Panel>
  );
}
