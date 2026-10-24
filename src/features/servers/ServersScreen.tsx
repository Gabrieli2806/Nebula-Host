/**
 * The server list.
 *
 * A plain "Servers" heading (the machine's name is in the title bar), then one summary card with the
 * machine's numbers (running, players, memory given out with the memory
 * budget bar), then the servers. Two densities, because "an old PC as a
 * server" means ten to twenty servers as often as it means one.
 */

import { useEffect, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";

import { CreateServerDialog } from "./CreateServerDialog";
import { ServerCard } from "./ServerCard";
import { ServerRow } from "./ServerRow";
import { useServerMenu } from "./useServerMenu";
import {
  useAppSettings,
  useDistribution,
  useInstances,
  useNodeHealth,
} from "@/hooks/useBackend";
import { useRemoteLink } from "@/hooks/useRemoteLink";
import { useI18n } from "@/i18n";
import { formatMegabytes } from "@/lib/format";
import type { InstanceStatus, InstanceView } from "@/types/api";
import { PairDialog } from "@/app/NodeSwitcher";
import { IS_MOBILE } from "@/lib/web";
import { isRemote } from "@/lib/connection";
import { useManualOrder } from "@/lib/manualOrder";
import { Button, ErrorNotice, Icon, Mark, Skeleton, identityProps } from "@/ui";

type Density = "cards" | "list";
type Filter = "all" | "running" | "stopped";
type Sort = "name" | "status" | "created" | "memory" | "custom";

const FILTERS: Filter[] = ["all", "running", "stopped"];
const SORTS: Sort[] = ["name", "status", "created", "memory", "custom"];
const FILTER_KEY = "nebula.serversFilter";
const SORT_KEY = "nebula.serversSort";
const ORDER_KEY = "nebula.serversOrder";
/** The search field earns its place once the list no longer fits at a glance. */
const SEARCH_FROM = 5;

/** Running first, then the ones that need attention, then the idle ones. */
const STATUS_ORDER: Record<InstanceStatus, number> = {
  online: 0,
  starting: 1,
  stopping: 2,
  crashed: 3,
  sleeping: 4,
  offline: 5,
};

function isRunning(instance: InstanceView): boolean {
  return instance.status === "online" || instance.status === "starting" || instance.status === "stopping";
}

/** A per-viewer preference; storage may be unavailable, so reads never throw. */
function readStored<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const value = localStorage.getItem(key);
    return value && (allowed as readonly string[]).includes(value) ? (value as T) : fallback;
  } catch {
    return fallback;
  }
}

function writeStored(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // A private window or blocked storage: the choice lasts for this visit.
  }
}

function useStoredChoice<T extends string>(key: string, allowed: readonly T[], fallback: T) {
  const [value, setValue] = useState<T>(() => readStored(key, allowed, fallback));
  const set = (next: T) => {
    setValue(next);
    writeStored(key, next);
  };
  return [value, set] as const;
}

type Arrange = (items: InstanceView[], idOf: (item: InstanceView) => string) => InstanceView[];

function applyView(instances: InstanceView[], query: string, filter: Filter, sort: Sort, arrange: Arrange): InstanceView[] {
  const needle = query.trim().toLowerCase();
  const visible = instances.filter((instance) => {
    if (filter === "running" && !isRunning(instance)) return false;
    if (filter === "stopped" && isRunning(instance)) return false;
    if (!needle) return true;
    return [instance.name, instance.build.distribution, instance.build.version, instance.address ?? ""]
      .join(" ")
      .toLowerCase()
      .includes(needle);
  });
  const byName = (a: InstanceView, b: InstanceView) =>
    a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true });
  if (sort === "custom") return arrange(visible, (instance) => instance.id);
  return [...visible].sort((a, b) => {
    if (sort === "status") return STATUS_ORDER[a.status] - STATUS_ORDER[b.status] || byName(a, b);
    if (sort === "created") return b.created_at.localeCompare(a.created_at) || byName(a, b);
    if (sort === "memory") return b.resources.memory_mb - a.resources.memory_mb || byName(a, b);
    return byName(a, b);
  });
}

