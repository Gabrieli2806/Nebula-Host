/**
 * Modrinth search for the launcher, laid out like the server add-ons browser:
 * search, sort, page size, range and pager, cards or list. With an instance
 * it lists only what fits it and marks what it already has; for modpacks
 * each result creates a new instance.
 */

import { useEffect, useRef, useState } from "react";
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { openExternal } from "@/lib/external";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, EmptyState, ErrorNotice, Icon, Panel, Segmented, Select, TextInput } from "@/ui";
import { AddonIcon } from "@/features/server/panels/addons/AddonIcon";
import { type AddonView, PAGE_SIZES, compactCount, relativeDate, useAddonView, usePageSize } from "@/features/server/panels/addons/util";
import type { ContentKind, InstalledContent, LauncherProfile, LauncherProject } from "./types";
import { loaderInfo } from "./loaderOptions";

const SORTS = ["relevance", "downloads", "follows", "updated", "newest"] as const;
type Sort = (typeof SORTS)[number];
type Kind = Exclude<ContentKind, "modpack">;

export function Browse({
  profile,
  initialKind = "mod",
  modpacks = false,
  onCreated,
}: {
  /** Install into this instance; omitted for modpacks. */
  profile?: LauncherProfile;
  initialKind?: Kind;
  /** Browse modpacks: each creates its own instance. */
  modpacks?: boolean;
  onCreated?: (profile: LauncherProfile) => void;
}) {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const [view, setView] = useAddonView();
  const [PAGE, setPage] = usePageSize();
  const [kind, setKind] = useState<Kind>(initialKind);
  const searchKind: ContentKind = modpacks ? "modpack" : kind;
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [sort, setSort] = useState<Sort>("relevance");
  const [offset, setOffset] = useState(0);
  const [installing, setInstalling] = useState<string | null>(null);
  const top = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced(query.trim());
      setOffset(0);
    }, 320);
    return () => window.clearTimeout(timer);
  }, [query]);

  const results = useQuery({
    queryKey: ["launcher", "search", searchKind, debounced, sort, offset, PAGE, profile?.id, profile?.game_version, profile?.loader],
    queryFn: () => launcherApi.search({ query: debounced, kind: searchKind, offset, limit: PAGE, sort, instance: profile?.id }),
    staleTime: 120_000,
    // Keep the current page on screen while the next one loads: no flash.
    placeholderData: keepPreviousData,
  });
  const contentKey = ["launcher", "content", profile?.id];
  const installed = useQuery({
    queryKey: contentKey,
    queryFn: () => launcherApi.contentList(profile!.id),
    enabled: !!profile && !modpacks,
  });
  const have = new Set((installed.data ?? []).map((item) => item.project_id).filter(Boolean));

  const install = useMutation({
    mutationFn: async (hit: LauncherProject) => {
      setInstalling(hit.project_id);
      if (modpacks) return { pack: await launcherApi.modpackInstall(hit.project_id) };
      const files = await launcherApi.contentInstall(profile!.id, hit.project_id, kind);
      return { files };
    },
    onSuccess: (result, hit) => {
      if (result.pack) {
        void client.invalidateQueries({ queryKey: ["launcher", "profiles"] });
        onCreated?.(result.pack);
        return;
      }
      // Mark it installed right away, then refresh quietly in the background.
      client.setQueryData<InstalledContent[]>(contentKey, (old) => [
        ...(old ?? []),
        { kind, file: result.files?.[0] ?? hit.slug, size: 0, enabled: true, project_id: hit.project_id },
      ]);
      void client.invalidateQueries({ queryKey: contentKey });
    },
    onSettled: () => setInstalling(null),
  });

  const goTo = (next: number) => {
    setOffset(next);
    top.current?.scrollIntoView({ block: "nearest" });
  };

  const data = results.data;
  const total = data?.total_hits ?? 0;
  const from = total === 0 ? 0 : offset + 1;
  const to = Math.min(offset + PAGE, total);
  const noLoader = !modpacks && kind === "mod" && profile?.loader === "vanilla";

  return (
    <div className="flex flex-col gap-3" ref={top}>
      {!modpacks ? (
        <div className="launcher-content-bar">
          <Segmented<Kind>
            size="sm"
            label={t("launcher.contentType")}
            value={kind}
            onChange={(next) => { setKind(next); setOffset(0); }}
            options={(["mod", "resourcepack", "shader"] as const).map((id) => ({ id, label: t(`launcher.${id}`) }))}
          />
          {profile ? <span className="launcher-muted">{t("launcher.fitsInstance", { version: profile.game_version, loader: loaderInfo(profile.loader).name })}</span> : null}
        </div>
      ) : (
        <p className="launcher-muted">{t("launcher.packCreatesInstance")}</p>
      )}

      <form className="addons-toolbar" role="search" onSubmit={(e) => { e.preventDefault(); setDebounced(query.trim()); setOffset(0); }}>
        <span className="addons-search addons-search-wide">
          <Icon name="search" />
          <TextInput type="search" value={query} aria-label={t("launcher.search")} placeholder={t("launcher.search")} onChange={(e) => setQuery(e.target.value)} />
        </span>
        <label className="addons-sort">
          <span className="label">{t("addons.sortLabel")}</span>
          <Select value={sort} onChange={(e) => { setSort(e.target.value as Sort); setOffset(0); }}>
            {SORTS.map((id) => <option key={id} value={id}>{t(`addons.sort_${id}`)}</option>)}
          </Select>
        </label>
        <label className="addons-sort">
          <span className="label">{t("addons.perPage")}</span>
          <Select value={String(PAGE)} onChange={(e) => { setPage(Number(e.target.value)); setOffset(0); }}>
            {PAGE_SIZES.map((size) => <option key={size} value={size}>{size}</option>)}
          </Select>
        </label>
        <Segmented<AddonView>
          size="sm"
          label={t("addons.viewLabel")}
          value={view}
          onChange={setView}
          options={[{ id: "cards", label: t("servers.densityCards") }, { id: "list", label: t("servers.densityList") }]}
        />
        {data && total > 0 ? (
          <p className="addons-summary addons-summary-end" aria-live="polite">
            {t("addons.resultsRange", { from: compactCount(from, language), to: compactCount(to, language), total: new Intl.NumberFormat(language).format(total) })}
          </p>
        ) : null}
      </form>

      {noLoader ? <p className="launcher-notice"><Icon name="info" size={14} />{t("launcher.vanillaNoMods")}</p> : null}
      {install.error ? <ErrorNotice error={toAppError(install.error)} compact /> : null}

      {results.error ? (
        <ErrorNotice error={toAppError(results.error)} onRetry={() => void results.refetch()} />
      ) : !data ? (
        <p role="status" className="launcher-muted">{t("launcher.loading")}</p>
      ) : data.hits.length === 0 ? (
        <Panel><EmptyState icon="search" title={debounced ? t("addons.noResults", { query: debounced }) : t("addons.noResultsAny")} /></Panel>
      ) : (
        <>
          <ul className="addons-collection" data-view={view} data-busy={results.isPlaceholderData ? "" : undefined} aria-busy={results.isFetching}>
            {data.hits.map((hit) => (
              <Hit
                key={hit.project_id}
                hit={hit}
                view={view}
                language={language}
                installed={have.has(hit.project_id)}
                installing={installing === hit.project_id}
                disabled={noLoader || installing !== null}
                modpack={modpacks}
                onInstall={() => install.mutate(hit)}
              />
            ))}
          </ul>
          {total > PAGE ? (
            <nav className="addons-pager" aria-label={t("addons.pages")}>
              <Button size="sm" icon="chevron-left" disabled={offset === 0 || results.isFetching} onClick={() => goTo(Math.max(0, offset - PAGE))}>{t("addons.previous")}</Button>
              <span className="addons-summary">{t("addons.pageOf", { page: Math.floor(offset / PAGE) + 1, pages: new Intl.NumberFormat(language).format(Math.ceil(total / PAGE)) })}</span>
              <Button size="sm" disabled={to >= total || results.isFetching} onClick={() => goTo(offset + PAGE)}>{t("addons.next")}<Icon name="chevron-right" /></Button>
            </nav>
          ) : null}
        </>
      )}
    </div>
  );
}

