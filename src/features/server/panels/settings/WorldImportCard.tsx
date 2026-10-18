/**
 * Import a world: a singleplayer save or the worlds of a previous server.
 * On this PC from a `.zip` or straight from its folder; on a remote machine
 * from a `.zip`, streamed there first. The current world is backed up before
 * it is replaced, and the server must be stopped.
 */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { isRemote } from "@/lib/connection";
import { api } from "@/lib/ipc";
import type { InstanceView, WorldImportResult } from "@/types/api";
import { Button, Dialog, ErrorNotice, Icon, Panel } from "@/ui";

export function WorldImportCard({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const remote = isRemote();
  const [source, setSource] = useState<{ path: string; kind: "zip" | "folder" } | null>(null);
  const [done, setDone] = useState<WorldImportResult | null>(null);
  const running = ["online", "starting", "stopping"].includes(instance.status);

  const run = useMutation({
    mutationFn: (path: string) => api.worlds.import(instance.id, path),
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
              <Button variant="primary" disabled={run.isPending} onClick={() => run.mutate(source.path)}>
                {run.isPending ? (remote ? t("worldImport.uploading") : t("worldImport.importing")) : t("worldImport.confirm")}
              </Button>
            </>
          }
        >
          <div className="flex flex-col gap-3">
            <code className="selectable break-all text-[length:var(--text-xs)]">{source.path}</code>
            <p className="m-0 text-[length:var(--text-sm)] text-[color:var(--text-muted)]">{t("worldImport.confirmBody")}</p>
            {run.error ? <ErrorNotice error={run.error} compact /> : null}
          </div>
        </Dialog>
      ) : null}
    </Panel>
  );
}
