import { useEffect, useState } from "react";
import { useLocation, useSearchParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { openExternal } from "@/lib/external";
import { IS_WEB } from "@/lib/web";
import { Button, ErrorNotice, Icon, Panel, Segmented, Select, StatusDot } from "@/ui";
import type { ContentKind, DeviceLogin, LauncherProfile, LauncherProject } from "./types";
import { Library } from "./Library";
import "./launcher.css";

const ROOT = ["launcher"];

export function LauncherScreen() {
  const [search] = useSearchParams();
  return <LauncherContent key={search.get("request") ?? "launcher"} />;
}

function LauncherContent() {
  const { t } = useI18n();
  const [search] = useSearchParams();
  const location = useLocation();
  const shortcutId = search.get("instance");
  const [tab, setTab] = useState<"library" | "discover" | "accounts">("library");
  // The instance (and kind of content) Discover installs into, when the person
  // came from an instance's "Add from Modrinth".
  const [target, setTarget] = useState<{ profile: LauncherProfile; kind: ContentKind } | null>(null);
  const [lastProfile, setLastProfile] = useState<string | null>(null);
  if (IS_WEB) return <div className="launcher"><h1 className="display servers-node">Launcher</h1><p className="launcher-muted">{t("launcher.localOnly")}</p></div>;
  return <div className="launcher">
    <header className="launcher-head">
      <div className="launcher-head-title">
        <h1 className="display servers-node">Launcher</h1>
        <p className="launcher-muted">{t("launcher.subtitle")}</p>
      </div>
      <Segmented<"library" | "discover" | "accounts"> label="Launcher" value={tab} onChange={setTab} options={[
        { id: "library", label: t("launcher.library"), icon: "servers" },
        { id: "discover", label: t("launcher.discover"), icon: "search" },
        { id: "accounts", label: t("launcher.accounts"), icon: "users" },
      ]} />
    </header>
    <Failure error={location.state?.shortcutError} />
    {tab === "library" ? <Library key={search.get("request") ?? "library"} focusId={shortcutId ?? lastProfile} renderControls={profile => <ProfileControls profile={profile} />} onBrowse={(profile, kind) => { setTarget({ profile, kind }); setTab("discover"); }} /> : tab === "discover" ? <Discover target={target} onOpenInstance={id => { setLastProfile(id); setTab("library"); }} /> : <Accounts />}
  </div>;
}

function Failure({ error }: { error: unknown }) {
  return error ? <ErrorNotice error={toAppError(error)} /> : null;
}

function ProfileControls({ profile }: { profile: LauncherProfile }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const states = useQuery({ queryKey: [...ROOT, "status"], queryFn: launcherApi.status, refetchInterval: 1000 });
  const accounts = useQuery({ queryKey: [...ROOT, "accounts"], queryFn: launcherApi.accounts, refetchInterval: 60_000 });
  const [account, setAccount] = useState("demo");
  const [showLogs, setShowLogs] = useState(false);
  const logs = useQuery({ queryKey: [...ROOT, "logs", profile.id], queryFn: () => launcherApi.logs(profile.id), enabled: showLogs, refetchInterval: showLogs ? 1500 : false });
  const state = states.data?.[profile.id];
  const installing = state?.phase === "installing";
  const running = state?.phase === "running" || state?.phase === "stopping";
  const caps = useQuery({ queryKey: [...ROOT, "capabilities"], queryFn: launcherApi.capabilities });
  const anyInstalling = Object.values(states.data ?? {}).some(s => s.phase === "installing");
  const start = useMutation({ mutationFn: (mode: "install" | "demo" | "account") => launcherApi.start(profile.id, mode, account === "demo" ? undefined : account), onSuccess: () => client.invalidateQueries({ queryKey: [...ROOT, "status"] }) });
  const resume = useMutation({ mutationFn: () => launcherApi.resume(profile.id) });
  const shortcut = useMutation({ mutationFn: () => launcherApi.pinShortcut(profile.id, account === "demo" ? "demo" : "account", account === "demo" ? undefined : account) });
  const stop = useMutation({ mutationFn: () => launcherApi.stop(profile.id), onSuccess: () => client.invalidateQueries({ queryKey: [...ROOT, "status"] }) });
  return <div className="launcher-client">
    <label className="launcher-field"><span className="label">{t("launcher.playAs")}</span><Select value={account} disabled={running || installing} onChange={e => setAccount(e.target.value)}><option value="demo">{t("launcher.officialDemo")}</option>{accounts.data?.map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</Select></label>
    {installing ? <div role="status" className="launcher-progress"><div className="launcher-progress-head"><span>{t(`launcher.stage_${state.stage}`)}</span><span className="readout">{state.total > 0 ? `${state.completed} / ${state.total}` : ""}</span></div><div className="meter" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={state.total ? Math.round(state.completed / state.total * 100) : undefined}><div className="meter-fill" data-indeterminate={state.total ? undefined : ""} style={{ width: state.total ? `${state.completed / state.total * 100}%` : "35%" }} /></div></div> : null}
    {running ? <p role="status" className="launcher-state"><StatusDot status="online" />{t("launcher.running")}</p> : null}
    {state?.phase === "ready" ? <p className="launcher-state"><Icon name="check" size={13} />{t("launcher.ready")}</p> : null}
    <div className="launcher-actions">
      {running && state?.phase === "running" && caps.data?.platform === "android" ? <Button variant="primary" icon="play" disabled={resume.isPending} onClick={() => resume.mutate()}>{t("launcher.returnToGame")}</Button> : null}
      {running || installing ? <Button icon="stop" disabled={stop.isPending} onClick={() => stop.mutate()}>{t(installing ? "launcher.cancel" : "launcher.stopGame")}</Button> : <>
        <Button variant="primary" icon="play" disabled={start.isPending || anyInstalling} onClick={() => start.mutate(account === "demo" ? "demo" : "account")}>{t(account === "demo" ? "launcher.playDemo" : "launcher.play")}</Button>
        <Button icon="download" disabled={start.isPending || anyInstalling} onClick={() => start.mutate("install")}>{t("launcher.installRepair")}</Button>
      </>}
      <Button variant="quiet" icon="terminal" aria-expanded={showLogs} onClick={() => setShowLogs(!showLogs)}>{t("launcher.gameLogs")}</Button>
      {caps.data?.platform === "android" ? <Button icon="plus" disabled={shortcut.isPending} onClick={() => shortcut.mutate()}>{t("launcher.pinShortcut")}</Button> : null}
    </div>
    {shortcut.isSuccess ? <p role="status" className="launcher-notice">{t("launcher.shortcutRequested")}</p> : null}
    <Failure error={start.error || stop.error || resume.error || shortcut.error || state?.error || states.error || accounts.error} />
    {showLogs ? <><Failure error={logs.error} /><pre className="launcher-game-log selectable mono" aria-label={t("launcher.gameLogs")}>{logs.data?.join("\n") || t("launcher.noLogs")}</pre></> : null}
  </div>;
}

function Discover({ target, onOpenInstance }: { target: { profile: LauncherProfile; kind: ContentKind } | null; onOpenInstance: (id: string) => void }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<ContentKind>(target?.kind ?? "modpack");
  const profiles = useQuery({ queryKey: [...ROOT, "profiles"], queryFn: launcherApi.profiles });
  const caps = useQuery({ queryKey: [...ROOT, "capabilities"], queryFn: launcherApi.capabilities });
  const [instanceId, setInstanceId] = useState(target?.profile.id ?? "");
  const chosen = profiles.data?.find(p => p.id === instanceId) ?? profiles.data?.find(p => p.id === target?.profile.id) ?? profiles.data?.[0];
  const canInstall = !!caps.data?.can_launch;
  const install = useMutation({
    mutationFn: async (project: LauncherProject) => {
      if (kind === "modpack") {
        const profile = await launcherApi.modpackInstall(project.project_id);
        return { project: project.project_id, profile, files: [] as string[] };
      }
      if (!chosen) throw { code: "invalid_input", params: { reason: "no instance" } };
      const files = await launcherApi.contentInstall(chosen.id, project.project_id, kind);
      return { project: project.project_id, profile: chosen, files };
    },
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: [...ROOT, "profiles"] });
      await client.invalidateQueries({ queryKey: [...ROOT, "content"] });
    },
  });
  const done = install.data;
  const [offset, setOffset] = useState(0);
  const result = useQuery({ queryKey: [...ROOT, "search", query, kind, offset], queryFn: () => launcherApi.search(query, kind, offset), staleTime: 120_000 });
  const [openError, setOpenError] = useState(false);
  return <>
    <div className="servers-toolbar launcher-toolbar">
      <form className="servers-search" onSubmit={e => { e.preventDefault(); setQuery(input.trim()); setOffset(0); }}>
        <span className="servers-search-icon" aria-hidden="true"><Icon name="search" size={14} /></span>
        <input type="search" className="field" aria-label={t("launcher.search")} placeholder={t("launcher.search")} maxLength={200} value={input} onChange={e => setInput(e.target.value)} />
      </form>
      <Segmented<ContentKind> size="sm" label={t("launcher.contentType")} value={kind} onChange={id => { setKind(id); setOffset(0); }} options={(["modpack", "mod", "resourcepack", "shader"] as const).map(id => ({ id, label: t(`launcher.${id}`) }))} />
      <Button size="sm" icon="search" onClick={() => { setQuery(input.trim()); setOffset(0); }}>{t("launcher.searchAction")}</Button>
    </div>
    {canInstall ? (kind === "modpack"
      ? <p className="launcher-muted">{t("launcher.packCreatesInstance")}</p>
      : <label className="launcher-field launcher-install-target"><span className="label">{t("launcher.installInto")}</span>
          <Select value={chosen?.id ?? ""} disabled={!profiles.data?.length || install.isPending} onChange={e => setInstanceId(e.target.value)}>
            {profiles.data?.map(p => <option key={p.id} value={p.id}>{p.name} · {p.game_version} · {p.loader}</option>)}
          </Select>
        </label>) : null}
    {canInstall && kind !== "modpack" && profiles.data?.length === 0 ? <p className="launcher-notice">{t("launcher.createInstanceFirst")}</p> : null}
    {canInstall ? null : <p className="launcher-muted">{t("launcher.catalogNote")}</p>}
    {install.error ? <Failure error={install.error} /> : null}
    {done ? <p className="launcher-notice" role="status"><Icon name="check" size={14} />{kind === "modpack" ? t("launcher.packCreated", { name: done.profile.name }) : done.files.length ? t("launcher.installedFiles", { count: done.files.length, name: done.profile.name }) : t("launcher.alreadyInstalled", { name: done.profile.name })} <Button size="sm" onClick={() => onOpenInstance(done.profile.id)}>{t("launcher.openInstance")}</Button></p> : null}
    <Failure error={result.error} />
    {result.isError ? <Button onClick={() => void result.refetch()}>{t("launcher.retry")}</Button> : null}
    {openError ? <p role="alert">{t("launcher.openFailed")}</p> : null}
    {result.isPending ? <p role="status" className="launcher-muted">{t("launcher.loading")}</p> : null}
    {result.data?.hits.length === 0 ? <div className="panel servers-empty"><div className="empty-state"><span className="empty-state-icon"><Icon name="search" size={20} /></span><p className="empty-state-title">{t("launcher.noResults")}</p></div></div> : null}
    <div className="servers-grid">{result.data?.hits.map(project => <article className="panel server-card launcher-project" key={project.project_id}>
      <header className="flex items-start gap-3.5">
        <span className="launcher-project-icon" aria-hidden="true">{project.icon_url?.startsWith("https://") ? <img src={project.icon_url} alt="" loading="lazy" referrerPolicy="no-referrer" /> : <Icon name={kind === "shader" ? "sun" : kind === "resourcepack" ? "image" : "puzzle"} size={18} />}</span>
        <div className="flex min-w-0 flex-1 flex-col gap-1.5">
          <h3 className="launcher-project-title" title={project.title}>{project.title}</h3>
          <div className="server-card-tags"><span className="tag">{project.author}</span><span className="tag">{new Intl.NumberFormat(undefined, { notation: "compact" }).format(project.downloads)} {t("launcher.downloads")}</span></div>
        </div>
      </header>
      <p className="launcher-description">{project.description}</p>
      <div className="launcher-actions">
        {canInstall ? <Button variant="primary" icon={kind === "modpack" ? "plus" : "download"} disabled={install.isPending || (kind !== "modpack" && !chosen)} onClick={() => install.mutate(project)}>{install.isPending && install.variables?.project_id === project.project_id ? t("launcher.installing") : t(kind === "modpack" ? "launcher.createFromPack" : "launcher.install")}</Button> : null}
        <Button icon="external" iconOnly title={t("launcher.viewProject")} onClick={() => { void openExternal(`https://modrinth.com/project/${encodeURIComponent(project.project_id)}`).then(ok => setOpenError(!ok)); }}>{t("launcher.viewProject")}</Button>
      </div>
    </article>)}</div>
    <div className="launcher-pager"><Button size="sm" disabled={!offset || result.isFetching} onClick={() => setOffset(Math.max(0, offset - 20))}>{t("launcher.previous")}</Button><span className="readout">{Math.floor(offset / 20) + 1}</span><Button size="sm" disabled={!result.data || offset + 20 >= result.data.total_hits || offset >= 10000 || result.isFetching} onClick={() => setOffset(offset + 20)}>{t("launcher.next")}</Button></div>
  </>;
}

