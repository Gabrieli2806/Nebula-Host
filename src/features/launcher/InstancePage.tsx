/**
 * One instance, laid out like a server: a header with its picture, tags,
 * state and the Play button, then tabs for what it has, adding more, its
 * log and its settings.
 */

import { useRef, useState } from "react";
import { useContextMenu } from "@/ui/ContextMenu";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, ErrorNotice, Icon, StatusDot, Tabs, statusStyle } from "@/ui";
import { Browse } from "./Browse";
import { InstanceContent } from "./Content";
import { InstanceLog } from "./InstanceLog";
import { InstanceSettingsPanel, InstanceTile, squareIcon } from "./InstanceSettings";
import { loaderInfo } from "./loaderOptions";
import { usePrimaryAccount } from "./primaryAccount";
import type { ContentKind, LauncherProfile } from "./types";

type Tab = "content" | "add" | "log" | "settings";

export function phaseDot(phase?: string): "online" | "starting" | "crashed" | "offline" {
  return phase === "running" ? "online" : phase === "installing" || phase === "stopping" ? "starting" : phase === "failed" ? "crashed" : "offline";
}
export function phaseLabel(phase?: string): string {
  return phase === "running" ? "launcher.runningShort" : phase === "installing" ? "launcher.installingShort" : phase === "failed" ? "launcher.failedShort" : "status.offline";
}

export function InstancePage({ profile, onBack, onRemoved }: { profile: LauncherProfile; onBack: () => void; onRemoved: () => void }) {
  const { t } = useI18n();
  const [tab, setTab] = useState<Tab>("content");
  const [addKind, setAddKind] = useState<Exclude<ContentKind, "modpack">>("mod");
  const caps = useQuery({ queryKey: ["launcher", "capabilities"], queryFn: launcherApi.capabilities });
  const states = useQuery({ queryKey: ["launcher", "status"], queryFn: launcherApi.status, refetchInterval: 1000 });
  const phase = states.data?.[profile.id]?.phase;
  const busy = phase === "running" || phase === "installing" || phase === "stopping";
  const canLaunch = !!caps.data?.can_launch && profile.loader !== "quilt";

  return (
    <div className="launcher-instance-page">
      <button type="button" className="rail-back launcher-back" onClick={onBack}>
        <Icon name="chevron-left" size={14} />{t("launcher.yourInstances")}
      </button>

      <section className="panel launcher-instance-hero">
        <PictureButton profile={profile} />
        <div className="min-w-0 flex-1">
          <div className="server-card-title">
            <h2 className="launcher-detail-title" title={profile.name}>{profile.name}</h2>
            <span className="status-chip flex-none" style={statusStyle(phaseDot(phase))}><StatusDot status={phaseDot(phase)} />{t(phaseLabel(phase))}</span>
          </div>
          <div className="server-card-tags">
            <span className="tag">{loaderInfo(profile.loader).name}{profile.loader_version ? ` ${profile.loader_version}` : ""}</span>
            <span className="tag">{profile.game_version}</span>
            <span className="tag">{profile.memory_mb / 1024} GB</span>
          </div>
        </div>
        {canLaunch ? <PlayControls profile={profile} /> : null}
      </section>
      {!canLaunch ? <p className="launcher-notice"><Icon name="info" size={14} />{t(profile.loader === "quilt" ? "launcher.loaderPending" : "launcher.androidRuntime")}</p> : null}
      {canLaunch ? <Progress profile={profile} /> : null}

      <Tabs<Tab>
        label={profile.name}
        active={tab}
        onChange={setTab}
        tabs={[
          { id: "content", label: t("launcher.content"), icon: "puzzle" },
          { id: "add", label: t("launcher.addContent"), icon: "plus" },
          { id: "log", label: t("launcher.gameLogs"), icon: "terminal" },
          { id: "settings", label: t("launcher.instanceSettings"), icon: "sliders" },
        ]}
      />

      {tab === "content" ? <InstanceContent profile={profile} onBrowse={(kind) => { if (kind !== "modpack") setAddKind(kind); setTab("add"); }} /> : null}
      {tab === "add" ? <Browse key={addKind} profile={profile} initialKind={addKind} /> : null}
      {tab === "log" ? <InstanceLog profile={profile} /> : null}
      {tab === "settings" ? <InstanceSettingsPanel profile={profile} busy={busy} onRemoved={onRemoved} /> : null}
    </div>
  );
}

