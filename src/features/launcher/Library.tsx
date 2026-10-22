import { useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, Icon, LoaderIcon, Panel, Select, StatusDot, statusStyle } from "@/ui";
import { RamSlider } from "@/ui/RamSlider";
import { CreateInstanceDialog } from "./CreateInstanceDialog";
import { InstanceContent } from "./Content";
import { LOADERS, loaderInfo } from "./loaderOptions";
import type { ContentKind, LauncherProfile, Loader } from "./types";

/** The card's colour: the loader's, so each kind of instance reads at a glance. */
const LOADER_COLOUR: Record<Loader, string> = {
  vanilla: "#5cb88e",
  fabric: "#d9b67e",
  forge: "#83a7dd",
  neoforge: "#efa36b",
  quilt: "#ba9be5",
};

export function Library({ renderControls, onBrowse, focusId }: { renderControls: (profile: LauncherProfile) => ReactNode; onBrowse: (profile: LauncherProfile, kind: ContentKind) => void; focusId?: string | null }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const profiles = useQuery({ queryKey: ["launcher", "profiles"], queryFn: launcherApi.profiles });
  const caps = useQuery({ queryKey: ["launcher", "capabilities"], queryFn: launcherApi.capabilities });
  const statuses = useQuery({ queryKey: ["launcher", "status"], queryFn: launcherApi.status, refetchInterval: 1000 });
  const [creating, setCreating] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(focusId ?? null);
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
  const phaseOf = (profile: LauncherProfile) => statuses.data?.[profile.id]?.phase;
  // One short word per state, like a server's chip.
  const statusOf = (profile: LauncherProfile) => {
    const phase = phaseOf(profile);
    return phase === "running" ? "launcher.runningShort" : phase === "installing" ? "launcher.installingShort" : phase === "failed" ? "launcher.failedShort" : "status.offline";
  };
  const dotOf = (profile: LauncherProfile): "online" | "starting" | "crashed" | "offline" => {
    const phase = phaseOf(profile);
    return phase === "running" ? "online" : phase === "installing" ? "starting" : phase === "failed" ? "crashed" : "offline";
  };
  const empty = profiles.data?.length === 0;

  return <>
    {profiles.data?.length ? <div className="servers-toolbar launcher-toolbar">
      <label className="servers-search">
        <span className="servers-search-icon" aria-hidden="true"><Icon name="search" size={14} /></span>
        <input type="search" className="field" aria-label={t("launcher.searchInstances")} placeholder={t("launcher.searchInstances")} value={search} onChange={e => setSearch(e.target.value)} />
      </label>
      <Select className="launcher-filter" aria-label={t("launcher.filterLoader")} value={loader} onChange={e => setLoader(e.target.value as Loader | "all")}>
        <option value="all">{t("launcher.allLoaders")}</option>{LOADERS.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
      </Select>
      <span className="launcher-toolbar-spacer" />
      <Button variant="primary" icon="plus" onClick={() => setCreating(true)}>{t("launcher.newInstance")}</Button>
    </div> : null}
    {failure ? <ErrorNotice error={toAppError(failure)} /> : null}
    {profiles.isPending ? <p role="status" className="launcher-muted">{t("launcher.loading")}</p> : null}
    {profiles.isError ? <Button onClick={() => void profiles.refetch()}>{t("launcher.retry")}</Button> : null}
    {empty ? <div className="panel servers-empty"><div className="empty-state">
      <span className="empty-state-icon"><LoaderIcon glyph="cube" size={26} /></span>
      <p className="empty-state-title">{t("launcher.emptyTitle")}</p>
      <p className="empty-state-hint">{t("launcher.emptyDescription")}</p>
      <div className="mt-1"><Button variant="primary" icon="plus" onClick={() => setCreating(true)}>{t("launcher.createFirstInstance")}</Button></div>
    </div></div> : null}
    {profiles.data?.length && visible.length === 0 ? <div className="panel servers-empty"><div className="empty-state">
      <span className="empty-state-icon"><Icon name="search" size={20} /></span>
      <p className="empty-state-title">{t("launcher.noMatchingInstances")}</p>
      <div className="mt-1"><Button onClick={() => { setSearch(""); setLoader("all"); }}>{t("launcher.clearFilters")}</Button></div>
    </div></div> : null}
    {visible.length ? <div className="launcher-library-layout">
      <section className="servers-grid launcher-instance-grid" aria-label={t("launcher.yourInstances")}>
        {visible.map(profile => {
          const info = loaderInfo(profile.loader);
          const phase = phaseOf(profile);
          return <article key={profile.id} className="panel card-interactive server-card launcher-instance" data-selected={selected?.id === profile.id ? "" : undefined} data-phase={phase}>
            <header className="flex items-start gap-3.5">
              <span className="launcher-tile" style={{ ["--tile" as string]: LOADER_COLOUR[profile.loader] }} aria-hidden="true"><LoaderIcon glyph={info.glyph} size={20} /></span>
              <div className="flex min-w-0 flex-1 flex-col gap-2">
                <div className="server-card-title">
                  <h3 className="pt-[1px]"><button type="button" className="server-card-open" aria-pressed={selected?.id === profile.id} title={profile.name} onClick={() => setSelectedId(profile.id)}>{profile.name}</button></h3>
                  <span className="status-chip flex-none" style={statusStyle(dotOf(profile))}><StatusDot status={dotOf(profile)} />{t(statusOf(profile))}</span>
                </div>
                <div className="server-card-tags"><span className="tag">{info.name}</span><span className="tag">{profile.game_version}</span><span className="tag">{profile.memory_mb / 1024} GB</span></div>
              </div>
            </header>
          </article>;
        })}
      </section>
      {selected ? <Panel title={selected.name} className="launcher-instance-detail" bodyClassName="launcher-detail-body" key={selected.id}>
        <dl className="launcher-facts">
          <div><dt>{t("launcher.version")}</dt><dd className="readout">{selected.game_version}</dd></div>
          <div><dt>{t("launcher.loader")}</dt><dd>{loaderInfo(selected.loader).name}{selected.loader_version ? <span className="launcher-muted"> {selected.loader_version}</span> : null}</dd></div>
        </dl>
        <MemoryEditor profile={selected} disabled={isBusy(selected.id)} />
        {caps.data?.can_launch && selected.loader !== "quilt" ? renderControls(selected) : <p className="launcher-notice"><Icon name="info" size={14} />{t(selected.loader === "quilt" ? "launcher.loaderPending" : "launcher.androidRuntime")}</p>}
        {caps.data?.can_launch ? <InstanceContent profile={selected} onBrowse={kind => onBrowse(selected, kind)} /> : null}
        <div className="launcher-detail-footer"><Button icon="trash" variant="quiet" disabled={isBusy(selected.id)} onClick={() => { remove.reset(); setDeleting(selected); }}>{t("launcher.removeInstance")}</Button></div>
      </Panel> : null}
    </div> : null}
    {caps.data?.platform === "android" ? <p className="launcher-footnote">{t("launcher.androidRuntime")}</p> : visible.length ? <p className="launcher-footnote"><Icon name="info" size={13} />{t("launcher.libraryFootnote")}</p> : null}
    {creating ? <CreateInstanceDialog onClose={() => setCreating(false)} onCreated={profile => { setSearch(""); setLoader("all"); setSelectedId(profile.id); setCreating(false); }} /> : null}
    {deleting ? <Dialog open onClose={() => setDeleting(null)} title={t("launcher.removeInstance")} dismissable={!remove.isPending} footer={<><Button variant="quiet" disabled={remove.isPending} onClick={() => setDeleting(null)}>{t("launcher.cancel")}</Button><Button variant="danger" disabled={remove.isPending || isBusy(deleting.id)} onClick={() => remove.mutate(deleting.id)}>{t("launcher.removeInstance")}</Button></>}><div className="flex flex-col gap-3"><p>{t("launcher.confirmRemoveInstance", { name: deleting.name })}</p><p className="launcher-muted">{t("launcher.keepGameFiles")}</p>{remove.error ? <ErrorNotice error={toAppError(remove.error)} /> : null}</div></Dialog> : null}
  </>;
}

/** The game's memory, with the same slider and guidance the servers use. */
function MemoryEditor({ profile, disabled }: { profile: LauncherProfile; disabled: boolean }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const [value, setValue] = useState(profile.memory_mb);
  const save = useMutation({
    mutationFn: (mb: number) => launcherApi.setMemory(profile.id, mb),
    onSuccess: () => client.invalidateQueries({ queryKey: ["launcher", "profiles"] }),
  });
  return <section className="launcher-memory">
    <span id={`memory-${profile.id}`} className="label">{t("launcher.memory")}</span>
    <RamSlider
      value={value}
      onChange={setValue}
      onCommit={mb => { if (mb !== profile.memory_mb) save.mutate(mb); }}
      projectType={profile.loader === "vanilla" ? null : "mod"}
      labelledBy={`memory-${profile.id}`}
      disabled={disabled || save.isPending}
    />
    {save.error ? <ErrorNotice error={toAppError(save.error)} compact /> : null}
  </section>;
}