function Accounts() {
  const { t } = useI18n();
  const client = useQueryClient();
  const accounts = useQuery({ queryKey: [...ROOT, "accounts"], queryFn: launcherApi.accounts, refetchInterval: 60_000 });
  const caps = useQuery({ queryKey: [...ROOT, "capabilities"], queryFn: launcherApi.capabilities });
  const begin = useMutation({ mutationFn: () => launcherApi.begin() });
  const logout = useMutation({ mutationFn: launcherApi.logout, onSuccess: () => client.invalidateQueries({ queryKey: [...ROOT, "accounts"] }) });
  return <Panel title={t("launcher.accounts")} bodyClassName="launcher-panel-body"><p className="launcher-muted">{t("launcher.accountsNote")}</p>
    {begin.data ? <DeviceFlow login={begin.data} onClose={() => begin.reset()} /> : <form className="launcher-signin" onSubmit={e => { e.preventDefault(); begin.mutate(); }}>
      <Button type="submit" variant="primary" disabled={begin.isPending}>{t("launcher.signIn")}</Button>
    </form>}
    <Failure error={begin.error || logout.error || accounts.error || caps.error} />
    {accounts.data?.map(account => <div className="launcher-account" key={account.id}><div><strong>{account.name}</strong><p className="launcher-muted">{t("launcher.verified")}</p></div><Button onClick={() => logout.mutate(account.id)} disabled={logout.isPending}>{t("launcher.signOut")}</Button></div>)}
  </Panel>;
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