/** The instance picture; click to choose a new one (saved right away). */
function PictureButton({ profile }: { profile: LauncherProfile }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const picker = useRef<HTMLInputElement>(null);
  const [failed, setFailed] = useState(false);
  const save = useMutation({
    mutationFn: (icon: string | null) => launcherApi.updateDetails(profile.id, profile.name, icon),
    onSuccess: () => client.invalidateQueries({ queryKey: ["launcher", "profiles"] }),
    onError: () => setFailed(true),
  });
  const { open, menu } = useContextMenu();
  const choose = () => picker.current?.click();
  return (
    <>
      <button
        type="button"
        className="launcher-picture"
        title={failed ? t("launcher.iconFailed") : t("launcher.chooseIcon")}
        aria-label={t("launcher.chooseIcon")}
        disabled={save.isPending}
        onClick={choose}
        onContextMenu={(event) => open(event, [
          { label: t("launcher.chooseIcon"), icon: "image", onSelect: choose },
          { label: t("launcher.resetIcon"), icon: "restore", disabled: !profile.icon, onSelect: () => save.mutate(null) },
        ])}
      >
        <InstanceTile profile={profile} size={64} />
        <span className="launcher-picture-edit" aria-hidden="true"><Icon name="image" size={14} /></span>
      </button>
      <input
        ref={picker}
        type="file"
        accept="image/png,image/jpeg,image/webp,image/gif"
        hidden
        onChange={(e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (!file) return;
          setFailed(false);
          squareIcon(file).then((icon) => save.mutate(icon), () => setFailed(true));
        }}
      />
      {menu}
    </>
  );
}

/** Play with the primary account (the demo without one), or stop / return. */
export function PlayControls({ profile, compact = false }: { profile: LauncherProfile; compact?: boolean }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const { primary } = usePrimaryAccount();
  const states = useQuery({ queryKey: ["launcher", "status"], queryFn: launcherApi.status, refetchInterval: 1000 });
  const caps = useQuery({ queryKey: ["launcher", "capabilities"], queryFn: launcherApi.capabilities });
  const state = states.data?.[profile.id];
  const installing = state?.phase === "installing";
  const running = state?.phase === "running" || state?.phase === "stopping";
  const anyInstalling = Object.values(states.data ?? {}).some((s) => s.phase === "installing");
  const refresh = () => client.invalidateQueries({ queryKey: ["launcher", "status"] });
  const start = useMutation({ mutationFn: () => launcherApi.start(profile.id, primary ? "account" : "demo", primary?.id), onSuccess: refresh });
  const stop = useMutation({ mutationFn: () => launcherApi.stop(profile.id), onSuccess: refresh });
  const resume = useMutation({ mutationFn: () => launcherApi.resume(profile.id) });
  const size = compact ? "sm" : undefined;
  const error = start.error || stop.error || resume.error;

  return (
    <div className="launcher-play-controls">
      {running && state?.phase === "running" && caps.data?.platform === "android" ? (
        <Button size={size} variant="primary" icon="play" disabled={resume.isPending} onClick={() => resume.mutate()}>{t("launcher.returnToGame")}</Button>
      ) : null}
      {running || installing ? (
        <Button size={size} icon="stop" disabled={stop.isPending} onClick={() => stop.mutate()}>{t(installing ? "launcher.cancel" : "launcher.stopGame")}</Button>
      ) : (
        <Button size={size} variant="primary" icon="play" className={compact ? undefined : "launcher-play"} disabled={start.isPending || anyInstalling} onClick={() => start.mutate()}>
          {t(primary ? "launcher.play" : "launcher.playDemo")}
        </Button>
      )}
      {error && !compact ? <ErrorNotice error={toAppError(error)} compact /> : null}
    </div>
  );
}

function Progress({ profile }: { profile: LauncherProfile }) {
  const { t } = useI18n();
  const states = useQuery({ queryKey: ["launcher", "status"], queryFn: launcherApi.status, refetchInterval: 1000 });
  const state = states.data?.[profile.id];
  if (state?.phase === "installing") {
    const percent = state.total ? (state.completed / state.total) * 100 : null;
    return (
      <div role="status" className="launcher-progress">
        <div className="launcher-progress-head"><span>{t(`launcher.stage_${state.stage}`)}</span><span className="readout">{state.total > 0 ? `${state.completed} / ${state.total}` : ""}</span></div>
        <div className="meter" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent === null ? undefined : Math.round(percent)}>
          <div className="meter-fill" data-indeterminate={percent === null ? "" : undefined} style={{ width: percent === null ? "35%" : `${percent}%` }} />
        </div>
      </div>
    );
  }
  return state?.error ? <ErrorNotice error={toAppError(state.error)} compact /> : null;
}
