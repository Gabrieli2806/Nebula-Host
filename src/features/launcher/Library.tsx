import { useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, Icon, LoaderIcon } from "@/ui";
import { CreateInstanceDialog } from "./CreateInstanceDialog";
import { LOADERS, loaderInfo } from "./loaderOptions";
import type { LauncherProfile, Loader } from "./types";

export function Library({ renderControls }: { renderControls: (profile: LauncherProfile) => ReactNode }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const profiles = useQuery({ queryKey: ["launcher", "profiles"], queryFn: launcherApi.profiles });
  const caps = useQuery({ queryKey: ["launcher", "capabilities"], queryFn: launcherApi.capabilities });
  const statuses = useQuery({ queryKey: ["launcher", "status"], queryFn: launcherApi.status, refetchInterval: 1000 });
  const [creating, setCreating] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [loader, setLoader] = useState<Loader | "all">("all");
  const [deleting, setDeleting] = useState<LauncherProfile | null>(null);
  const remove = useMutation({ mutationFn: launcherApi.remove, onSuccess: async () => {
    setDeleting(null); await client.invalidateQueries({ queryKey: ["launcher", "profiles"] });
  }});
  const visible = (profiles.data ?? []).filter(profile =>
    (loader === "all" || profile.loader === loader) && `${profile.name} ${profile.game_version}`.toLowerCase().includes(search.toLowerCase()));
  const selected = visible.find(profile => profile.id === selectedId) ?? visible[0];
  const failure = profiles.error || caps.error || statuses.error || remove.error;
  const isBusy = (id: string) => ["running", "installing", "stopping"].includes(statuses.data?.[id]?.phase ?? "");
  return <>
    <header className="launcher-library-header"><div><h2>{t("launcher.yourInstances")} <span className="launcher-instance-count">{profiles.data?.length ?? 0}</span></h2><p className="launcher-muted">{t("launcher.libraryIntro")}</p></div><Button variant="primary" icon="plus" onClick={() => setCreating(true)}>{t("launcher.newInstance")}</Button></header>
    {profiles.data?.length ? <div className="launcher-library-toolbar">
      <div className="launcher-instance-search"><Icon name="search" /><input type="search" aria-label={t("launcher.searchInstances")} placeholder={t("launcher.searchInstances")} value={search} onChange={e => setSearch(e.target.value)} /></div>
      <select aria-label={t("launcher.filterLoader")} value={loader} onChange={e => setLoader(e.target.value as Loader | "all")}><option value="all">{t("launcher.allLoaders")}</option>{LOADERS.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select>
    </div> : null}
    {failure ? <ErrorNotice error={toAppError(failure)} /> : null}
    {profiles.isPending ? <p role="status">{t("launcher.loading")}</p> : null}
    {profiles.isError ? <Button onClick={() => void profiles.refetch()}>{t("launcher.retry")}</Button> : null}
    {profiles.data?.length === 0 ? <section className="launcher-library-empty"><span className="launcher-empty-mark"><LoaderIcon glyph="cube" size={44} /></span><h2>{t("launcher.emptyTitle")}</h2><p>{t("launcher.emptyDescription")}</p><Button variant="primary" icon="plus" onClick={() => setCreating(true)}>{t("launcher.createFirstInstance")}</Button><span className="launcher-muted">Minecraft Java Edition</span></section> : null}
    {profiles.data?.length && visible.length === 0 ? <div className="launcher-library-empty"><Icon name="search" size={28} /><h2>{t("launcher.noMatchingInstances")}</h2><Button onClick={() => { setSearch(""); setLoader("all"); }}>{t("launcher.clearFilters")}</Button></div> : null}
    {visible.length ? <div className="launcher-library-layout"><section className="launcher-instance-grid" aria-label={t("launcher.yourInstances")}>
      {visible.map(profile => {
        const info = loaderInfo(profile.loader);
        const phase = statuses.data?.[profile.id]?.phase;
        const status = phase === "running" ? "launcher.runningShort" : phase === "installing" ? "launcher.installingShort" : phase === "failed" ? "launcher.failedShort" : profile.loader === "quilt" ? "launcher.configOnly" : phase === "ready" ? "launcher.ready" : "launcher.vanillaInstance";
        return <button type="button" className="launcher-instance-card" data-loader={profile.loader} data-selected={selected?.id === profile.id ? "" : undefined} aria-pressed={selected?.id === profile.id} key={profile.id} onClick={() => setSelectedId(profile.id)}>
          <span className="launcher-instance-cover" aria-hidden="true"><span className="launcher-instance-emblem"><LoaderIcon glyph={info.glyph} size={40} /></span><span className="launcher-cover-version">{profile.game_version}</span></span>
          <span className="launcher-instance-body"><strong>{profile.name}</strong><span className="launcher-instance-meta"><LoaderIcon glyph={info.glyph} size={13} />{info.name}<span>·</span>{profile.memory_mb / 1024} GB</span><span className="launcher-instance-status" data-phase={phase}>{t(status)}</span></span>
        </button>;
      })}
      <button className="launcher-add-instance" type="button" onClick={() => setCreating(true)}><Icon name="plus" size={24} /><span>{t("launcher.newInstance")}</span></button>
    </section>
    {selected ? <aside className="launcher-instance-detail launcher-panel" aria-label={t("launcher.instanceDetails")} key={selected.id}>
      <div className="launcher-detail-heading"><span className="launcher-detail-icon"><LoaderIcon glyph={loaderInfo(selected.loader).glyph} size={25} /></span><div><p className="label">MINECRAFT JAVA</p><h2>{selected.name}</h2></div></div>
      <dl className="launcher-instance-specs"><div><dt>{t("launcher.version")}</dt><dd>{selected.game_version}</dd></div><div><dt>{t("launcher.loader")}</dt><dd>{loaderInfo(selected.loader).name}</dd></div><div><dt>{t("launcher.memoryShort")}</dt><dd>{selected.memory_mb / 1024} GB</dd></div></dl>
      {selected.loader_version ? <p className="launcher-muted">{loaderInfo(selected.loader).name} {selected.loader_version}</p> : null}
      {caps.data?.can_launch && selected.loader !== "quilt" ? renderControls(selected) : <p className="launcher-notice">{t(selected.loader === "quilt" ? "launcher.loaderPending" : "launcher.androidRuntime")}</p>}
      <div className="launcher-detail-footer"><Button icon="trash" variant="quiet" disabled={isBusy(selected.id)} onClick={() => { remove.reset(); setDeleting(selected); }}>{t("launcher.removeInstance")}</Button></div>
    </aside> : null}</div> : null}
    {caps.data?.platform === "android" ? <p className="launcher-muted">{t("launcher.androidRuntime")}</p> : <p className="launcher-library-footnote"><Icon name="info" size={14} />{t("launcher.libraryFootnote")}</p>}
    {creating ? <CreateInstanceDialog onClose={() => setCreating(false)} onCreated={profile => { setSearch(""); setLoader("all"); setSelectedId(profile.id); setCreating(false); }} /> : null}
    {deleting ? <Dialog open onClose={() => setDeleting(null)} title={t("launcher.removeInstance")} dismissable={!remove.isPending} footer={<><Button disabled={remove.isPending} onClick={() => setDeleting(null)}>{t("launcher.cancel")}</Button><Button variant="danger" disabled={remove.isPending || isBusy(deleting.id)} onClick={() => remove.mutate(deleting.id)}>{t("launcher.removeInstance")}</Button></>}><p>{t("launcher.confirmRemoveInstance", { name: deleting.name })}</p><p className="launcher-muted">{t("launcher.keepGameFiles")}</p>{remove.error ? <ErrorNotice error={toAppError(remove.error)} /> : null}</Dialog> : null}
  </>;
}