function Hit({ hit, view, language, installed, installing, disabled, modpack, onInstall }: {
  hit: LauncherProject;
  view: AddonView;
  language: string;
  installed: boolean;
  installing: boolean;
  disabled: boolean;
  modpack: boolean;
  onInstall: () => void;
}) {
  const { t } = useI18n();
  const updated = hit.date_modified ? relativeDate(hit.date_modified, language) : null;
  // The same width in every state, so the row never jumps.
  const action = installed ? (
    <span className="addons-installed"><Icon name="check" />{t("addons.installedState")}</span>
  ) : (
    <Button size="sm" icon={modpack ? "plus" : "download"} disabled={disabled} onClick={onInstall}>
      {installing ? t("addons.installing") : t(modpack ? "launcher.createFromPack" : "addons.install")}
    </Button>
  );
  const name = (
    <button type="button" className="addons-name addons-name-link" onClick={() => void openExternal(`https://modrinth.com/project/${encodeURIComponent(hit.project_id)}`)}>
      {hit.title}
    </button>
  );
  const stats = (
    <p className="addons-meta">
      <span><Icon name="arrow-down" size={12} /><span className="mono">{compactCount(hit.downloads, language)}</span></span>
      {updated ? <span>{t("addons.updatedAgo", { when: updated })}</span> : null}
    </p>
  );
  if (view === "list") {
    return (
      <li className="addons-row addons-row-hit">
        <AddonIcon url={hit.icon_url} size={40} />
        <div className="addons-row-body">
          <div className="addons-row-head">{name}<span className="addons-author">{t("addons.byAuthor", { author: hit.author })}</span></div>
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
        <div className="min-w-0 flex-1">{name}<p className="addons-author">{t("addons.byAuthor", { author: hit.author })}</p></div>
      </div>
      <p className="addons-desc">{hit.description}</p>
      {hit.categories?.length ? (
        <div className="addons-tags">
          {hit.categories.slice(0, 3).map((c) => <span key={c} className="tag">{c.charAt(0).toUpperCase() + c.slice(1).replace(/-/g, " ")}</span>)}
        </div>
      ) : null}
      <div className="addons-card-foot">{stats}{action}</div>
    </li>
  );
}
