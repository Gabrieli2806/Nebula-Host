/**
 * The Minecraft launcher: your instances (each opens like a server), modpacks
 * to start a new one from, screenshots and your skin. The account is in the
 * sidebar, or top right on a phone.
 *
 * The section and an open instance's panel live in the URL (see `panels.ts`),
 * so the sidebar, the phone tab bar and the back button work as they do for
 * servers.
 */

import { useEffect } from "react";
import { Navigate, useLocation, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { useDeviceBuild } from "@/lib/deviceBuild";
import { useLauncherEnabled } from "@/lib/launcherPref";
import { IS_WEB } from "@/lib/web";
import { Button, EmptyState, ErrorNotice, Segmented } from "@/ui";
import { AccountMenu, SkinTab } from "./Account";
import { Browse } from "./Browse";
import { InstancePage } from "./InstancePage";
import { Library } from "./Library";
import { LAUNCHER_SECTIONS, instancePath, isInstancePanel, isSection, sectionPath, type LauncherSection } from "./panels";
import { usePrimaryAccount } from "./primaryAccount";
import { Screenshots } from "./Screenshots";
import "./launcher.css";

export function LauncherScreen() {
  const { section, profileId, panel } = useParams();
  const [search] = useSearchParams();
  const enabled = useLauncherEnabled();
  const { t } = useI18n();
  usePrefetchSkin();

  // Links from before instances had their own address.
  const legacy = search.get("instance");
  if (legacy) return <Navigate to={instancePath(legacy)} replace />;

  // A home-screen shortcut to an instance still opens it; the rest follows
  // the setting.
  if (!enabled && !profileId) return <Navigate to="/servers" replace />;

  if (IS_WEB) {
    return (
      <div className="launcher">
        <h1 className="display servers-node">Launcher</h1>
        <p className="launcher-muted">{t("launcher.localOnly")}</p>
      </div>
    );
  }

  if (profileId) {
    if (!isInstancePanel(panel)) return <Navigate to={instancePath(profileId)} replace />;
    return <InstanceRoute profileId={profileId} panel={panel} />;
  }
  if (section !== undefined && !isSection(section)) return <Navigate to="/launcher" replace />;
  return <LauncherHome section={section ?? "library"} />;
}

/** The sections this build has: no Modpacks in the Google Play build. */
function useSections() {
  const build = useDeviceBuild();
  return LAUNCHER_SECTIONS.filter((item) => item.id !== "discover" || build.contentKinds.includes("modpack"));
}

/** Fetch the skin early so the Skin section opens with it ready. */
function usePrefetchSkin() {
  const client = useQueryClient();
  const { primary } = usePrimaryAccount();
  useEffect(() => {
    if (primary) {
      void client.prefetchQuery({
        queryKey: ["launcher", "skin", primary.id],
        queryFn: () => launcherApi.skin(primary.id),
        staleTime: 5 * 60_000,
      });
    }
  }, [primary, client]);
}

/** The launcher's front: a header, the section switch, the section. */
function LauncherHome({ section }: { section: LauncherSection }) {
  const { t } = useI18n();
  const navigate = useNavigate();
  const sections = useSections();
  const open = (profileId: string) => navigate(instancePath(profileId));

  return (
    <div className="launcher">
      <header className="launcher-head">
        <div className="launcher-head-row">
          <div className="launcher-head-title">
            <h1 className="display servers-node">Launcher</h1>
            <p className="launcher-muted">{t("launcher.subtitle")}</p>
          </div>
          <div className="launcher-account-top">
            <AccountMenu />
          </div>
        </div>
        <Segmented<LauncherSection>
          label="Launcher"
          className="launcher-sections"
          value={section}
          onChange={(next) => navigate(sectionPath(next))}
          options={sections.map((item) => ({ id: item.id, label: t(item.labelKey), icon: item.icon }))}
        />
      </header>
      {sections.some((item) => item.id === section) ? null : <Navigate to="/launcher" replace />}
      {section === "library" ? <Library onOpen={(profile) => open(profile.id)} /> : null}
      {section === "discover" ? <Browse modpacks onCreated={(profile) => open(profile.id)} /> : null}
      {section === "screenshots" ? <Screenshots /> : null}
      {section === "skin" ? <SkinTab /> : null}
    </div>
  );
}

/** One instance, found by the id in the URL. */
function InstanceRoute({ profileId, panel }: { profileId: string; panel: Parameters<typeof InstancePage>[0]["panel"] }) {
  const { t } = useI18n();
  const navigate = useNavigate();
  const location = useLocation();
  const profiles = useQuery({ queryKey: ["launcher", "profiles"], queryFn: launcherApi.profiles });
  const profile = profiles.data?.find((candidate) => candidate.id === profileId);
  // A home-screen shortcut that could not start the game says why here.
  const shortcutError = (location.state as { shortcutError?: unknown } | null)?.shortcutError;

  if (profiles.isPending) {
    return (
      <div className="launcher">
        <p role="status" className="launcher-muted">{t("launcher.loading")}</p>
      </div>
    );
  }

  if (!profile) {
    return (
      <div className="launcher">
        {profiles.error ? <ErrorNotice error={toAppError(profiles.error)} /> : null}
        <div className="plate">
          <EmptyState
            icon="play"
            title={t("launcher.instanceMissing")}
            hint={t("launcher.instanceMissingHint")}
            action={<Button onClick={() => navigate("/launcher")}>{t("launcher.yourInstances")}</Button>}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="launcher">
      {shortcutError ? <ErrorNotice error={toAppError(shortcutError)} /> : null}
      <InstancePage key={profile.id} profile={profile} panel={panel} />
    </div>
  );
}
