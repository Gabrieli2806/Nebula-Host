/**
 * One instance, laid out like a server: a header with its picture, tags,
 * state and the Play button, then tabs for what it has, adding more, its
 * log and its settings.
 *
 * On a narrow screen Play moves from the header to a bar pinned at the
 * bottom (launcher.css swaps which of the two is shown), so it stays within
 * reach of a thumb however far the content scrolls.
 */

import { useRef, useState } from "react";
import { useContextMenu } from "@/ui/ContextMenu";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { formatMegabytes } from "@/lib/format";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, ErrorNotice, Icon, StatusChip, Tabs } from "@/ui";
import { Browse } from "./Browse";
import { InstanceContent } from "./Content";
import { isBusy, launcherKeys, phaseDot, phaseLabel, useLauncherCapabilities, useLauncherStatus } from "./hooks";
import { InstanceLog } from "./InstanceLog";
import { InstanceSettingsPanel, InstanceTile, squareIcon } from "./InstanceSettings";
import { loaderInfo } from "./loaderOptions";
import { usePrimaryAccount } from "./primaryAccount";
import type { ContentKind, LauncherProfile } from "./types";

type Tab = "content" | "add" | "log" | "settings";

export function InstancePage({ profile, onBack, onRemoved }: { profile: LauncherProfile; onBack: () => void; onRemoved: () => void }) {
  const { t } = useI18n();
  const [tab, setTab] = useState<Tab>("content");
  const [addKind, setAddKind] = useState<Exclude<ContentKind, "modpack">>("mod");
  const caps = useLauncherCapabilities();
  const states = useLauncherStatus();
  const phase = states.data?.[profile.id]?.phase;
  const busy = isBusy(phase);
  const canLaunch = !!caps.data?.can_launch && profile.loader !== "quilt";

  return (
    <article className="launcher-instance-page" aria-label={profile.name}>
      <button type="button" className="rail-back launcher-back" onClick={onBack}>
        <Icon name="chevron-left" size={14} />{t("launcher.yourInstances")}
      </button>

      <section className="panel launcher-hero">
        <PictureButton profile={profile} />
        <div className="launcher-hero-text">
          <div className="launcher-hero-title">
            <h2 title={profile.name}>{profile.name}</h2>
            <StatusChip status={phaseDot(phase)} label={t(phaseLabel(phase))} />
          </div>
          <div className="launcher-card-tags">
            <span className="tag">{loaderInfo(profile.loader).name}{profile.loader_version ? ` ${profile.loader_version}` : ""}</span>
            <span className="tag">{profile.game_version}</span>
            <span className="tag">{formatMegabytes(profile.memory_mb)}</span>
          </div>
        </div>
        {canLaunch ? <div className="launcher-hero-play"><PlayControls profile={profile} /></div> : null}
      </section>
      {!canLaunch ? <p className="launcher-notice"><Icon name="info" size={14} />{t(profile.loader === "quilt" ? "launcher.loaderPending" : "launcher.androidRuntime")}</p> : null}
      {canLaunch ? <Progress profile={profile} /> : null}

      <Tabs<Tab>
        label={profile.name}
        className="launcher-tabs"
        active={tab}
        onChange={setTab}
        tabs={[
          { id: "content", label: t("launcher.content"), icon: "puzzle" },
          { id: "add", label: t("launcher.addContent"), icon: "plus" },
          { id: "log", label: t("launcher.gameLogs"), icon: "terminal" },
          { id: "settings", label: t("launcher.instanceSettings"), icon: "sliders" },
        ]}
      />

      <div role="tabpanel" className="launcher-panel">
        {tab === "content" ? <InstanceContent profile={profile} onBrowse={(kind) => { if (kind !== "modpack") setAddKind(kind); setTab("add"); }} /> : null}
        {tab === "add" ? <Browse key={addKind} profile={profile} initialKind={addKind} /> : null}
        {tab === "log" ? <InstanceLog profile={profile} /> : null}
        {tab === "settings" ? <InstanceSettingsPanel profile={profile} busy={busy} onRemoved={onRemoved} /> : null}
      </div>

      {canLaunch ? (
        <div className="launcher-actionbar">
          <InstanceTile profile={profile} size={32} />
          <span className="launcher-actionbar-name">{profile.name}</span>
          <PlayControls profile={profile} />
        </div>
      ) : null}
    </article>
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
    onSuccess: () => client.invalidateQueries({ queryKey: launcherKeys.profiles }),
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

/** Play with the primary account (the demo without one), or stop / return.
 *  `compact` is the card's version: a round icon button on a phone. */
export function PlayControls({ profile, compact = false }: { profile: LauncherProfile; compact?: boolean }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const { primary } = usePrimaryAccount();
  const states = useLauncherStatus();
  const caps = useLauncherCapabilities();
  const state = states.data?.[profile.id];
  const installing = state?.phase === "installing";
  const running = state?.phase === "running" || state?.phase === "stopping";
  const anyInstalling = Object.values(states.data ?? {}).some((s) => s.phase === "installing");
  const refresh = () => client.invalidateQueries({ queryKey: launcherKeys.status });
  const start = useMutation({ mutationFn: () => launcherApi.start(profile.id, primary ? "account" : "demo", primary?.id), onSuccess: refresh });
  const stop = useMutation({ mutationFn: () => launcherApi.stop(profile.id), onSuccess: refresh });
  const resume = useMutation({ mutationFn: () => launcherApi.resume(profile.id) });
  const size = compact ? "sm" : undefined;
  const error = start.error || stop.error || resume.error;

  return (
    <div className="launcher-play-controls" data-compact={compact ? "" : undefined}>
      {running && state?.phase === "running" && caps.data?.platform === "android" ? (
        <Button size={size} variant="primary" icon="play" disabled={resume.isPending} onClick={() => resume.mutate()}>{t("launcher.returnToGame")}</Button>
      ) : null}
      {running || installing ? (
        <Button size={size} icon="stop" disabled={stop.isPending} onClick={() => stop.mutate()}>{t(installing ? "launcher.cancel" : "launcher.stopGame")}</Button>
      ) : (
        <Button size={size} variant="primary" icon="play" className="launcher-play" disabled={start.isPending || anyInstalling} onClick={() => start.mutate()}>
          {t(primary ? "launcher.play" : "launcher.playDemo")}
        </Button>
      )}
      {error && !compact ? <ErrorNotice error={toAppError(error)} compact /> : null}
    </div>
  );
}

function Progress({ profile }: { profile: LauncherProfile }) {
  const { t } = useI18n();
  const states = useLauncherStatus();
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
