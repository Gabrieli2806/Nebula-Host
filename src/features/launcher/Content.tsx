/**
 * What an instance has installed (mods, resource packs, shaders) with the
 * switches to turn each off or remove it, and the way into Modrinth to add
 * more. Shown in the instance's panel in the Library.
 */

import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { formatBytes } from "@/lib/format";
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
function ContentIcon({ profile, item, size }: { profile: LauncherProfile; item: InstalledContent; size: number }) {
  const icon = useQuery({
    queryKey: ["launcher", "content-icon", profile.id, item.kind, item.file],
    queryFn: () => launcherApi.contentIcon(profile.id, item.kind, item.file),
    staleTime: Infinity,
    retry: false,
  });
  return icon.data
    ? <img src={icon.data} alt="" width={size} height={size} loading="lazy" draggable={false} className="addon-icon" />
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
  const items = (list.data ?? []).filter(item => item.kind === kind);
  const counts = (id: string) => (list.data ?? []).filter(item => item.kind === id).length;
  const noLoader = kind === "mod" && profile.loader === "vanilla";
  const failure = list.error || toggle.error || remove.error || upload.error;

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
    {noLoader ? <p className="launcher-muted">{t("launcher.vanillaNoMods")}</p> : null}
    {list.isPending ? <p role="status">{t("launcher.loading")}</p> : null}
    {failure ? <ErrorNotice error={toAppError(failure)} compact /> : null}
    {list.data && items.length === 0 && !noLoader ? <p className="launcher-muted">{t("launcher.noContent")}</p> : null}
    {items.length ? <ul className="addons-collection" data-view={view} aria-label={t("launcher.content")}>
      {items.map(item => {
        const name = item.file.replace(/\.disabled$/, "").replace(/\.(jar|zip)$/i, "");
        const actions = <div className="addons-actions">
          <Switch checked={item.enabled} label={t(item.enabled ? "launcher.disable" : "launcher.enable")} disabled={toggle.isPending} onChange={() => toggle.mutate(item)} />
          <Button size="sm" variant="quiet" iconOnly icon="trash" className="row-action" disabled={remove.isPending} onClick={() => remove.mutate(item)}>{t("launcher.removeContent")}</Button>
        </div>;
        const meta = <p className="addons-meta"><span className="mono truncate" title={item.file}>{item.file}</span><span className="mono">{formatBytes(item.size)}</span></p>;
        return view === "list"
          ? <li key={item.file} className="addons-row" data-disabled={item.enabled ? undefined : ""}>
              <ContentIcon profile={profile} item={item} size={36} />
              <div className="addons-row-body"><div className="addons-row-head"><span className="addons-name">{name}</span>{item.project_id ? <span className="tag">Modrinth</span> : null}</div>{meta}</div>
              {actions}
            </li>
          : <li key={item.file} className="addons-card" data-disabled={item.enabled ? undefined : ""}>
              <div className="addons-card-head"><ContentIcon profile={profile} item={item} size={44} /><div className="min-w-0 flex-1"><span className="addons-name">{name}</span>{meta}</div></div>
              <div className="addons-card-foot">{actions}</div>
            </li>;
      })}
    </ul> : null}
  </section>;
}

