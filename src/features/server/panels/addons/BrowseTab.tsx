/** Modrinth search, already narrowed to what runs on this server. */

import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { keys, useAddonBrowse, useAddonMutations, usePendingAddons } from "@/hooks/useBackend";
import { api } from "@/lib/ipc";
import type { AddonContext, AddonHit, AddonSort, InstanceView } from "@/types/api";
import { Button, EmptyState, ErrorNotice, Icon, Panel, Select, TextInput } from "@/ui";

import { LoadingRows } from "./InstalledTab";
import type { ProjectTarget } from "./ProjectDialog";
import { AddonIcon } from "./AddonIcon";
import {
  type AddonView,
  PAGE_SIZES,
  compactCount,
  pendingProjects,
  usePageSize,
  pickBest,
  relativeDate,
  useAddonCopy,
} from "./util";

const SORTS: AddonSort[] = ["relevance", "downloads", "follows", "updated", "newest"];

export function BrowseTab({
  instance,
  context,
  view,
  onOpenProject,
}: {
  instance: InstanceView;
  context: AddonContext;
  view: AddonView;
  onOpenProject: (target: ProjectTarget) => void;
}) {
  const { t, language } = useAddonCopy();
  const client = useQueryClient();
  const { installVersion } = useAddonMutations(instance.id);
  const { data: pending } = usePendingAddons(instance.id);
  const queued = pendingProjects(pending);
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [sort, setSort] = useState<AddonSort>("relevance");
  const [offset, setOffset] = useState(0);
  const [PAGE, setPage] = usePageSize();
  const [installing, setInstalling] = useState<string | null>(null);
  const [installError, setInstallError] = useState<unknown>(null);
  const topRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced(query.trim());
      setOffset(0);
    }, 320);
    return () => window.clearTimeout(timer);
  }, [query]);

  const results = useAddonBrowse(instance.id, {
    query: debounced || undefined,
    sort,
    offset,
    limit: PAGE,
  });

  const goTo = (next: number) => {
    setOffset(next);
    topRef.current?.scrollIntoView({ block: "nearest" });
  };

  /**
   * One click: resolve the best compatible version and its dependencies. If a
   * required dependency is missing, or nothing fits, the project dialog takes
   * over so the choice is made with everything in view.
   */
  const quickInstall = async (hit: AddonHit) => {
    setInstalling(hit.project_id);
    setInstallError(null);
    const target: ProjectTarget = { projectId: hit.project_id, project: hit };
    try {
      const versions = await client.fetchQuery({
        queryKey: keys.addonVersions(instance.id, hit.project_id, false),
        queryFn: () => api.addons.versions(instance.id, hit.project_id, false),
        staleTime: 5 * 60_000,
      });
      const best = pickBest(versions, context);
      if (!best) {
        onOpenProject(target);
        return;
      }
      const deps = await client.fetchQuery({
        queryKey: keys.addonDependencies(instance.id, best.id),
        queryFn: () => api.addons.dependencies(instance.id, best.id),
        staleTime: 5 * 60_000,
      });
      if (deps.some((dep) => dep.dependency_type === "required" && !dep.installed)) {
        onOpenProject(target);
        return;
      }
      await installVersion.mutateAsync({ projectId: hit.project_id, versionId: best.id });
    } catch (error) {
      setInstallError(error);
    } finally {
      setInstalling(null);
    }
  };

  const kind = context.project_type === "plugin" ? "plugin" : "mod";
  const data = results.data;
  const total = data?.total_hits ?? 0;
  const from = total === 0 ? 0 : offset + 1;
  const to = Math.min(offset + PAGE, total);

  return (
    <div className="flex flex-col gap-3" ref={topRef}>
      <form
        className="addons-toolbar"
        role="search"
        onSubmit={(event) => {
          event.preventDefault();
          setDebounced(query.trim());
          setOffset(0);
        }}
      >
        <span className="addons-search addons-search-wide">
          <Icon name="search" />
          <TextInput
            type="search"
            value={query}
            aria-label={t(`addons.searchPlaceholder_${kind}`)}
            placeholder={t(`addons.searchPlaceholder_${kind}`)}
            onChange={(event) => setQuery(event.target.value)}
          />
        </span>
        <label className="addons-sort">
          <span className="label">{t("addons.sortLabel")}</span>
          <Select
            value={sort}
            onChange={(event) => {
              setSort(event.target.value as AddonSort);
              setOffset(0);
            }}
          >
            {SORTS.map((id) => (
              <option key={id} value={id}>
                {t(`addons.sort_${id}`)}
              </option>
            ))}
          </Select>
        </label>
        <label className="addons-sort">
          <span className="label">{t("addons.perPage")}</span>
          <Select
            value={String(PAGE)}
            onChange={(event) => {
              setPage(Number(event.target.value));
              setOffset(0);
            }}
          >
            {PAGE_SIZES.map((size) => (
              <option key={size} value={size}>
                {size}
              </option>
            ))}
          </Select>
        </label>
        {data && total > 0 ? (
          <p className="addons-summary addons-summary-end" aria-live="polite">
            {t("addons.resultsRange", {
              from: compactCount(from, language),
              to: compactCount(to, language),
              total: new Intl.NumberFormat(language).format(total),
            })}
          </p>
        ) : null}
      </form>

      {installError ? <ErrorNotice error={installError} compact /> : null}

      {results.error ? (
        <ErrorNotice error={results.error} onRetry={() => void results.refetch()} />
      ) : !data ? (
        <LoadingRows view={view} />
      ) : data.hits.length === 0 ? (
        <Panel>
          <EmptyState
            icon="search"
            title={debounced ? t("addons.noResults", { query: debounced }) : t("addons.noResultsAny")}
            hint={t(`addons.noResultsHint_${kind}`, { version: context.game_version })}
          />
        </Panel>
      ) : (
        <>
          <ul
            className="addons-collection"
            data-view={view}
            data-busy={results.isPlaceholderData ? "" : undefined}
            aria-busy={results.isFetching}
            aria-label={t("addons.results")}
          >
            {data.hits.map((hit) => (
              <HitItem
                key={hit.project_id}
                hit={hit}
                view={view}
                queued={queued.has(hit.project_id)}
                installing={installing === hit.project_id}
                busy={installing !== null || installVersion.isPending}
                onInstall={() => void quickInstall(hit)}
                onOpen={() => onOpenProject({ projectId: hit.project_id, project: hit })}
              />
            ))}
          </ul>

          {total > PAGE ? (
            <nav className="addons-pager" aria-label={t("addons.pages")}>
              <Button
                size="sm"
                icon="chevron-left"
                disabled={offset === 0 || results.isFetching}
                onClick={() => goTo(Math.max(0, offset - PAGE))}
              >
                {t("addons.previous")}
              </Button>
              <span className="addons-summary">
                {t("addons.pageOf", {
                  page: Math.floor(offset / PAGE) + 1,
                  pages: new Intl.NumberFormat(language).format(Math.ceil(total / PAGE)),
                })}
              </span>
              <Button
                size="sm"
                disabled={to >= total || results.isFetching}
                onClick={() => goTo(offset + PAGE)}
              >
                {t("addons.next")}
                <Icon name="chevron-right" />
              </Button>
            </nav>
          ) : null}
        </>
      )}
    </div>
  );
}