export function ServersScreen() {
  const { t } = useI18n();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const { data: instances, isLoading, error, refetch } = useInstances();
  // A managed machine that stopped answering: say so instead of loading for
  // minutes or summing up zeros that look real.
  const { remote, reachable } = useRemoteLink();
  const unreachable = remote !== null && !reachable;
  const noData = unreachable && !instances;
  const { data: settings } = useAppSettings();
  const [creating, setCreating] = useState(false);
  // The saved preference is the default; a click here overrides it for this
  // visit without writing back, so trying the other density is free.
  const [override, setOverride] = useState<Density | null>(null);
  const density: Density = override ?? (settings?.density === "list" ? "list" : "cards");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useStoredChoice<Filter>(FILTER_KEY, FILTERS, "all");
  const [sort, setSort] = useStoredChoice<Sort>(SORT_KEY, SORTS, "name");
  const total = instances?.length ?? 0;
  // The toolbar only shows from two servers up; a filter saved back then
  // must not hide the last server with no control on screen to explain it.
  const effectiveFilter: Filter = total > 1 ? filter : "all";
  // Dragging a card arranges the servers by hand and switches to that order.
  const manual = useManualOrder(ORDER_KEY);
  const visible = applyView(instances ?? [], query, effectiveFilter, sort, manual.arrange);
  // The whole list as shown now, filters aside, so hidden servers keep their place.
  const arrangedIds = applyView(instances ?? [], "", "all", sort, manual.arrange).map((instance) => instance.id);
  const drag = (id: string) => manual.dragProps(arrangedIds, id, () => setSort("custom"));
  const moveTo = (id: string, edge: "start" | "end") => {
    manual.moveTo(arrangedIds, id, edge);
    setSort("custom");
  };
  const narrowed = Boolean(query.trim()) || effectiveFilter !== "all";

  // `?create=1` (first run's "Create my first server") opens the dialog.
  // Adjusted during render, not in an effect, so it opens on the first paint;
  // the effect below only tidies the URL so a reload does not reopen it.
  const wantsCreate = searchParams.get("create") === "1";
  if (wantsCreate && !creating) setCreating(true);
  useEffect(() => {
    if (!wantsCreate) return;
    const next = new URLSearchParams(searchParams);
    next.delete("create");
    setSearchParams(next, { replace: true });
  }, [wantsCreate, searchParams, setSearchParams]);

  const open = (instance: InstanceView) => navigate(`/servers/${instance.id}/overview`);
  const serverMenu = useServerMenu(instances ?? [], () => setCreating(true), moveTo);

  return (
    <div
      className="servers-screen flex max-w-[var(--content-max)] flex-col gap-6 p-[var(--gutter)]"
      onContextMenu={serverMenu.onContextMenu}
    >
      {serverMenu.menu}
      <Heading
        instances={instances}
        onCreate={() => setCreating(true)}
        canCreate={!unreachable}
        density={density}
        onDensity={setOverride}
      />

      {/* With nothing hosted, the empty state carries the one action and the
          summary would only read zeros. */}
      {instances?.length === 0 || noData ? null : <MachineSummary instances={instances ?? []} />}

      {error && !noData ? <ErrorNotice error={error} onRetry={() => void refetch()} /> : null}

      {total > 1 ? (
        <Toolbar
          instances={instances ?? []}
          query={query}
          onQuery={setQuery}
          searchable={total >= SEARCH_FROM}
          filter={filter}
          onFilter={setFilter}
          sort={sort}
          onSort={setSort}
        />
      ) : null}

      {noData && remote ? <Unreachable name={remote.name} /> : null}

      {isLoading && !noData ? <LoadingServers density={density} /> : null}

      {instances && instances.length === 0 ? <NoServers onCreate={() => setCreating(true)} /> : null}

      {total > 0 && visible.length === 0 ? (
        <NoMatch
          onClear={() => {
            setQuery("");
            setFilter("all");
          }}
        />
      ) : null}

      {visible.length > 0 ? (
        density === "cards" ? (
          <div className="servers-grid">
            {visible.map((instance) => (
              <ServerCard key={instance.id} instance={instance} onOpen={() => open(instance)} drag={drag(instance.id)} />
            ))}
          </div>
        ) : (
          <div className="panel-flush servers-table-card">
            <table className="servers-table">
              <TableHead />
              <tbody>
                {visible.map((instance) => (
                  <ServerRow key={instance.id} instance={instance} onOpen={() => open(instance)} drag={drag(instance.id)} />
                ))}
              </tbody>
            </table>
          </div>
        )
      ) : null}

      {narrowed && visible.length > 0 ? (
        <p className="servers-count" aria-live="polite">
          {t("servers.showing", { shown: visible.length, total })}
        </p>
      ) : null}

      <CreateServerDialog
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={(instanceId) => {
          setCreating(false);
          navigate(`/servers/${instanceId}/overview`);
        }}
      />
    </div>
  );
}

