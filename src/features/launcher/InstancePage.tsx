/**
 * One instance, laid out like a server. Its panels are in the sidebar (or the
 * phone tab bar), and the page says what the instance is once:
 *
 * - Content (its "overview"): the full header, with its picture, tags, state
 *   and the Play key, then what it has installed.
 * - Every other panel: a light title row, the panel's name with the
 *   instance's name and state small beside it, as on a server's panels.
 */

import { useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useContextMenu } from "@/ui/ContextMenu";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, ErrorNotice, Icon, StatusDot, statusStyle } from "@/ui";
import { Browse } from "./Browse";
import { InstanceContent } from "./Content";
import { InstanceLog } from "./InstanceLog";
import { InstanceSettingsPanel, InstanceTile, squareIcon } from "./InstanceSettings";
import { loaderInfo } from "./loaderOptions";
import { INSTANCE_PANELS, instancePath, type InstancePanel } from "./panels";
import { phaseDot, phaseLabel, useLauncherStatus, usePlayState } from "./play";
import type { ContentKind, LauncherProfile } from "./types";

type AddKind = Exclude<ContentKind, "modpack">;
const ADD_KINDS: readonly AddKind[] = ["mod", "resourcepack", "shader"];

export function InstancePage({ profile, panel }: { profile: LauncherProfile; panel: InstancePanel }) {
  const { t } = useI18n();
  const navigate = useNavigate();
  const [search] = useSearchParams();
  const play = usePlayState(profile);
  const busy = play.running || play.installing;
  const requested = search.get("kind");
  const addKind = ADD_KINDS.find((kind) => kind === requested) ?? "mod";
  const current = INSTANCE_PANELS.find((candidate) => candidate.id === panel) ?? INSTANCE_PANELS[0]!;

  return (
    <div className="launcher-instance-page">
      {panel === "content" ? (
        <>
          <section className="panel launcher-instance-hero">
            <PictureButton profile={profile} />
            <div className="launcher-instance-hero-text">
              <div className="server-card-title">
                <h1 className="launcher-detail-title" title={profile.name}>{profile.name}</h1>
                <StateChip phase={play.phase} />
              </div>
              <div className="server-card-tags">
                <span className="tag">{loaderInfo(profile.loader).name}{profile.loader_version ? ` ${profile.loader_version}` : ""}</span>
                <span className="tag">{profile.game_version}</span>
                <span className="tag">{profile.memory_mb / 1024} GB</span>
              </div>
            </div>
            {play.canLaunch ? <PlayControls profile={profile} /> : null}
          </section>
          {!play.canLaunch ? (
            <p className="launcher-notice">
              <Icon name="info" size={14} />
              {t(profile.loader === "quilt" ? "launcher.loaderPending" : "launcher.androidRuntime")}
            </p>
          ) : null}
          {play.canLaunch ? <Progress profile={profile} /> : null}
        </>
      ) : (
        <header className="server-pagehead">
          <h1 className="server-pagehead-title">{t(current.labelKey)}</h1>
          <p className="server-pagehead-server" title={profile.name}>
            <span className="server-pagehead-name">{profile.name}</span>
            <span className="server-pagehead-status">
              <StatusDot status={phaseDot(play.phase)} size={7} />
              {t(phaseLabel(play.phase))}
            </span>
          </p>
        </header>
      )}

      {panel === "content" ? (
        <InstanceContent
          profile={profile}
          onBrowse={(kind) => navigate(`${instancePath(profile.id, "add")}${kind === "modpack" ? "" : `?kind=${kind}`}`)}
        />
      ) : null}
      {panel === "add" ? <Browse key={addKind} profile={profile} initialKind={addKind} /> : null}
      {panel === "log" ? <InstanceLog profile={profile} /> : null}
      {panel === "settings" ? (
        <InstanceSettingsPanel profile={profile} busy={busy} onRemoved={() => navigate("/launcher", { replace: true })} />
      ) : null}
    </div>
  );
}

/** The instance's state, as the chip a server card wears. */
export function StateChip({ phase }: { phase?: string }) {
  const { t } = useI18n();
  return (
    <span className="status-chip flex-none" style={statusStyle(phaseDot(phase))}>
      <StatusDot status={phaseDot(phase)} />
      {t(phaseLabel(phase))}
    </span>
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
  const play = usePlayState(profile);
  const size = compact ? "sm" : undefined;

  return (
    <div className="launcher-play-controls">
      {play.canReturn ? (
        <Button size={size} variant="primary" icon="play" disabled={play.resume.isPending} onClick={() => play.resume.mutate()}>{t("launcher.returnToGame")}</Button>
      ) : null}
      {play.running || play.installing ? (
        <Button size={size} icon="stop" disabled={play.stop.isPending} onClick={() => play.stop.mutate()}>{t(play.installing ? "launcher.cancel" : "launcher.stopGame")}</Button>
      ) : (
        <Button size={size} variant="primary" icon="play" className={compact ? undefined : "launcher-play"} disabled={play.start.isPending || play.anyInstalling} onClick={() => play.start.mutate()}>
          {t(play.withAccount ? "launcher.play" : "launcher.playDemo")}
        </Button>
      )}
      {play.error ? <ErrorNotice error={toAppError(play.error)} compact /> : null}
    </div>
  );
}

function Progress({ profile }: { profile: LauncherProfile }) {
  const { t } = useI18n();
  const state = useLauncherStatus().data?.[profile.id];
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
