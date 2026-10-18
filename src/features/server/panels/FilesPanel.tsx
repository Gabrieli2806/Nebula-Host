/**
 * Browsing and editing the server's own files, plus copying files and
 * folders in from this computer: native drag and drop onto the list (or onto
 * a folder row / breadcrumb), or the upload pickers in the header.
 *
 * Paths from the OS never travel as bytes through IPC: the backend copies
 * them (`files_import`) and reports progress as `import` events.
 */

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { keys, useImportFiles, useWindowDragDrop } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api, toAppError, type AppError } from "@/lib/ipc";
import { isRemote } from "@/lib/connection";
import { formatBytes, formatDateTime } from "@/lib/format";
import type {
  ConflictPolicy,
  ImportCheck,
  ImportItem,
  ImportSummary,
  InstanceView,
  WindowDragDrop,
} from "@/types/api";
import { Button, Dialog, EmptyState, ErrorNotice, Icon, Meter, Panel, Skeleton } from "@/ui";

import "./FilesPanel.css";

/** Extensions worth opening in the editor rather than the file manager. */
const EDITABLE = /\.(properties|txt|json|ya?ml|toml|cfg|conf|log|md|snbt)$/i;

/** How long a clean "copied" notice stays before it clears itself. */
const NOTICE_TIMEOUT_MS = 8000;

interface Target {
  path: string;
  name: string;
}

/** A pending copy: where to, and the absolute OS paths to copy. */
interface ImportRequest {
  target: string;
  sources: string[];
}

/** The live native drag, while the pointer is over this panel. */
interface DragState {
  count: number;
  target: string;
  /** True when the pointer is on a folder row or crumb, not the list itself. */
  onFolder: boolean;
}

/** Stepping through the names that already exist in the target folder. */
interface ConflictState {
  request: ImportRequest;
  check: ImportCheck;
  index: number;
  choices: Record<string, ConflictPolicy>;
  applyToAll: boolean;
}

type Status =
  | { kind: "idle" }
  | { kind: "checking"; request: ImportRequest }
  | {
      kind: "running";
      request: ImportRequest;
      totalFiles: number;
      totalBytes: number;
      /** Files and bytes copied by the calls already finished. */
      doneFiles: number;
      doneBytes: number;
    }
  | { kind: "done"; request: ImportRequest; summary: ImportSummary }
  | { kind: "failed"; request: ImportRequest; error: AppError };

function baseName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

/** What "keep both" will likely call the copy (the backend picks the number). */
function numberedCopy(item: ImportItem) {
  const dot = item.isDir ? -1 : item.name.lastIndexOf(".");
  return dot > 0 ? `${item.name.slice(0, dot)} (1)${item.name.slice(dot)}` : `${item.name} (1)`;
}

function mergeSummaries(a: ImportSummary | null, b: ImportSummary): ImportSummary {
  if (!a) return b;
  return {
    ...b,
    copiedFiles: a.copiedFiles + b.copiedFiles,
    copiedBytes: a.copiedBytes + b.copiedBytes,
    imported: [...a.imported, ...b.imported],
    skipped: [...a.skipped, ...b.skipped],
    failed: [...a.failed, ...b.failed],
  };
}

