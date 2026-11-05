/**
 * The sidebar while a launcher instance is open, built like an open server's:
 * the way back to the library, the instance's Play / Close control, and its
 * panels. Its name, picture and state stay on the page, said once.
 */

import { useQuery } from "@tanstack/react-query";

import { NavItem, RailAlert, RailBack, RailKey } from "@/app/RailItems";
import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, ErrorNotice } from "@/ui";
import { INSTANCE_PANELS, instancePath } from "./panels";
import { usePlayState } from "./play";
import type { LauncherProfile } from "./types";

export function LauncherInstanceNav({
  profileId,
  collapsed,
  onExpand,
}: {
  profileId: string;
  collapsed: boolean;
  onExpand: () => void;
}) {
  const { t } = useI18n();
  const profiles = useQuery({ queryKey: ["launcher", "profiles"], queryFn: launcherApi.profiles });
  const profile = profiles.data?.find((candidate) => candidate.id === profileId);
  const back = t("launcher.yourInstances");

  return (
    <div className="flex flex-col gap-3">
      <RailBack to="/launcher" label={back} />

      {profile ? <InstanceControl profile={profile} collapsed={collapsed} onExpand={onExpand} /> : null}

      <div className="rail-rule" aria-hidden="true" />

      <div className="flex flex-col gap-0.5">
        <p className="label rail-group">{t("launcher.instance")}</p>
        {INSTANCE_PANELS.map((panel) => (
          <NavItem key={panel.id} to={instancePath(profileId, panel.id)} icon={panel.icon} label={t(panel.labelKey)} />
        ))}
      </div>
    </div>
  );
}

/** Play, or Close / Return while the game runs; Cancel while it installs. */
function InstanceControl({
  profile,
  collapsed,
  onExpand,
}: {
  profile: LauncherProfile;
  collapsed: boolean;
  onExpand: () => void;
}) {
  const { t, describeError } = useI18n();
  const play = usePlayState(profile);
  if (!play.canLaunch) return null;

  const playLabel = t(play.withAccount ? "launcher.play" : "launcher.playDemo");
  const stopLabel = t(play.installing ? "launcher.cancel" : "launcher.stopGame");
  const returnLabel = t("launcher.returnToGame");
  const startDisabled = play.start.isPending || play.anyInstalling;

  if (collapsed) {
    const failure = play.error ? describeError(toAppError(play.error)).title : "";
    return (
      <section className="rail-control" data-collapsed="" aria-label={profile.name}>
        {play.canReturn ? (
          <RailKey icon="play" label={returnLabel} primary disabled={play.resume.isPending} onClick={() => play.resume.mutate()} />
        ) : null}
        {play.running || play.installing ? (
          <RailKey icon="stop" label={stopLabel} disabled={play.stop.isPending} onClick={() => play.stop.mutate()} />
        ) : (
          <RailKey icon="play" label={playLabel} primary disabled={startDisabled} onClick={() => play.start.mutate()} />
        )}
        {failure ? <RailAlert label={failure} onClick={onExpand} /> : null}
      </section>
    );
  }

  return (
    <section className="rail-control" aria-label={profile.name}>
      <div className="flex flex-col gap-2">
        {play.canReturn ? (
          <Button size="sm" variant="primary" icon="play" disabled={play.resume.isPending} onClick={() => play.resume.mutate()}>
            {returnLabel}
          </Button>
        ) : null}
        {play.running || play.installing ? (
          <Button size="sm" icon="stop" disabled={play.stop.isPending} onClick={() => play.stop.mutate()}>
            {stopLabel}
          </Button>
        ) : (
          <Button size="sm" variant="primary" icon="play" disabled={startDisabled} onClick={() => play.start.mutate()}>
            {playLabel}
          </Button>
        )}
      </div>
      {play.error ? <ErrorNotice error={toAppError(play.error)} compact /> : null}
    </section>
  );
}
