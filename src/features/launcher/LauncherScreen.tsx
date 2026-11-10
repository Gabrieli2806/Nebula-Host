/**
 * The Minecraft launcher: your instances (each opens like a server), modpacks
 * to start a new one from, screenshots and your skin.
 *
 * One layout for every screen size, driven by the launcher's own width
 * (container queries in launcher.css), not the device:
 * - wide (desktop, an unfolded foldable): the instance list stays beside the
 *   open instance, so switching is one tap and there is no "back";
 * - narrow (phones, a foldable's cover screen): one pane at a time, with Play
 *   pinned to the bottom of the screen within thumb reach;
 * - a dual-screen device puts the list and the instance on either side of the
 *   hinge.
 *
 * The section, the open instance and its panel live in the URL's query
 * (panels.ts), so the back button walks back to the library.
 */

import { useEffect } from "react";
import { Navigate, useLocation, useNavigate, useSearchParams } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { useDeviceBuild } from "@/lib/deviceBuild";
import { launcherApi, toAppError } from "@/lib/ipc";
import { useLauncherEnabled } from "@/lib/launcherPref";
import { IS_WEB } from "@/lib/web";
import { ErrorNotice, Tabs } from "@/ui";
import { AccountMenu, SkinTab } from "./Account";
import { Browse } from "./Browse";
import { useLauncherProfiles } from "./hooks";
import { InstancePage } from "./InstancePage";
import { InstanceRail, Library } from "./Library";
import { LAUNCHER_SECTIONS, instanceUrl, isSection, sectionUrl, type LauncherSection } from "./panels";
import { usePrimaryAccount } from "./primaryAccount";
import { Screenshots } from "./Screenshots";
import "./launcher.css";

export function LauncherScreen() {
  const [search] = useSearchParams();
  // A home-screen shortcut still opens it; otherwise it follows the setting.
  const enabled = useLauncherEnabled();
  if (!enabled && !search.get("instance")) return <Navigate to="/servers" replace />;
  return <LauncherContent />;
}

function LauncherContent() {
  const { t } = useI18n();
  const [search] = useSearchParams();
  const location = useLocation();
  const navigate = useNavigate();
  const build = useDeviceBuild();
  // No Modpacks section in the Google Play build (src-tauri/src/flavor.rs).
  const sections = LAUNCHER_SECTIONS.filter((item) => item.id !== "discover" || build.contentKinds.includes("modpack"));
  const asked = search.get("tab");
  const tab: LauncherSection = isSection(asked) && sections.some((item) => item.id === asked) ? asked : "library";
  const openId = search.get("instance");
  // Each step is a new history entry, like a server's panels.
  const setTab = (next: LauncherSection) => navigate(sectionUrl(next));
  const setOpenId = (id: string | null) => navigate(id ? instanceUrl(id) : sectionUrl(tab));
  const profiles = useLauncherProfiles();
  const open = profiles.data?.find((profile) => profile.id === openId) ?? null;
  // Fetch the skin early so the Skin tab opens with it ready.
  const client = useQueryClient();
  const { primary } = usePrimaryAccount();
  useEffect(() => {
    if (primary) void client.prefetchQuery({ queryKey: ["launcher", "skin", primary.id], queryFn: () => launcherApi.skin(primary.id), staleTime: 5 * 60_000 });
  }, [primary, client]);
  if (IS_WEB) return <div className="launcher"><h1 className="display servers-node">Launcher</h1><p className="launcher-muted">{t("launcher.localOnly")}</p></div>;

  const shortcutError = location.state?.shortcutError;
  return (
    <div className="launcher" data-open={open ? "" : undefined}>
      <header className="launcher-head">
        <div className="launcher-head-title">
          <h1 className="display servers-node">Launcher</h1>
          <p className="launcher-muted launcher-subtitle">{t("launcher.subtitle")}</p>
        </div>
        <div className="launcher-account-top"><AccountMenu /></div>
      </header>
      {shortcutError ? <ErrorNotice error={toAppError(shortcutError)} /> : null}
      {open ? (
        <div className="launcher-split">
          <InstanceRail current={open.id} onOpen={(profile) => setOpenId(profile.id)} />
          <InstancePage key={open.id} profile={open} onBack={() => setOpenId(null)} onRemoved={() => navigate("/launcher", { replace: true })} />
        </div>
      ) : (
        <>
          <Tabs<LauncherSection>
            label="Launcher"
            className="launcher-tabs"
            active={tab}
            onChange={setTab}
            tabs={sections.map((item) => ({ id: item.id, label: t(item.labelKey), icon: item.icon }))}
          />
          <div role="tabpanel" className="launcher-panel">
            {tab === "library" ? (
              <Library onOpen={(profile) => setOpenId(profile.id)} />
            ) : tab === "discover" ? (
              <Browse modpacks onCreated={(profile) => setOpenId(profile.id)} />
            ) : tab === "screenshots" ? (
              <Screenshots />
            ) : (
              <SkinTab />
            )}
          </div>
        </>
      )}
    </div>
  );
}