export function FilesPanel({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const [path, setPath] = useState("");
  const [editing, setEditing] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<Target | null>(null);

  const listing = useQuery({
    queryKey: keys.files(instance.id, path),
    queryFn: () => api.files.list(instance.id, path),
  });

  const remove = useMutation({
    mutationFn: (target: string) => api.files.remove(instance.id, target),
    onSuccess: () => {
      setDeleting(null);
      return client.invalidateQueries({ queryKey: keys.files(instance.id, path) });
    },
  });

  const importer = useImportFiles(instance.id);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [conflict, setConflict] = useState<ConflictState | null>(null);
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const dragCount = useRef(0);

  const number = useCallback((value: number) => new Intl.NumberFormat(language).format(value), [language]);
  const folderLabel = useCallback(
    (target: string) => (target ? target : t("files.rootFolder")),
    [t],
  );

  const busy = status.kind === "checking" || status.kind === "running" || conflict !== null;

  // -- the copy itself ------------------------------------------------------

  /** Runs one backend call per conflict policy, one after another. */
  const runImport = useCallback(
    async (request: ImportRequest, check: ImportCheck, choices: Record<string, ConflictPolicy>) => {
      const nameOf = (source: string, index: number) =>
        check.items[index]?.name ?? baseName(source);

      // Sources without a conflict ride along with the first group: the
      // policy only changes what happens to names that already exist.
      const groups: { policy: ConflictPolicy; sources: string[]; files: number; bytes: number }[] = [];
      const fallback = choices[check.conflicts[0] ?? ""] ?? "keep_both";
      request.sources.forEach((source, index) => {
        const name = nameOf(source, index);
        const policy = choices[name] ?? fallback;
        const item = check.items.find((entry) => entry.name === name);
        let group = groups.find((entry) => entry.policy === policy);
        if (!group) {
          group = { policy, sources: [], files: 0, bytes: 0 };
          groups.push(group);
        }
        group.sources.push(source);
        group.files += item?.files ?? 0;
        group.bytes += item?.bytes ?? 0;
      });

      let summary: ImportSummary | null = null;
      let doneFiles = 0;
      let doneBytes = 0;
      try {
        for (const group of groups) {
          setStatus({
            kind: "running",
            request,
            totalFiles: check.totalFiles,
            totalBytes: check.totalBytes,
            doneFiles,
            doneBytes,
          });
          const result = await importer.run(request.target, group.sources, group.policy);
          summary = mergeSummaries(summary, result);
          doneFiles += group.files;
          doneBytes += group.bytes;
        }
        if (summary) setStatus({ kind: "done", request, summary });
        else setStatus({ kind: "idle" });
      } catch (raw) {
        setStatus({ kind: "failed", request, error: toAppError(raw) });
      }
    },
    [importer],
  );

  /** Dry run first: conflicts open the dialog, anything else copies at once. */
  const startImport = useCallback(
    async (request: ImportRequest) => {
      if (request.sources.length === 0) return;
      importer.reset();
      setStatus({ kind: "checking", request });
      let check: ImportCheck;
      try {
        check = await importer.check(request.target, request.sources);
      } catch (raw) {
        setStatus({ kind: "failed", request, error: toAppError(raw) });
        return;
      }
      if (check.conflicts.length > 0) {
        setStatus({ kind: "idle" });
        setConflict({ request, check, index: 0, choices: {}, applyToAll: false });
        return;
      }
      await runImport(request, check, {});
    },
    [importer, runImport],
  );

  const resolveConflict = (policy: ConflictPolicy) => {
    if (!conflict) return;
    const { check, index, applyToAll } = conflict;
    const choices = { ...conflict.choices };
    const remaining = applyToAll ? check.conflicts.slice(index) : check.conflicts.slice(index, index + 1);
    for (const name of remaining) choices[name] = policy;
    const next = applyToAll ? check.conflicts.length : index + 1;
    if (next < check.conflicts.length) {
      setConflict({ ...conflict, index: next, choices });
      return;
    }
    setConflict(null);
    void runImport(conflict.request, check, choices);
  };

  const cancelConflict = () => {
    setConflict(null);
    setStatus({ kind: "idle" });
  };

  // A clean result clears itself; anything with a skip or failure stays
  // until it is dismissed, so it can be read.
  useEffect(() => {
    if (status.kind !== "done") return;
    const { summary } = status;
    if (summary.failed.length > 0 || summary.skipped.length > 0) return;
    const timer = window.setTimeout(() => setStatus({ kind: "idle" }), NOTICE_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [status]);

  // -- native drag and drop -------------------------------------------------

  /** Where a drop at (x, y) would land, or null when it is not on this panel. */
  const dropTargetAt = useCallback(
    (x: number, y: number): { target: string; onFolder: boolean } | null => {
      const element = document.elementFromPoint(x, y);
      if (!element?.closest(".files-panel")) return null;
      const folder = element.closest<HTMLElement>("[data-drop-dir]");
      if (folder) return { target: folder.dataset.dropDir ?? "", onFolder: true };
      return { target: path, onFolder: false };
    },
    [path],
  );

  const blocked = busy || deleting !== null;

  const onDrag = useCallback(
    (event: WindowDragDrop) => {
      if (event.type === "leave") {
        setDrag(null);
        return;
      }
      if (event.type === "enter") dragCount.current = event.paths.length;
      if (blocked || dragCount.current === 0) {
        setDrag(null);
        return;
      }
      const hit = dropTargetAt(event.position.x, event.position.y);
      if (event.type === "drop") {
        setDrag(null);
        if (hit) void startImport({ target: hit.target, sources: event.paths });
        return;
      }
      setDrag(hit ? { count: dragCount.current, ...hit } : null);
    },
    [blocked, dropTargetAt, startImport],
  );

  useWindowDragDrop(onDrag, editing === null && !isRemote());

  const pick = async (directory: boolean) => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: true,
        directory,
        title: directory ? t("files.uploadFolder") : t("files.upload"),
      });
      if (!picked) return;
      void startImport({ target: path, sources: Array.isArray(picked) ? picked : [picked] });
    } catch (raw) {
      setStatus({ kind: "failed", request: { target: path, sources: [] }, error: toAppError(raw) });
    }
  };

  const segments = path ? path.split("/") : [];

  if (editing) {
    return <FileEditor instance={instance} path={editing} onClose={() => setEditing(null)} />;
  }

  const closeDelete = () => {
    setDeleting(null);
    remove.reset();
  };

  const currentConflict = conflict
    ? conflict.check.items.find((item) => item.name === conflict.check.conflicts[conflict.index]) ?? {
        name: conflict.check.conflicts[conflict.index] ?? "",
        isDir: false,
        exists: true,
        typeMismatch: false,
        files: 0,
        bytes: 0,
      }
    : null;
  const conflictsLeft = conflict ? conflict.check.conflicts.length - conflict.index - 1 : 0;
  // A running server keeps its world's region files and its jars open:
  // overwriting them now can corrupt the world or be undone by the next
  // autosave. Overwrite waits until the server is stopped.
  const serverLive = instance.status === "online" || instance.status === "starting";
  const overwriteBlocked =
    serverLive &&
    currentConflict !== null &&
    !currentConflict.typeMismatch &&
    (currentConflict.isDir || /\.(jar|mca|dat|db)$/i.test(currentConflict.name));

  return (
    <Panel
      className="files-panel"
      title={
        <nav className="files-crumbs" aria-label={t("files.title")}>
          <button
            type="button"
            className="files-crumb files-crumb-root"
            aria-current={segments.length === 0 ? "location" : undefined}
            data-drop-dir=""
            data-drop-target={drag?.onFolder && drag.target === "" ? "" : undefined}
            onClick={() => setPath("")}
            aria-label={t("overview.diskFolder")}
            title={t("overview.diskFolder")}
          >
            {/* Icon only: the page title above already says "Files". */}
            <Icon name="folder" size={16} />
          </button>
          {segments.map((segment, index) => {
            const crumbPath = segments.slice(0, index + 1).join("/");
            return (
              <span key={segment + index} className="flex min-w-0 items-center gap-1">
                <span className="files-crumb-sep" aria-hidden="true">
                  <Icon name="chevron-right" size={12} />
                </span>
                <button
                  type="button"
                  className="files-crumb"
                  aria-current={index === segments.length - 1 ? "location" : undefined}
                  data-drop-dir={crumbPath}
                  data-drop-target={drag?.onFolder && drag.target === crumbPath ? "" : undefined}
                  onClick={() => setPath(crumbPath)}
                >
                  {segment}
                </button>
              </span>
            );
          })}
        </nav>
      }
      actions={
        <>
          <Button size="sm" icon="folder" disabled={busy || isRemote()} title={isRemote() ? t("remote.localOnly") : undefined} onClick={() => void pick(true)}>
            {t("files.uploadFolder")}
          </Button>
          <Button size="sm" icon="upload" disabled={busy || isRemote()} title={isRemote() ? t("remote.localOnly") : undefined} onClick={() => void pick(false)}>
            {t("files.upload")}
          </Button>
          <Button
            size="sm"
            variant="quiet"
            icon="external"
            disabled={isRemote()}
            title={isRemote() ? t("remote.localOnly") : undefined}
            onClick={async () => {
              const folder = await api.files.reveal(instance.id);
              const { openPath } = await import("@tauri-apps/plugin-opener");
              await openPath(folder);
            }}
          >
            {t("actions.openFolder")}
          </Button>
        </>
      }
      bodyClassName="files-body"
    >
      <ImportStatus
        status={status}
        progress={importer.progress}
        folderLabel={folderLabel}
        number={number}
        onDismiss={() => {
          importer.reset();
          setStatus({ kind: "idle" });
        }}
        onRetry={(request) => void startImport(request)}
      />

      {listing.isLoading ? (
        <div className="flex flex-col gap-2 p-4" aria-hidden="true">
          {[0, 1, 2, 3, 4].map((index) => (
            <Skeleton key={index} className="h-7" />
          ))}
        </div>
      ) : null}

      {listing.error ? (
        <div className="p-4">
          <ErrorNotice error={listing.error} onRetry={() => void listing.refetch()} />
        </div>
      ) : null}

      {listing.data?.length === 0 ? (
        <EmptyState icon="folder" title={t("files.empty")} hint={t("files.dropHint")} />
      ) : null}

      {listing.data?.length ? (
        <>
          <div className="files-table" role="table" aria-label={t("files.title")}>
            <div className="files-row files-head" role="row">
              <span aria-hidden="true" />
              <span className="label" role="columnheader">
                {t("files.name")}
              </span>
              <span className="label text-right" role="columnheader">
                {t("files.size")}
              </span>
              <span className="label files-col-date" role="columnheader">
                {t("files.modified")}
              </span>
              <span aria-hidden="true" />
            </div>
            {listing.data.map((entry) => {
              const editable = !entry.isDir && EDITABLE.test(entry.name);
              const kind = entry.isDir ? "dir" : editable ? "editable" : "file";
              return (
                <div
                  key={entry.path}
                  className="files-row panel-row"
                  role="row"
                  data-kind={kind}
                  data-drop-dir={entry.isDir ? entry.path : undefined}
                  data-drop-target={drag?.onFolder && entry.isDir && drag.target === entry.path ? "" : undefined}
                >
                  <span className="files-icon" aria-hidden="true">
                    <Icon name={entry.isDir ? "folder" : "file"} />
                  </span>
                  <span role="cell" className="min-w-0">
                    {entry.isDir || editable ? (
                      <button
                        type="button"
                        className="files-name"
                        onClick={() => {
                          if (entry.isDir) setPath(entry.path);
                          else setEditing(entry.path);
                        }}
                      >
                        {entry.name}
                      </button>
                    ) : (
                      <span className="files-name">{entry.name}</span>
                    )}
                  </span>
                  <span role="cell" className="mono files-meta text-right">
                    {entry.isDir ? "" : formatBytes(entry.size)}
                  </span>
                  <span role="cell" className="mono files-meta files-col-date">
                    {formatDateTime(entry.modified, language)}
                  </span>
                  <span role="cell" className="flex justify-end">
                    <Button
                      size="sm"
                      variant="quiet"
                      icon="trash"
                      iconOnly
                      className="row-action"
                      onClick={() => setDeleting({ path: entry.path, name: entry.name })}
                    >
                      {t("actions.delete")}
                    </Button>
                  </span>
                </div>
              );
            })}
          </div>
          <p className="files-foot">{t("files.dropHint")}</p>
        </>
      ) : null}

      {drag ? (
        <div className="files-drop" data-on-folder={drag.onFolder ? "" : undefined} aria-hidden="true">
          <div className="files-drop-banner">
            <Icon name="arrow-down" size={14} />
            <span className="min-w-0 truncate">
              {t("files.dropInto", { folder: folderLabel(drag.target) })}
            </span>
            <span className="files-drop-count mono">
              {drag.count === 1 ? t("files.itemsOne") : t("files.itemsMany", { count: number(drag.count) })}
            </span>
          </div>
        </div>
      ) : null}

      <Dialog
        open={conflict !== null && currentConflict !== null}
        onClose={cancelConflict}
        title={t("files.conflictTitle", { name: currentConflict?.name ?? "" })}
        width={540}
        header={
          conflict && conflict.check.conflicts.length > 1 ? (
            <p className="files-conflict-step">
              {t("files.conflictStep", {
                index: number(conflict.index + 1),
                total: number(conflict.check.conflicts.length),
              })}
            </p>
          ) : undefined
        }
        footer={
          <>
            <Button variant="quiet" onClick={cancelConflict} className="mr-auto">
              {t("actions.cancel")}
            </Button>
            <Button onClick={() => resolveConflict("skip")}>{t("files.skip")}</Button>
            <Button onClick={() => resolveConflict("keep_both")}>{t("files.keepBoth")}</Button>
            <Button
              variant="danger"
              disabled={overwriteBlocked}
              onClick={() => resolveConflict("overwrite")}
            >
              {t("files.overwrite")}
            </Button>
          </>
        }
      >
        {conflict && currentConflict ? (
          <div className="flex flex-col gap-4">
            <p className="text-[color:var(--text-muted)]">
              {t(currentConflict.isDir ? "files.conflictFolder" : "files.conflictFile", {
                folder: folderLabel(conflict.request.target),
              })}
            </p>
            <div className="files-conflict-item well">
              <Icon name={currentConflict.isDir ? "folder" : "file"} />
              <span className="min-w-0 flex-1 truncate">{currentConflict.name}</span>
              <span className="mono files-meta">
                {t("files.incoming", {
                  what: currentConflict.isDir
                    ? t("files.filesAndSize", {
                        files: number(currentConflict.files),
                        size: formatBytes(currentConflict.bytes),
                      })
                    : formatBytes(currentConflict.bytes),
                })}
              </span>
            </div>
            {overwriteBlocked ? (
              <p className="files-conflict-live" role="status">
                <Icon name="warning" size={14} />
                <span>{t("files.conflictLive")}</span>
              </p>
            ) : null}
            <dl className="files-conflict-options">
              <dt>{t("files.overwrite")}</dt>
              <dd>
                {currentConflict.typeMismatch
                  ? t("files.overwriteMismatchHint")
                  : currentConflict.isDir
                    ? t("files.overwriteFolderHint")
                    : t("files.overwriteFileHint")}
              </dd>
              <dt>{t("files.keepBoth")}</dt>
              <dd>{t("files.keepBothHint", { name: numberedCopy(currentConflict) })}</dd>
              <dt>{t("files.skip")}</dt>
              <dd>{t("files.skipHint")}</dd>
            </dl>
            {conflictsLeft > 0 ? (
              <label className="files-apply-all">
                <input
                  type="checkbox"
                  checked={conflict.applyToAll}
                  onChange={(event) => setConflict({ ...conflict, applyToAll: event.target.checked })}
                />
                <span>
                  {conflictsLeft === 1
                    ? t("files.applyToAllOne")
                    : t("files.applyToAllMany", { count: number(conflictsLeft) })}
                </span>
              </label>
            ) : null}
          </div>
        ) : null}
      </Dialog>

      <Dialog
        open={deleting !== null}
        onClose={closeDelete}
        title={t("files.deleteConfirm", { name: deleting?.name ?? "" })}
        width={460}
        footer={
          <>
            <Button variant="quiet" onClick={closeDelete}>
              {t("actions.cancel")}
            </Button>
            <Button
              variant="danger"
              icon="trash"
              disabled={remove.isPending}
              onClick={() => deleting && remove.mutate(deleting.path)}
            >
              {t("actions.delete")}
            </Button>
          </>
        }
      >
        <p className="text-[color:var(--text-muted)]">{t("files.deleteWarning")}</p>
        {deleting ? (
          <p className="mono selectable mt-3 truncate text-[length:var(--text-sm)] text-[color:var(--text-faint)]">
            {deleting.path}
          </p>
        ) : null}
        {remove.error ? (
          <div className="mt-4">
            <ErrorNotice error={remove.error} compact />
          </div>
        ) : null}
      </Dialog>
    </Panel>
  );
}