function TableHead() {
  const { t } = useI18n();
  return (
    <thead>
      <tr>
        <th scope="col" className="label">
          {t("servers.title")}
        </th>
        <th scope="col" className="label" data-col="status">
          {t("servers.status")}
        </th>
        <th scope="col" className="label">
          {t("servers.version")}
        </th>
        <th scope="col" className="label" data-numeric="">
          {t("servers.players")}
        </th>
        <th scope="col" className="label" data-numeric="" data-col="cpu">
          {t("node.cpu")}
        </th>
        <th scope="col" className="label" data-numeric="">
          {t("servers.memory")}
        </th>
        <th scope="col" className="label" data-numeric="" data-col="uptime">
          {t("servers.uptime")}
        </th>
        <th aria-hidden="true" />
      </tr>
    </thead>
  );
}

/**
 * Search, a status filter and the sort order, on one line under the heading.
 * The filter and the sort are remembered on this device; the search is not.
 * "/" focuses the search from anywhere on the screen, as in most lists.
 */
function Toolbar({
  instances,
  query,
  onQuery,
  searchable,
  filter,
  onFilter,
  sort,
  onSort,
}: {
  instances: InstanceView[];
  query: string;
  onQuery: (query: string) => void;
  searchable: boolean;
  filter: Filter;
  onFilter: (filter: Filter) => void;
  sort: Sort;
  onSort: (sort: Sort) => void;
}) {
  const { t } = useI18n();
  const search = useRef<HTMLInputElement>(null);
  const running = instances.filter(isRunning).length;
  const counts: Record<Filter, number> = {
    all: instances.length,
    running,
    stopped: instances.length - running,
  };

  useEffect(() => {
    if (!searchable) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "/" || event.ctrlKey || event.metaKey || event.altKey) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, select, [contenteditable='true'], [role='dialog']")) return;
      event.preventDefault();
      search.current?.focus();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [searchable]);

  return (
    <div className="servers-toolbar">
      {searchable ? (
        <label className="servers-search">
          <span className="servers-search-icon" aria-hidden="true">
            <Icon name="search" size={14} />
          </span>
          <input
            ref={search}
            type="search"
            className="field"
            value={query}
            placeholder={t("servers.searchPlaceholder")}
            aria-label={t("servers.search")}
            onChange={(event) => onQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape" && query) {
                event.preventDefault();
                onQuery("");
              }
            }}
          />
          {query ? null : (
            <kbd className="servers-search-key" aria-hidden="true">
              /
            </kbd>
          )}
        </label>
      ) : null}

      <div className="segmented" data-size="sm" role="group" aria-label={t("servers.filter")}>
        {FILTERS.map((option) => (
          <button
            key={option}
            type="button"
            className="segment"
            aria-pressed={filter === option}
            onClick={() => onFilter(option)}
          >
            {t(`servers.filter_${option}`)}
            <span className="servers-filter-count readout">{counts[option]}</span>
          </button>
        ))}
      </div>

      <label className="servers-sort">
        <span className="servers-sort-label">{t("servers.sort")}</span>
        <select className="field" value={sort} onChange={(event) => onSort(event.target.value as Sort)}>
          {SORTS.map((option) => (
            <option key={option} value={option}>
              {t(`servers.sort_${option}`)}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}

/** Placeholders shaped like what is coming, in the density that is coming. */
function LoadingServers({ density }: { density: Density }) {
  if (density === "list") {
    return (
      <div className="panel-flush servers-skeleton-list" aria-busy="true">
        {[0, 1, 2, 3].map((index) => (
          <div key={index} className="servers-skeleton-row">
            <Skeleton className="h-7 w-7 flex-none rounded-[var(--radius)]" />
            <Skeleton className="h-3.5 w-[22%] rounded-[var(--radius-sm)]" />
            <Skeleton className="ml-auto h-3.5 w-[12%] rounded-[var(--radius-sm)]" />
            <Skeleton className="h-3.5 w-[10%] rounded-[var(--radius-sm)]" />
          </div>
        ))}
      </div>
    );
  }
  return (
    <div className="servers-grid" aria-busy="true">
      {[0, 1, 2].map((index) => (
        <div key={index} className="panel server-card servers-skeleton-card">
          <div className="flex items-start gap-3.5">
            <Skeleton className="h-11 w-11 flex-none rounded-[var(--radius)]" />
            <div className="flex flex-1 flex-col gap-2.5 pt-1">
              <Skeleton className="h-4 w-[60%] rounded-[var(--radius-sm)]" />
              <Skeleton className="h-3 w-[40%] rounded-[var(--radius-sm)]" />
            </div>
          </div>
          <Skeleton className="h-9 rounded-[var(--radius)]" />
          <div className="servers-skeleton-foot">
            <Skeleton className="h-3 w-[30%] rounded-[var(--radius-sm)]" />
            <Skeleton className="ml-auto h-7 w-[72px] rounded-[var(--radius)]" />
          </div>
        </div>
      ))}
    </div>
  );
}

/** The search or the filter hid every server. */
function NoMatch({ onClear }: { onClear: () => void }) {
  const { t } = useI18n();
  return (
    <div className="panel servers-empty">
      <div className="empty-state">
        <span className="empty-state-icon">
          <Icon name="search" size={20} />
        </span>
        <p className="empty-state-title">{t("servers.noMatch")}</p>
        <p className="empty-state-hint">{t("servers.noMatchHint")}</p>
        <div className="mt-1">
          <Button onClick={onClear}>{t("servers.clearFilters")}</Button>
        </div>
      </div>
    </div>
  );
}

/**
 * The page's name as a plain heading. The view switch and the one primary
 * action sit on the same line, right-aligned.
 */
function Heading({
  instances,
  onCreate,
  canCreate,
  density,
  onDensity,
}: {
  instances: InstanceView[] | undefined;
  onCreate: () => void;
  canCreate: boolean;
  density: Density;
  onDensity: (density: Density) => void;
}) {
  const { t } = useI18n();

  // The machine's name already sits in the title bar; the page says what it is.
  return (
    <header className="flex flex-wrap items-center justify-between gap-x-8 gap-y-4">
      <h1 className="display servers-node min-w-0">{t("nav.servers")}</h1>

      {instances?.length === 0 ? null : (
        <div className="flex flex-none items-center gap-2">
          <div className="segmented" role="group" aria-label={t("servers.density")}>
            <DensityKey
              active={density === "cards"}
              onClick={() => onDensity("cards")}
              label={t("servers.densityCards")}
            />
            <DensityKey
              active={density === "list"}
              onClick={() => onDensity("list")}
              label={t("servers.densityList")}
            />
          </div>
          <Button variant="primary" icon="plus" onClick={onCreate} disabled={!canCreate}>
            {t("actions.create")}
          </Button>
        </div>
      )}
    </header>
  );
}

function DensityKey({
  active,
  onClick,
  label,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
}) {
  return (
    <button type="button" className="segment" aria-pressed={active} onClick={onClick}>
      {label}
    </button>
  );
}

/**
 * The machine's numbers in one card, split by hairlines: value over label.
 * The memory cell carries the budget bar: one track of the machine's RAM,
 * one segment per server holding memory, in its identity colour.
 */
function MachineSummary({ instances }: { instances: InstanceView[] }) {
  const { t } = useI18n();
  const { data: health } = useNodeHealth();

  // Same rule as the "Running" filter next to it, so the two never disagree.
  const running = instances.filter(isRunning).length;
  const players = instances.reduce((total, instance) => total + instance.players_online, 0);
  // "Given out" = memory promised to every server whose process is alive.
  // Sleeping and crashed servers have no process (see model.rs
  // expects_process), so they hold nothing and do not count against
  // "can I start another?".
  const holding = instances.filter(
    (instance) =>
      instance.status === "online" || instance.status === "starting" || instance.status === "stopping",
  );
  const committed = holding.reduce((total, instance) => total + instance.resources.memory_mb, 0);
  const total = health?.memory_total_mb ?? 0;

  return (
    <section className="panel machine-summary">
      <div className="summary-cell">
        <p className="readout summary-value">
          {running}
          <span className="summary-of">/{instances.length}</span>
        </p>
        <p className="label summary-label">{t("servers.running")}</p>
      </div>

      <span className="summary-divider" aria-hidden="true" />

      <div className="summary-cell">
        <p className="readout summary-value">{players}</p>
        <p className="label summary-label">{t("servers.playersNow")}</p>
      </div>

      <span className="summary-divider" aria-hidden="true" />

      <div className="summary-cell summary-cell-memory">
        <div className="flex min-w-0 flex-col gap-1.5">
          <p className="readout summary-value">
            <Amount mb={committed} />
            {total ? (
              <span className="summary-of">
                {" / "}
                {formatMegabytes(total)}
              </span>
            ) : null}
          </p>
          <p className="label summary-label">{t("servers.committed")}</p>
        </div>
        <MemoryBudget holding={holding} committed={committed} total={total} />
      </div>
    </section>
  );
}

/** "16.0 GB" with the unit stepped down. */
function Amount({ mb }: { mb: number }) {
  const [value, unit] = formatMegabytes(mb).split(" ");
  return (
    <>
      {value}
      {unit ? <span className="summary-unit">{unit}</span> : null}
    </>
  );
}

function MemoryBudget({
  holding,
  committed,
  total,
}: {
  holding: InstanceView[];
  committed: number;
  total: number;
}) {
  const { t } = useI18n();
  // If more is promised than the machine has, the track grows to fit the
  // promise, so the overdraw is visible rather than clipped.
  const scale = Math.max(total, committed, 1);
  const free = Math.max(0, total - committed);

  const description = [
    `${t("servers.memoryBudget")}: ${formatMegabytes(committed)}${
      total ? ` / ${formatMegabytes(total)}` : ""
    }`,
    ...holding.map(
      (instance) => `${instance.name} ${formatMegabytes(instance.resources.memory_mb)}`,
    ),
    total ? t("servers.memoryFree", { free: formatMegabytes(free) }) : null,
  ]
    .filter(Boolean)
    .join(", ");

  return (
    <div className="memory-budget">
      <div className="memory-budget-track" role="img" aria-label={description}>
        {holding.map((instance) => (
          <BudgetSegment
            key={instance.id}
            instance={instance}
            share={(instance.resources.memory_mb / scale) * 100}
          />
        ))}
      </div>
      {total ? (
        <p className="memory-budget-free readout" aria-hidden="true">
          {t("servers.memoryFree", { free: formatMegabytes(free) })}
        </p>
      ) : null}
    </div>
  );
}

/**
 * One server's share of the machine: the whole segment is what it was given,
 * the brighter fill inside is what it is actually using right now.
 */
function BudgetSegment({ instance, share }: { instance: InstanceView; share: number }) {
  const distribution = useDistribution(instance);
  const identity = identityProps(distribution?.accent);
  const used =
    instance.resources.memory_mb > 0
      ? Math.min(100, (instance.memory_used_mb / instance.resources.memory_mb) * 100)
      : 0;

  return (
    <span
      className="memory-budget-segment"
      {...identity}
      style={{ ...identity.style, flexBasis: `${share}%` }}
      title={`${instance.name}: ${formatMegabytes(instance.resources.memory_mb)}`}
    >
      <span className="memory-budget-used" style={{ width: `${used}%` }} />
    </span>
  );
}

/** Nothing hosted yet: the mark, one plain line and the one action. */
/** The managed machine is not answering and nothing was loaded from it. */
function Unreachable({ name }: { name: string }) {
  const { t } = useI18n();
  return (
    <div className="panel servers-empty">
      <div className="empty-state">
        <span className="empty-state-icon">
          <Icon name="servers" size={20} />
        </span>
        <p className="empty-state-title">{t("servers.unreachableTitle", { name })}</p>
        <p className="empty-state-hint">{t("servers.unreachableHint", { name })}</p>
      </div>
    </div>
  );
}

function NoServers({ onCreate }: { onCreate: () => void }) {
  const { t } = useI18n();
  const [pairing, setPairing] = useState(false);
  // On the phone, managing a computer comes first; hosting here second.
  if (IS_MOBILE && !isRemote()) {
    return (
      <div className="panel servers-empty">
        <div className="empty-state">
          <span className="empty-state-icon">
            <Mark size={28} />
          </span>
          <p className="empty-state-title">{t("phoneWelcome.emptyTitle")}</p>
          <p className="empty-state-hint">{t("phoneWelcome.emptyHint")}</p>
          <div className="mt-1 flex flex-col items-stretch gap-2">
            <Button variant="primary" icon="link" onClick={() => setPairing(true)}>
              {t("phoneWelcome.connect")}
            </Button>
            <Button icon="plus" onClick={onCreate}>
              {t("phoneWelcome.host")}
            </Button>
          </div>
        </div>
        {pairing ? <PairDialog open onClose={() => setPairing(false)} /> : null}
      </div>
    );
  }
  return (
    <div className="panel servers-empty">
      <div className="empty-state">
        <span className="empty-state-icon">
          <Mark size={28} />
        </span>
        <p className="empty-state-title">{t("servers.noFires")}</p>
        <p className="empty-state-hint">{t("servers.noFiresHint")}</p>
        <div className="mt-1">
          <Button variant="primary" icon="plus" onClick={onCreate}>
            {t("actions.create")}
          </Button>
        </div>
      </div>
    </div>
  );
}
