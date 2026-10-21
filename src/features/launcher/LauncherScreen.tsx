import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { openExternal } from "@/lib/external";
import { IS_WEB } from "@/lib/web";
import { Button, ErrorNotice } from "@/ui";
import type { ContentKind, DeviceLogin, Loader } from "./types";
import "./launcher.css";

const ROOT = ["launcher"];

export function LauncherScreen() {
  const { t } = useI18n();
  const [tab, setTab] = useState<"library" | "discover" | "accounts">("library");
  if (IS_WEB) return <div className="launcher"><h1>Launcher</h1><p>{t("launcher.localOnly")}</p></div>;
  return <div className="launcher">
    <header><p className="label">MINECRAFT JAVA EDITION</p><h1>Launcher</h1><p>{t("launcher.subtitle")}</p></header>
    <nav className="launcher-tabs" aria-label="Launcher">
      {(["library", "discover", "accounts"] as const).map(id => <Button key={id} aria-pressed={tab === id} variant={tab === id ? "primary" : "default"} onClick={() => setTab(id)}>{t(`launcher.${id}`)}</Button>)}
    </nav>
    {tab === "library" ? <Library /> : tab === "discover" ? <Discover /> : <Accounts />}
  </div>;
}

function Failure({ error }: { error: unknown }) {
  return error ? <ErrorNotice error={toAppError(error)} /> : null;
}

function Library() {
  const { t } = useI18n();
  const client = useQueryClient();
  const profiles = useQuery({ queryKey: [...ROOT, "profiles"], queryFn: launcherApi.profiles });
  const capabilities = useQuery({ queryKey: [...ROOT, "capabilities"], queryFn: launcherApi.capabilities });
  const versions = useQuery({ queryKey: [...ROOT, "versions"], queryFn: launcherApi.versions, staleTime: 300_000 });
  const [name, setName] = useState("");
  const [version, setVersion] = useState("");
  const [loader, setLoader] = useState<Loader>("vanilla");
  const [memory, setMemory] = useState(4096);
  const [snapshots, setSnapshots] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);
  const available = versions.data?.filter(v => snapshots || v.type === "release") ?? [];
  const selected = available.some(v => v.id === version) ? version : available[0]?.id ?? "";
  const create = useMutation({ mutationFn: launcherApi.create, onSuccess: async () => {
    setName(""); await client.invalidateQueries({ queryKey: [...ROOT, "profiles"] });
  }});
  const remove = useMutation({ mutationFn: launcherApi.remove, onSuccess: async () => {
    setDeleting(null); await client.invalidateQueries({ queryKey: [...ROOT, "profiles"] });
  }});
  return <>
    <aside className="launcher-notice" role="note">{t(capabilities.data?.platform === "android" ? "launcher.androidRuntime" : "launcher.desktopRuntime")}</aside>
    <section className="launcher-panel"><h2>{t("launcher.newProfile")}</h2>
      <form className="launcher-form" onSubmit={e => { e.preventDefault(); create.mutate({ name, game_version: selected, loader, memory_mb: memory }); }}>
        <label>{t("launcher.name")}<input required maxLength={64} value={name} onChange={e => setName(e.target.value)} placeholder={t("launcher.namePlaceholder")} /></label>
        <label>{t("launcher.version")}<select value={selected} onChange={e => setVersion(e.target.value)} disabled={!available.length}>{available.map(v => <option key={v.id}>{v.id}</option>)}</select></label>
        <label>{t("launcher.loader")}<select value={loader} onChange={e => setLoader(e.target.value as Loader)}>{["vanilla", "fabric", "forge", "neoforge", "quilt"].map(id => <option key={id} value={id}>{id}</option>)}</select></label>
        <label>{t("launcher.memory")}<input type="number" required min={512} max={32768} step={256} value={memory} onChange={e => setMemory(e.target.valueAsNumber)} /></label>
        <label className="launcher-check"><input type="checkbox" checked={snapshots} onChange={e => setSnapshots(e.target.checked)} />{t("launcher.snapshots")}</label>
        <Button type="submit" variant="primary" icon="plus" disabled={create.isPending || !selected || !name.trim()}>{t(create.isPending ? "launcher.saving" : "launcher.create")}</Button>
      </form>
      {versions.isPending ? <p role="status">{t("launcher.loading")}</p> : null}
      <Failure error={versions.error || create.error} />
      {versions.isError ? <Button onClick={() => void versions.refetch()}>{t("launcher.retry")}</Button> : null}
      <p className="launcher-muted">{t("launcher.profileNote")}</p>
    </section>
    <Failure error={profiles.error || remove.error || capabilities.error} />
    <section className="launcher-grid" aria-label={t("launcher.library")}>
      {profiles.isPending ? <p role="status">{t("launcher.loading")}</p> : null}
      {profiles.data?.length === 0 ? <p>{t("launcher.empty")}</p> : null}
      {profiles.data?.map(profile => <article className="launcher-panel" key={profile.id}>
        <p className="label">{profile.loader.toUpperCase()}</p><h2>{profile.name}</h2><p>Minecraft {profile.game_version} · {profile.memory_mb} MiB</p>
        <p className="launcher-muted">{t("launcher.profilePending")}</p>
        {deleting === profile.id ? <div className="launcher-actions"><span>{t("launcher.confirmRemove")}</span><Button variant="danger" disabled={remove.isPending} onClick={() => remove.mutate(profile.id)}>{t("launcher.remove")}</Button><Button onClick={() => setDeleting(null)}>{t("launcher.cancel")}</Button></div> : <Button icon="trash" variant="quiet" onClick={() => setDeleting(profile.id)}>{t("launcher.remove")}</Button>}
      </article>)}
    </section>
  </>;
}