/** The strip above the list: checking, copying (with progress), result. */
function ImportStatus({
  status,
  progress,
  folderLabel,
  number,
  onDismiss,
  onRetry,
}: {
  status: Status;
  progress: ReturnType<typeof useImportFiles>["progress"];
  folderLabel: (target: string) => string;
  number: (value: number) => string;
  onDismiss: () => void;
  onRetry: (request: ImportRequest) => void;
}) {
  const { t, describeError } = useI18n();

  let body: ReactNode = null;

  if (status.kind === "checking") {
    body = (
      <div className="files-status" data-tone="neutral">
        <p className="files-status-title">{t("files.checking")}</p>
      </div>
    );
  }

  if (status.kind === "running") {
    const copiedFiles = status.doneFiles + (progress?.copiedFiles ?? 0);
    const copiedBytes = status.doneBytes + (progress?.copiedBytes ?? 0);
    const totalBytes = Math.max(status.totalBytes, copiedBytes);
    body = (
      <div className="files-status" data-tone="neutral">
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <div className="flex items-baseline justify-between gap-3">
            <p className="files-status-title truncate">
              {t("files.importing", { folder: folderLabel(status.request.target) })}
            </p>
            <span className="mono files-meta">
              {t("files.importProgress", {
                files: number(copiedFiles),
                total: number(Math.max(status.totalFiles, copiedFiles)),
                size: formatBytes(copiedBytes),
                totalSize: formatBytes(totalBytes),
              })}
            </span>
          </div>
          <Meter value={copiedBytes} max={totalBytes || 1} label={undefined} />
          {progress?.current ? <p className="mono files-status-current truncate">{progress.current}</p> : null}
        </div>
      </div>
    );
  }

  if (status.kind === "done") {
    const { summary, request } = status;
    const folder = folderLabel(request.target);
    const copied = summary.imported.length;
    const renamed = summary.imported.filter((entry) => entry.renamed).length;
    const clean = summary.failed.length === 0 && summary.skipped.length === 0;
    const title =
      copied === 0
        ? t("files.importNothing")
        : copied === 1
          ? t("files.importDoneOne", { name: summary.imported[0]?.sourceName ?? "", folder })
          : t("files.importDoneMany", { count: number(copied), folder });
    const facts = [
      copied > 0 ? t("files.filesAndSize", { files: number(summary.copiedFiles), size: formatBytes(summary.copiedBytes) }) : null,
      renamed > 0 ? (renamed === 1 ? t("files.renamedOne") : t("files.renamedMany", { count: number(renamed) })) : null,
      summary.skipped.length > 0
        ? summary.skipped.length === 1
          ? t("files.skippedOne")
          : t("files.skippedMany", { count: number(summary.skipped.length) })
        : null,
    ].filter(Boolean);

    body = (
      <div className="files-status" data-tone={summary.failed.length > 0 ? "warn" : clean ? "ok" : "neutral"}>
        <span className="files-status-icon" aria-hidden="true">
          <Icon name={summary.failed.length > 0 ? "warning" : "check"} />
        </span>
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <p className="files-status-title">{title}</p>
          {facts.length > 0 ? <p className="files-status-facts">{facts.join(" · ")}</p> : null}
          {summary.failed.length > 0 ? (
            <div className="mt-1 flex flex-col gap-1">
              <p className="files-status-facts">
                {summary.failed.length === 1
                  ? t("files.failedOne")
                  : t("files.failedMany", { count: number(summary.failed.length) })}
              </p>
              <ul className="files-status-failed">
                {summary.failed.slice(0, 6).map((entry) => (
                  <li key={entry.path}>
                    <span className="mono truncate">{entry.path}</span>
                    <span>{describeError({ code: entry.code, params: {} }).title}</span>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </div>
        <Button size="sm" variant="quiet" icon="close" iconOnly onClick={onDismiss}>
          {t("dialog.close")}
        </Button>
      </div>
    );
  }

  if (status.kind === "failed") {
    body = (
      <div className="files-status-error">
        <ErrorNotice
          error={status.error}
          compact={status.request.sources.length === 0}
          onRetry={status.request.sources.length > 0 ? () => onRetry(status.request) : undefined}
          className="flex-1"
        />
        <Button size="sm" variant="quiet" icon="close" iconOnly onClick={onDismiss}>
          {t("dialog.close")}
        </Button>
      </div>
    );
  }

  return (
    <div className="files-status-region" role="status" aria-live="polite">
      {body}
    </div>
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

  const client = useQueryClient();
  const save = useMutation({
    mutationFn: (contents: string) => api.files.write(instance.id, path, contents),
    // The saved text becomes the file's text, so dropping the draft does not
    // show the copy read before the save.
    onSuccess: (_, contents) => {
      client.setQueryData(["file", instance.id, path], contents);
      setDraft(null);
    },
  });

  const contents = draft ?? file.data ?? "";
  const dirty = draft !== null && draft !== file.data;
  const name = path.split("/").pop() ?? path;

  return (
    <Panel
      title={
        <span className="flex min-w-0 items-baseline gap-3">
          <span className="truncate">{t("files.editing", { name })}</span>
          {path !== name ? <span className="mono files-editor-path truncate">{path}</span> : null}
        </span>
      }
      actions={
        <>
          {dirty ? (
            <span className="tag" data-tone="warn">
              {t("files.unsaved")}
            </span>
          ) : null}
          <Button size="sm" variant="quiet" icon="chevron-left" onClick={onClose}>
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
      bodyClassName="flex flex-col p-3"
    >
      {file.error ? (
        <ErrorNotice error={file.error} onRetry={() => void file.refetch()} />
      ) : (
        <textarea
          className="field mono selectable files-editor"
          aria-label={t("files.editing", { name })}
          spellCheck={false}
          value={contents}
          onChange={(event) => setDraft(event.target.value)}
        />
      )}

      {save.error ? (
        <div className="pt-3">
          <ErrorNotice error={save.error} compact />
        </div>
      ) : null}
    </Panel>
  );
}