function HitItem({
  hit,
  view,
  queued,
  installing,
  busy,
  onInstall,
  onOpen,
}: {
  hit: AddonHit;
  view: AddonView;
  /** Downloaded and waiting for the server to stop. */
  queued: boolean;
  installing: boolean;
  busy: boolean;
  onInstall: () => void;
  onOpen: () => void;
}) {
  const { t, language } = useAddonCopy();
  const updated = relativeDate(hit.date_modified, language);

  const action = queued ? (
    <span className="addons-installed addons-queued" title={t("addons.pendingQueued")}>
      <Icon name="clock" />
      {t("addons.pendingBadge")}
    </span>
  ) : hit.installed ? (
    <span className="addons-installed">
      <Icon name="check" />
      {t("addons.installedState")}
    </span>
  ) : (
    <Button
      size="sm"
      icon="plus"
      disabled={busy}
      onClick={onInstall}
      aria-label={installing ? undefined : t("addons.installName", { name: hit.title })}
    >
      {installing ? t("addons.installing") : t("addons.install")}
    </Button>
  );

  const name = (
    <button type="button" className="addons-name addons-name-link" onClick={onOpen}>
      {hit.title}
    </button>
  );

  const stats = (
    <p className="addons-meta">
      <span title={t("addons.downloads", { count: new Intl.NumberFormat(language).format(hit.downloads) })}>
        <Icon name="arrow-down" size={12} />
        <span className="mono">{compactCount(hit.downloads, language)}</span>
      </span>
      {updated ? <span>{t("addons.updatedAgo", { when: updated })}</span> : null}
    </p>
  );

  const categories = hit.categories.slice(0, 3).map((category) => (
    <span key={category} className="tag">
      {category.charAt(0).toUpperCase() + category.slice(1).replace(/-/g, " ")}
    </span>
  ));

  if (view === "list") {
    return (
      <li className="addons-row addons-row-hit">
        <AddonIcon url={hit.icon_url} size={40} />
        <div className="addons-row-body">
          <div className="addons-row-head">
            {name}
            <span className="addons-author">{t("addons.byAuthor", { author: hit.author })}</span>
          </div>
          <p className="addons-desc addons-desc-line">{hit.description}</p>
          {stats}
        </div>
        <div className="addons-actions">{action}</div>
      </li>
    );
  }

  return (
    <li className="addons-card addons-card-hit">
      <div className="addons-card-head">
        <AddonIcon url={hit.icon_url} size={44} />
        <div className="min-w-0 flex-1">
          {name}
          <p className="addons-author">{t("addons.byAuthor", { author: hit.author })}</p>
        </div>
      </div>
      <p className="addons-desc">{hit.description}</p>
      {categories.length > 0 ? <div className="addons-tags">{categories}</div> : null}
      <div className="addons-card-foot">
        {stats}
        {action}
      </div>
    </li>
  );
}
