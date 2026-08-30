/** Browsing and editing the server's own files. */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { formatBytes, formatDateTime } from "@/lib/format";
import type { InstanceView } from "@/types/api";
import { Button, EmptyState, ErrorNotice, Icon, Panel, Skeleton } from "@/ui";

/** Extensions worth opening in the editor rather than the file manager. */
const EDITABLE = /\.(properties|txt|json|ya?ml|toml|cfg|conf|log|md|snbt)$/i;

export function FilesPanel({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const [path, setPath] = useState("");
  const [editing, setEditing] = useState<string | null>(null);

  const listing = useQuery({
    queryKey: keys.files(instance.id, path),
    queryFn: () => api.files.list(instance.id, path),
  });

  const remove = useMutation({
    mutationFn: (target: string) => api.files.remove(instance.id, target),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.files(instance.id, path) }),
  });

  const segments = path ? path.split("/") : [];

  if (editing) {
    return <FileEditor instance={instance} path={editing} onClose={() => setEditing(null)} />;
  }

  return (
    <Panel
      title={
        <nav className="flex items-center gap-1 text-[length:var(--text-sm)]">
          <button type="button" className="hover:underline" onClick={() => setPath("")}>
            {instance.name}
          </button>
          {segments.map((segment, index) => (
            <span key={segment + index} className="flex items-center gap-1">
              <Icon name="chevron-right" size={12} />
              <button
                type="button"
                className="hover:underline"
                onClick={() => setPath(segments.slice(0, index + 1).join("/"))}
              >
                {segment}
              </button>
            </span>
          ))}
        </nav>
      }
      actions={
        <Button
          size="sm"
          icon="external"
          onClick={async () => {
            const folder = await api.files.reveal(instance.id);
            const { openPath } = await import("@tauri-apps/plugin-opener");
            await openPath(folder);
          }}
        >
          {t("actions.openFolder")}
        </Button>
      }
    >
      {listing.isLoading ? (
        <div className="flex flex-col gap-2 p-4">
          {[0, 1, 2, 3].map((index) => (
            <Skeleton key={index} className="h-8" />
          ))}
        </div>
      ) : null}

      {listing.error ? (
        <div className="p-4">
          <ErrorNotice error={listing.error} onRetry={() => void listing.refetch()} />
        </div>
      ) : null}

      {listing.data?.length === 0 ? <EmptyState icon="folder" title={t("files.empty")} /> : null}

      {listing.data?.length ? (
        <ul>
          {listing.data.map((entry) => (
            <li
              key={entry.path}
              className="flex items-center gap-3 border-b border-[color:var(--line)] px-4 py-2 last:border-0 hover:bg-[color:var(--surface-raised)]"
            >
              <span className="flex-none text-[color:var(--text-faint)]">
                <Icon name={entry.isDir ? "folder" : "file"} />
              </span>
              <button
                type="button"
                className="min-w-0 flex-1 truncate text-left"
                onClick={() => {
                  if (entry.isDir) setPath(entry.path);
                  else if (EDITABLE.test(entry.name)) setEditing(entry.path);
                }}
              >
                {entry.name}
              </button>
              <span className="mono flex-none text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
                {entry.isDir ? "" : formatBytes(entry.size)}
              </span>
              <span className="mono hidden flex-none text-[length:var(--text-xs)] text-[color:var(--text-faint)] sm:block">
                {formatDateTime(entry.modified, language)}
              </span>
              <Button
                size="sm"
                variant="quiet"
                icon="trash"
                iconOnly
                onClick={() => remove.mutate(entry.path)}
              >
                {t("actions.delete")}
              </Button>
            </li>
          ))}
        </ul>
      ) : null}

      {remove.error ? (
        <div className="p-4">
          <ErrorNotice error={remove.error} compact />
        </div>
      ) : null}
    </Panel>
  );
}

function FileEditor({
  instance,
  path,
  onClose,
}: {
  instance: InstanceView;
  path: string;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<string | null>(null);

  const file = useQuery({
    queryKey: ["file", instance.id, path],
    queryFn: () => api.files.read(instance.id, path),
  });

  const save = useMutation({
    mutationFn: (contents: string) => api.files.write(instance.id, path, contents),
    onSuccess: () => setDraft(null),
  });

  const contents = draft ?? file.data ?? "";
  const dirty = draft !== null && draft !== file.data;

  return (
    <Panel
      title={t("files.editing", { name: path.split("/").pop() ?? path })}
      actions={
        <>
          {dirty ? (
            <span className="text-[length:var(--text-xs)] text-[color:var(--status-starting)]">
              {t("files.unsaved")}
            </span>
          ) : null}
          <Button size="sm" variant="quiet" onClick={onClose}>
            {t("actions.back")}
          </Button>
          <Button
            size="sm"
            variant="primary"
            disabled={!dirty || save.isPending}
            onClick={() => save.mutate(contents)}
          >
            {save.isSuccess && !dirty ? t("actions.saved") : t("actions.save")}
          </Button>
        </>
      }
      bodyClassName="flex flex-col"
    >
      {file.error ? (
        <div className="p-4">
          <ErrorNotice error={file.error} onRetry={() => void file.refetch()} />
        </div>
      ) : (
        <textarea
          className="field mono selectable min-h-[420px] flex-1 rounded-none border-0"
          spellCheck={false}
          value={contents}
          onChange={(event) => setDraft(event.target.value)}
        />
      )}

      {save.error ? (
        <div className="p-4">
          <ErrorNotice error={save.error} compact />
        </div>
      ) : null}
    </Panel>
  );
}