function Discover() {
  const { t } = useI18n();
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<ContentKind>("modpack");
  const [offset, setOffset] = useState(0);
  const result = useQuery({ queryKey: [...ROOT, "search", query, kind, offset], queryFn: () => launcherApi.search(query, kind, offset), staleTime: 120_000 });
  const [openError, setOpenError] = useState(false);
  return <>
    <form className="launcher-search" onSubmit={e => { e.preventDefault(); setQuery(input.trim()); setOffset(0); }}>
      <input aria-label={t("launcher.search")} placeholder={t("launcher.search")} maxLength={200} value={input} onChange={e => setInput(e.target.value)} />
      <select aria-label={t("launcher.contentType")} value={kind} onChange={e => { setKind(e.target.value as ContentKind); setOffset(0); }}>{(["modpack", "mod", "resourcepack", "shader"] as const).map(id => <option key={id} value={id}>{t(`launcher.${id}`)}</option>)}</select>
      <Button type="submit" icon="search">{t("launcher.searchAction")}</Button>
    </form>
    <p className="launcher-muted">{t("launcher.catalogNote")}</p>
    <Failure error={result.error} />
    {result.isError ? <Button onClick={() => void result.refetch()}>{t("launcher.retry")}</Button> : null}
    {openError ? <p role="alert">{t("launcher.openFailed")}</p> : null}
    {result.isPending ? <p role="status">{t("launcher.loading")}</p> : null}
    {result.data?.hits.length === 0 ? <p>{t("launcher.noResults")}</p> : null}
    <div className="launcher-grid">{result.data?.hits.map(project => <article className="launcher-panel launcher-project" key={project.project_id}>
      {project.icon_url?.startsWith("https://") ? <img src={project.icon_url} alt="" loading="lazy" referrerPolicy="no-referrer" /> : null}
      <div><h2>{project.title}</h2><p className="launcher-muted">{project.author} · {project.downloads.toLocaleString()} {t("launcher.downloads")}</p></div>
      <p className="launcher-description">{project.description}</p>
      <Button icon="external" onClick={() => { void openExternal(`https://modrinth.com/project/${encodeURIComponent(project.project_id)}`).then(ok => setOpenError(!ok)); }}>{t("launcher.viewProject")}</Button>
    </article>)}</div>
    <div className="launcher-actions"><Button disabled={!offset || result.isFetching} onClick={() => setOffset(Math.max(0, offset - 20))}>{t("launcher.previous")}</Button><span>{Math.floor(offset / 20) + 1}</span><Button disabled={!result.data || offset + 20 >= result.data.total_hits || offset >= 10000 || result.isFetching} onClick={() => setOffset(offset + 20)}>{t("launcher.next")}</Button></div>
  </>;
}

