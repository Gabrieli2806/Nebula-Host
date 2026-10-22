/**
 * What an instance has installed (mods, resource packs, shaders) with the
 * switches to turn each off or remove it, and the way into Modrinth to add
 * more. Shown in the instance's panel in the Library.
 */

import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, ErrorNotice, Icon, Segmented, Switch, type IconName } from "@/ui";
import { type AddonView, useAddonView } from "@/features/server/panels/addons/util";
import type { ContentKind, InstalledContent, LauncherProfile } from "./types";

const FALLBACK: Record<Exclude<ContentKind, "modpack">, IconName> = { mod: "puzzle", resourcepack: "image", shader: "sun" };
const ACCEPT: Record<Exclude<ContentKind, "modpack">, string> = { mod: ".jar", resourcepack: ".zip", shader: ".zip" };

/** A file's bytes as base64, for the backend (no data-URL prefix). */
function toBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",")[1] ?? "");
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

/** The picture the file carries (a mod's logo, a pack's pack.png), loaded
 *  once and kept; the kind's own glyph when it has none. */
function ContentIcon({ profile, item, size, fallback }: { profile: LauncherProfile; item: InstalledContent; size: number; fallback?: string | null }) {
  const icon = useQuery({
    queryKey: ["launcher", "content-icon", profile.id, item.kind, item.file],
    queryFn: () => launcherApi.contentIcon(profile.id, item.kind, item.file),
    staleTime: Infinity,
    retry: false,
  });
  const src = icon.data ?? (fallback?.startsWith("https://") ? fallback : null);
  return src
    ? <img src={src} alt="" width={size} height={size} loading="lazy" draggable={false} className="addon-icon" />
    : <span className="addon-icon addon-icon-blank" style={{ width: size, height: size }} aria-hidden="true"><Icon name={FALLBACK[item.kind]} size={size > 36 ? 20 : 16} /></span>;
}

const KINDS: { id: Exclude<ContentKind, "modpack">; label: string }[] = [
  { id: "mod", label: "launcher.mod" },
  { id: "resourcepack", label: "launcher.resourcepack" },
  { id: "shader", label: "launcher.shader" },
];

