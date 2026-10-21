/**
 * What an instance has installed (mods, resource packs, shaders) with the
 * switches to turn each off or remove it, and the way into Modrinth to add
 * more. Shown in the instance's panel in the Library.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { formatBytes } from "@/lib/format";
import { Button, ErrorNotice, Segmented } from "@/ui";
import type { ContentKind, InstalledContent, LauncherProfile } from "./types";

const KINDS: { id: Exclude<ContentKind, "modpack">; label: string }[] = [
  { id: "mod", label: "launcher.mod" },
  { id: "resourcepack", label: "launcher.resourcepack" },
  { id: "shader", label: "launcher.shader" },
];

export function InstanceContent({ profile, onBrowse }: { profile: LauncherProfile; onBrowse: (kind: ContentKind) => void }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const [kind, setKind] = useState<Exclude<ContentKind, "modpack">>("mod");
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
  const items = (list.data ?? []).filter(item => item.kind === kind);
  const counts = (id: string) => (list.data ?? []).filter(item => item.kind === id).length;
  const noLoader = kind === "mod" && profile.loader === "vanilla";
  const failure = list.error || toggle.error || remove.error;

  return <section className="launcher-content" aria-label={t("launcher.content")}>
    <div className="launcher-content-head">
      <h3 className="label">{t("launcher.content")}</h3>
      <Button size="sm" variant="primary" icon="plus" disabled={noLoader} title={noLoader ? t("launcher.vanillaNoMods") : undefined} onClick={() => onBrowse(kind)}>{t("launcher.addFromModrinth")}</Button>
    </div>
    <Segmented<Exclude<ContentKind, "modpack">> size="sm" label={t("launcher.content")} value={kind} onChange={setKind} options={KINDS.map(item => ({ id: item.id, label: `${t(item.label)} ${counts(item.id)}` }))} />
    {noLoader ? <p className="launcher-muted">{t("launcher.vanillaNoMods")}</p> : null}
    {list.isPending ? <p role="status">{t("launcher.loading")}</p> : null}
    {failure ? <ErrorNotice error={toAppError(failure)} compact /> : null}
    {list.data && items.length === 0 && !noLoader ? <p className="launcher-muted">{t("launcher.noContent")}</p> : null}
    <ul className="launcher-content-list">
      {items.map(item => <li key={item.file} data-disabled={item.enabled ? undefined : ""}>
        <span className="launcher-content-name" title={item.file}>{item.file.replace(/\.disabled$/, "")}</span>
        <span className="launcher-muted">{formatBytes(item.size)}</span>
        <Button size="sm" variant="quiet" disabled={toggle.isPending} onClick={() => toggle.mutate(item)}>{t(item.enabled ? "launcher.disable" : "launcher.enable")}</Button>
        <Button size="sm" variant="quiet" iconOnly icon="trash" disabled={remove.isPending} aria-label={t("launcher.removeContent")} onClick={() => remove.mutate(item)}>{t("launcher.removeContent")}</Button>
      </li>)}
    </ul>
  </section>;
}

