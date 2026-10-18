/** What sits in the server's mods/plugins folder, identified through Modrinth. */

import { useMemo, useState } from "react";

import { useAddonMutations, useInstalledAddons, usePendingAddons } from "@/hooks/useBackend";
import { formatBytes } from "@/lib/format";
import type { AddonContext, InstalledAddonEntry, InstanceView, PendingAddonChange } from "@/types/api";
import { Button, Dialog, EmptyState, ErrorNotice, Icon, Panel, Skeleton, Switch, TextInput } from "@/ui";

import { AddonIcon } from "./AddonIcon";
import { PendingChanges } from "./PendingChanges";
import { type AddonView, pendingFor, useAddonCopy } from "./util";
import type { ProjectTarget } from "./ProjectDialog";

export function InstalledTab({
  instance,
  context,
  view,
  onOpenProject,
  onBrowse,
}: {
  instance: InstanceView;
  context: AddonContext;
  view: AddonView;
  onOpenProject: (target: ProjectTarget) => void;
  onBrowse: () => void;
}) {
  const { t, tc } = useAddonCopy();
  const report = useInstalledAddons(instance.id);
  const { data: pending = [] } = usePendingAddons(instance.id);
  const { update, setEnabled, remove } = useAddonMutations(instance.id);
  // While the process is up, changes queue instead of touching the jars.
  const running = instance.status === "online" || instance.status === "starting" || instance.status === "stopping";
  const [filter, setFilter] = useState("");
  // "N updates available" doubles as a filter to just those.
  const [onlyUpdates, setOnlyUpdates] = useState(false);
  const [removing, setRemoving] = useState<{ fileName: string; title: string } | null>(null);
  const [bulk, setBulk] = useState<{ done: number; total: number; failed: number } | null>(null);

  const addons = useMemo(() => report.data?.addons ?? [], [report.data]);
  const anyUpdates = addons.some((entry) => entry.modrinth?.update_available);
  const updatesOnly = onlyUpdates && anyUpdates;
  const visible = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    return addons.filter((entry) => {
      if (updatesOnly && !entry.modrinth?.update_available) return false;
      if (!needle) return true;
      return [entry.modrinth?.project.title, entry.display_name, entry.file_name]
        .filter(Boolean)
        .some((text) => text!.toLowerCase().includes(needle));
    });
  }, [addons, filter, updatesOnly]);

  if (report.error) {
    return <ErrorNotice error={report.error} onRetry={() => void report.refetch()} />;
  }

  if (report.isLoading || !report.data) {
    return <LoadingRows view={view} />;
  }

  const kind = context.project_type === "plugin" ? "plugin" : "mod";

  if (addons.length === 0) {
    return (
      <div className="flex flex-col gap-3">
        <PendingChanges instance={instance} pending={pending} />
        <Panel>
        <EmptyState
          icon="puzzle"
          title={t(`addons.none_${kind}`)}
          hint={t("addons.noneHint")}
          action={
            <Button icon="search" onClick={onBrowse}>
              {t("addons.browseModrinth")}
            </Button>
          }
        />
        </Panel>
      </div>
    );
  }

  // An update already queued is not offered again.
  const updatable = addons.filter(
    (entry) => entry.modrinth?.update_available && pendingFor(pending, entry.file_name)?.action !== "install",
  );
  const bulkRunning = bulk !== null && bulk.done < bulk.total;

  const updateAll = async () => {
    const files = updatable.map((entry) => entry.file_name);
    setBulk({ done: 0, total: files.length, failed: 0 });
    let failed = 0;
    for (const [index, file] of files.entries()) {
      try {
        await update.mutateAsync(file);
      } catch {
        failed += 1;
      }
      setBulk({ done: index + 1, total: files.length, failed });
    }
    if (failed === 0) setBulk(null);
  };

  const closeRemove = () => {
    setRemoving(null);
    remove.reset();
  };

  const failure = setEnabled.error ?? (bulkRunning ? null : update.error);

  return (
    <div className="flex flex-col gap-3">
      <div className="addons-toolbar">
        {addons.length > 6 ? (
          <span className="addons-search">
            <Icon name="search" />
            <TextInput
              type="search"
              value={filter}
              aria-label={t("addons.filterInstalled")}
              placeholder={t("addons.filterInstalled")}
              onChange={(event) => setFilter(event.target.value)}
            />
          </span>
        ) : null}
        <p className="addons-summary" aria-live="polite">
          <span>{tc("addons.installedCount", addons.length)}</span>
          {updatable.length > 0 ? (
            <button
              type="button"
              className="addons-summary-accent addons-summary-filter"
              aria-pressed={updatesOnly}
              title={updatesOnly ? t("addons.showAllInstalled") : t("addons.showUpdatesOnly")}
              onClick={() => setOnlyUpdates(!updatesOnly)}
            >
              {tc("addons.updatesCount", updatable.length)}
            </button>
          ) : null}
          {report.data.unknown_count > 0 ? (
            <span>{tc("addons.unknownCount", report.data.unknown_count)}</span>
          ) : null}
        </p>
        {updatable.length > 1 ? (
          <Button size="sm" icon="refresh" disabled={bulkRunning || update.isPending} onClick={() => void updateAll()}>
            {bulkRunning
              ? t("addons.updatingProgress", { done: bulk.done + 1, total: bulk.total })
              : t("addons.updateAll")}
          </Button>
        ) : null}
      </div>

      {report.data.modrinth_status === "unavailable" ? (
        <p className="addons-note" role="status">
          <Icon name="warning" />
          <span>{t("addons.modrinthDown")}</span>
          <Button size="sm" variant="quiet" onClick={() => void report.refetch()}>
            {t("actions.retry")}
          </Button>
        </p>
      ) : null}

      {bulk && !bulkRunning && bulk.failed > 0 ? (
        <p className="addons-note" role="status">
          <Icon name="warning" />
          <span>{tc("addons.updateAllFailed", bulk.failed)}</span>
          <Button size="sm" variant="quiet" onClick={() => setBulk(null)}>
            {t("dialog.close")}
          </Button>
        </p>
      ) : null}

      {failure ? <ErrorNotice error={failure} compact /> : null}

      <PendingChanges instance={instance} pending={pending} />

      {visible.length === 0 ? (
        <Panel>
          <EmptyState icon="search" title={t("addons.filterNone", { query: filter.trim() })} />
        </Panel>
      ) : (
        <ul className="addons-collection" data-view={view} aria-label={t("addons.installed")}>
          {visible.map((entry) => (
            <InstalledItem
              key={entry.file_name}
              entry={entry}
              context={context}
              view={view}
              lookedUp={report.data.modrinth_status === "ok"}
              pending={pendingFor(pending, entry.file_name)}
              toggling={setEnabled.isPending && setEnabled.variables?.fileName === entry.file_name}
              updating={update.isPending && update.variables === entry.file_name}
              busy={update.isPending || bulkRunning}
              onToggle={(enabled) => setEnabled.mutate({ fileName: entry.file_name, enabled })}
              onUpdate={() => update.mutate(entry.file_name)}
              onRemove={() =>
                setRemoving({
                  fileName: entry.file_name,
                  title: entry.modrinth?.project.title ?? entry.display_name,
                })
              }
              onOpen={
                entry.modrinth
                  ? () =>
                      onOpenProject({
                        projectId: entry.modrinth!.project.project_id,
                        project: entry.modrinth!.project,
                      })
                  : undefined
              }
            />
          ))}
        </ul>
      )}

      <Dialog
        open={removing !== null}
        onClose={closeRemove}
        title={t("addons.removeConfirm", { name: removing?.title ?? "" })}
        width={460}
        footer={
          <>
            <Button variant="quiet" onClick={closeRemove}>
              {t("actions.cancel")}
            </Button>
            <Button
              variant="danger"
              icon="trash"
              disabled={remove.isPending}
              onClick={() =>
                removing &&
                remove.mutate(removing.fileName, {
                  onSuccess: () => setRemoving(null),
                })
              }
            >
              {t("actions.delete")}
            </Button>
          </>
        }
      >
        <p className="text-[color:var(--text-muted)]">
          {running ? t("addons.removeWarningRunning") : t("addons.removeWarning")}
        </p>
        {removing ? (
          <p className="mono selectable mt-3 truncate text-[length:var(--text-sm)] text-[color:var(--text-faint)]">
            {removing.fileName}
          </p>
        ) : null}
        {remove.error ? (
          <div className="mt-4">
            <ErrorNotice error={remove.error} compact />
          </div>
        ) : null}
      </Dialog>
    </div>
  );
}