export function InstanceContent({ profile, onBrowse }: { profile: LauncherProfile; onBrowse: (kind: ContentKind) => void }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const [kind, setKind] = useState<Exclude<ContentKind, "modpack">>("mod");
  const [view, setView] = useAddonView();
  const key = ["launcher", "content", profile.id];
  const list = useQuery({ queryKey: key, queryFn: () => launcherApi.contentList(profile.id) });
  const refresh = () => client.invalidateQueries({ queryKey: key });
  const toggle = useMutation({
    mutationFn: (item: InstalledContent) => launcherApi.contentSetEnabled(profile.id, item.kind, item.file, !item.enabled),
    onSuccess: refresh,
  });
  const remove = useMutation({
    mutationFn: (item: InstalledContent) => launcherApi.contentRemove(profile.id, item.kind, item.file),
    onSuccess: refresh,
  });
  const picker = useRef<HTMLInputElement>(null);
  const upload = useMutation({
    mutationFn: async (file: File) => launcherApi.contentImport(profile.id, kind, file.name, await toBase64(file)),
    onSuccess: refresh,
  });
  // Modrinth's view of each file for this version and loader: updates and
  // compatibility. Keyed by version, so changing it re-checks by itself.
  const checkKey = ["launcher", "content-check", profile.id, profile.game_version, profile.loader, profile.loader_version];
  const check = useQuery({ queryKey: checkKey, queryFn: () => launcherApi.contentCheck(profile.id), staleTime: 10 * 60_000, retry: false });
  const statusOf = (item: InstalledContent) => check.data?.find(s => s.kind === item.kind && s.file === item.file);
  const [updating, setUpdating] = useState<string | null>(null);
  const update = useMutation({
    mutationFn: async (items: InstalledContent[]) => {
      for (const item of items) {
        const next = statusOf(item)?.update;
        if (!next) continue;
        setUpdating(item.file);
        await launcherApi.contentUpdate(profile.id, item.kind, item.file, next.version_id);
      }
    },
    onSettled: async () => {
      setUpdating(null);
      await refresh();
      await client.invalidateQueries({ queryKey: checkKey });
    },
  });
  const items = (list.data ?? []).filter(item => item.kind === kind);
  const outdated = items.filter(item => statusOf(item)?.update);
  const incompatible = items.filter(item => statusOf(item)?.compatible === false && !statusOf(item)?.update);
  const counts = (id: string) => (list.data ?? []).filter(item => item.kind === id).length;
  const noLoader = kind === "mod" && profile.loader === "vanilla";
  const failure = list.error || toggle.error || remove.error || upload.error || update.error;

  return <section className="launcher-content" aria-label={t("launcher.content")}>
    <div className="launcher-content-head">
      <h3 className="label">{t("launcher.content")}</h3>
      <div className="launcher-content-add">
        <input ref={picker} type="file" hidden accept={ACCEPT[kind]} onChange={e => { const file = e.target.files?.[0]; e.target.value = ""; if (file) upload.mutate(file); }} />
        <Button size="sm" icon="upload" disabled={noLoader || upload.isPending} title={noLoader ? t("launcher.vanillaNoMods") : undefined} onClick={() => picker.current?.click()}>{upload.isPending ? t("launcher.installing") : t(kind === "mod" ? "launcher.addJar" : "launcher.addZip")}</Button>
        <Button size="sm" variant="primary" icon="plus" disabled={noLoader} title={noLoader ? t("launcher.vanillaNoMods") : undefined} onClick={() => onBrowse(kind)}>{t("launcher.addFromModrinth")}</Button>
      </div>
    </div>
    <div className="launcher-content-bar">
      <Segmented<Exclude<ContentKind, "modpack">> size="sm" label={t("launcher.content")} value={kind} onChange={setKind} options={KINDS.map(item => ({ id: item.id, label: `${t(item.label)} ${counts(item.id)}` }))} />
      <Segmented<AddonView> size="sm" label={t("addons.viewLabel")} value={view} onChange={setView} options={[{ id: "cards", label: t("servers.densityCards") }, { id: "list", label: t("servers.densityList") }]} />
    </div>
    <div className="launcher-update-bar">
      <span className="launcher-muted" aria-live="polite">
        {check.isFetching ? t("launcher.checkingUpdates")
          : check.error ? t("launcher.checkFailed")
          : outdated.length ? t("launcher.updatesAvailable", { count: outdated.length })
          : check.data ? t("launcher.allUpToDate", { version: profile.game_version }) : ""}
        {incompatible.length && !check.isFetching ? ` · ${t("launcher.incompatibleCount", { count: incompatible.length, version: profile.game_version })}` : ""}
      </span>
      <div className="launcher-actions">
        <Button size="sm" variant="quiet" icon="refresh" disabled={check.isFetching} onClick={() => void check.refetch()}>{t("launcher.checkUpdates")}</Button>
        {outdated.length > 1 ? <Button size="sm" variant="primary" icon="refresh" disabled={update.isPending} onClick={() => update.mutate(outdated)}>{t("launcher.updateAll", { count: outdated.length })}</Button> : null}
      </div>
    </div>
    {noLoader ? <p className="launcher-muted">{t("launcher.vanillaNoMods")}</p> : null}
    {list.isPending ? <p role="status">{t("launcher.loading")}</p> : null}
    {failure ? <ErrorNotice error={toAppError(failure)} compact /> : null}
    {list.data && items.length === 0 && !noLoader ? <p className="launcher-muted">{t("launcher.noContent")}</p> : null}
    {items.length ? <ul className="addons-collection" data-view={view} aria-label={t("launcher.content")}>
      {items.map(item => {
        const status = statusOf(item);
        const name = status?.title ?? item.file.replace(/\.disabled$/, "").replace(/\.(jar|zip)$/i, "");
        const tags = <>
          {status && !status.compatible ? <span className="tag" data-tone="warn" title={t("launcher.incompatibleHint", { version: profile.game_version })}><Icon name="warning" size={11} /> {t("launcher.incompatibleTag", { version: profile.game_version })}</span> : null}
          {status?.update ? <span className="tag" data-tone="accent">{t("launcher.updateTag", { version: status.update.version_number })}</span> : null}
        </>;
        const actions = <div className="addons-actions">
          {status?.update ? <Button size="sm" icon="refresh" disabled={update.isPending} onClick={() => update.mutate([item])}>{updating === item.file ? t("addons.updating") : t("launcher.update")}</Button> : null}
          <Switch checked={item.enabled} label={t(item.enabled ? "launcher.disable" : "launcher.enable")} disabled={toggle.isPending} onChange={() => toggle.mutate(item)} />
          <Button size="sm" variant="quiet" iconOnly icon="trash" className="row-action" disabled={remove.isPending} onClick={() => remove.mutate(item)}>{t("launcher.removeContent")}</Button>
        </div>;
        const meta = status?.version ? <p className="addons-meta"><span className="mono">{status.version}</span></p> : null;
        return view === "list"
          ? <li key={item.file} className="addons-row" data-disabled={item.enabled ? undefined : ""}>
              <ContentIcon profile={profile} item={item} size={36} fallback={status?.icon_url} />
              <div className="addons-row-body"><div className="addons-row-head"><span className="addons-name">{name}</span>{tags}</div>{meta}</div>
              {actions}
            </li>
          : <li key={item.file} className="addons-card" data-disabled={item.enabled ? undefined : ""}>
              <div className="addons-card-head"><ContentIcon profile={profile} item={item} size={44} fallback={status?.icon_url} /><div className="min-w-0 flex-1"><span className="addons-name">{name}</span>{meta}</div></div>
              {status && (!status.compatible || status.update) ? <div className="addons-tags">{tags}</div> : null}
              <div className="addons-card-foot">{actions}</div>
            </li>;
      })}
    </ul> : null}
  </section>;
}