function Accounts() {
  const { t } = useI18n();
  const client = useQueryClient();
  const accounts = useQuery({ queryKey: [...ROOT, "accounts"], queryFn: launcherApi.accounts, refetchInterval: 60_000 });
  const caps = useQuery({ queryKey: [...ROOT, "capabilities"], queryFn: launcherApi.capabilities });
  const [clientId, setClientId] = useState("");
  const begin = useMutation({ mutationFn: () => launcherApi.begin(clientId.trim() || undefined) });
  const logout = useMutation({ mutationFn: launcherApi.logout, onSuccess: () => client.invalidateQueries({ queryKey: [...ROOT, "accounts"] }) });
  return <section className="launcher-panel"><h2>{t("launcher.accounts")}</h2><p>{t("launcher.accountsNote")}</p>
    {begin.data ? <DeviceFlow login={begin.data} onClose={() => begin.reset()} /> : <form className="launcher-search" onSubmit={e => { e.preventDefault(); begin.mutate(); }}>
      <label>{t("launcher.clientId")}<input placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx" value={clientId} onChange={e => setClientId(e.target.value)} required={!caps.data?.microsoft_configured} pattern="[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}" /></label>
      <Button type="submit" variant="primary" disabled={begin.isPending}>{t("launcher.signIn")}</Button>
    </form>}
    <p className="launcher-muted">{t("launcher.clientIdHelp")}</p>
    <Failure error={begin.error || logout.error || accounts.error || caps.error} />
    {accounts.data?.map(account => <div className="launcher-account" key={account.id}><div><strong>{account.name}</strong><p className="launcher-muted">{t("launcher.verified")}</p></div><Button onClick={() => logout.mutate(account.id)} disabled={logout.isPending}>{t("launcher.signOut")}</Button></div>)}
  </section>;
}

function DeviceFlow({ login, onClose }: { login: DeviceLogin; onClose: () => void }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const [deadline] = useState(() => Date.now() + login.expires_in * 1000);
  const [openError, setOpenError] = useState(false);
  const poll = useQuery({ queryKey: [...ROOT, "login", login.id], queryFn: async () => {
    if (Date.now() >= deadline) throw { code: "cancelled", params: {} };
    return launcherApi.poll(login.id);
  }, retry: false, refetchOnWindowFocus: false, staleTime: Infinity,
  refetchInterval: query => query.state.error || query.state.data?.status === "complete" ? false : (query.state.data?.status === "pending" ? query.state.data.interval : login.interval) * 1000 });
  useEffect(() => {
    if (poll.data?.status === "complete") void client.invalidateQueries({ queryKey: [...ROOT, "accounts"] });
  }, [poll.data, client]);
  useEffect(() => () => { void launcherApi.cancel(login.id).catch(() => {}); }, [login.id]);
  return <div className="launcher-notice" role="status">
    {poll.data?.status === "complete" ? <p>{t("launcher.signedIn", { name: poll.data.account.name })}</p> : <><p>{t("launcher.enterCode")}</p><code className="launcher-code">{login.user_code}</code><p><Button icon="external" onClick={() => { void openExternal(login.verification_uri).then(ok => setOpenError(!ok)); }}>{t("launcher.openMicrosoft")}</Button></p></>}
    {openError ? <p role="alert">{t("launcher.openFailed")}</p> : null}
    <Failure error={poll.error} /><Button onClick={onClose}>{t(poll.data?.status === "complete" ? "launcher.close" : "launcher.cancel")}</Button>
  </div>;
}