function InstalledItem({
  entry,
  context,
  view,
  lookedUp,
  pending,
  toggling,
  updating,
  busy,
  onToggle,
  onUpdate,
  onRemove,
  onOpen,
}: {
  entry: InstalledAddonEntry;
  context: AddonContext;
  view: AddonView;
  /** Modrinth answered, so a missing match really means an unknown jar. */
  lookedUp: boolean;
  /** A change to this jar waiting for the server to stop. */
  pending?: PendingAddonChange;
  toggling: boolean;
  updating: boolean;
  busy: boolean;
  onToggle: (enabled: boolean) => void;
  onUpdate: () => void;
  onRemove: () => void;
  onOpen?: () => void;
}) {
  const { t } = useAddonCopy();
  const match = entry.modrinth;
  const title = match?.project.title ?? entry.display_name;
  const version = match?.installed_version.version_number;
  const queuedInstall = pending?.action === "install" ? pending : null;
  const latest = match?.update_available && !queuedInstall ? match.latest_version : null;
  const incompatible = match?.installed_version.compatible === false;
  // The switch shows where the jar is headed: a queued switch (or the state a
  // queued update will install in) wins over the file as it is now.
  const enabled =
    pending?.action === "set_enabled" ? pending.enabled : queuedInstall ? !queuedInstall.disabled : entry.enabled;
  const removing = pending?.action === "remove";

  const name = onOpen ? (
    <button type="button" className="addons-name addons-name-link" onClick={onOpen}>
      {title}
    </button>
  ) : (
    <span className="addons-name">{title}</span>
  );

  const tags = (
    <>
      {incompatible ? (
        <span className="tag" data-tone="warn" title={t("addons.incompatibleHint", { version: context.game_version })}>
          {t("addons.incompatible", { version: context.game_version })}
        </span>
      ) : null}
      {!match && lookedUp ? <span className="tag">{t("addons.unknownJar")}</span> : null}
      {pending ? (
        <span className="tag addons-pending-tag" data-tone="accent" title={t("addons.pendingWhenStops")}>
          <Icon name="clock" size={11} />
          {pending.action === "remove"
            ? t("addons.pendingRemoval")
            : pending.action === "set_enabled"
              ? t("addons.pendingSwitch", {
                  state: pending.enabled ? t("addons.pendingStateOn") : t("addons.pendingStateOff"),
                })
              : t("addons.pendingUpdateTo", { version: pending.version_number ?? "" })}
        </span>
      ) : null}
    </>
  );

  const actions = (
    <div className="addons-actions">
      {latest ? (
        <Button size="sm" icon="refresh" disabled={busy} onClick={onUpdate}>
          {updating ? t("addons.updating") : t("addons.update", { version: latest.version_number })}
        </Button>
      ) : null}
      <Switch
        checked={enabled}
        label={t("addons.enabledLabel", { name: title })}
        disabled={toggling || removing}
        onChange={onToggle}
      />
      <Button
        size="sm"
        variant="quiet"
        icon="trash"
        iconOnly
        className="row-action"
        disabled={removing}
        onClick={onRemove}
      >
        {t("addons.removeLabel", { name: title })}
      </Button>
    </div>
  );

  const meta = (
    <p className="addons-meta">
      {version ? <span className="mono">{version}</span> : null}
      <span className="mono truncate" title={entry.file_name}>
        {entry.file_name}
      </span>
      <span className="mono">{formatBytes(entry.size_bytes)}</span>
    </p>
  );

  if (view === "list") {
    return (
      <li
        className="addons-row"
        data-disabled={enabled ? undefined : ""}
        data-pending={pending ? "" : undefined}
      >
        <AddonIcon url={match?.project.icon_url} size={36} />
        <div className="addons-row-body">
          <div className="addons-row-head">
            {name}
            {tags}
          </div>
          {meta}
        </div>
        {actions}
      </li>
    );
  }

  return (
    <li className="addons-card" data-disabled={enabled ? undefined : ""} data-pending={pending ? "" : undefined}>
      <div className="addons-card-head">
        <AddonIcon url={match?.project.icon_url} size={44} />
        <div className="min-w-0 flex-1">
          {name}
          <p className="addons-meta">
            <span className="mono truncate" title={entry.file_name}>
              {version ?? entry.file_name}
            </span>
            <span className="mono">{formatBytes(entry.size_bytes)}</span>
          </p>
        </div>
      </div>
      <p className="addons-desc">{match?.project.description || (lookedUp ? t("addons.unknownJarHint") : entry.file_name)}</p>
      {incompatible || (!match && lookedUp) || pending ? <div className="addons-tags">{tags}</div> : null}
      <div className="addons-card-foot">{actions}</div>
    </li>
  );
}

export function LoadingRows({ view }: { view: AddonView }) {
  if (view === "list") {
    return (
      <Panel>
        <div className="flex flex-col gap-2 p-4" aria-hidden="true">
          {[0, 1, 2, 3].map((index) => (
            <Skeleton key={index} className="h-10" />
          ))}
        </div>
      </Panel>
    );
  }
  return (
    <div className="addons-collection" data-view="cards" aria-hidden="true">
      {[0, 1, 2, 3, 4, 5].map((index) => (
        <Skeleton key={index} className="h-[168px] rounded-[var(--radius-lg)]" />
      ))}
    </div>
  );
}
