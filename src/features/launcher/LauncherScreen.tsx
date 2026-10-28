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
 */

import { useEffect, useState } from "react";
import { Navigate, useLocation, useSearchParams } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { useLauncherEnabled } from "@/lib/launcherPref";
import { IS_WEB } from "@/lib/web";
import { ErrorNotice, Tabs } from "@/ui";
import { AccountMenu, SkinTab } from "./Account";
import { Browse } from "./Browse";
import { useLauncherProfiles } from "./hooks";
import { InstancePage } from "./InstancePage";
import { InstanceRail, Library } from "./Library";
import { usePrimaryAccount } from "./primaryAccount";
import { Screenshots } from "./Screenshots";
import "./launcher.css";

type Tab = "library" | "discover" | "screenshots" | "skin";

export function LauncherScreen() {
  const [search] = useSearchParams();
  // A home-screen shortcut still opens it; otherwise it follows the setting.
  const enabled = useLauncherEnabled();
  if (!enabled && !search.get("instance")) return <Navigate to="/servers" replace />;
  return <LauncherContent key={search.get("request") ?? "launcher"} />;
}

function LauncherContent() {
  const { t } = useI18n();
  const [search] = useSearchParams();
  const location = useLocation();
  const [tab, setTab] = useState<Tab>("library");
  const [openId, setOpenId] = useState<string | null>(search.get("instance"));
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
          <InstancePage key={open.id} profile={open} onBack={() => setOpenId(null)} onRemoved={() => setOpenId(null)} />
        </div>
      ) : (
        <>
          <Tabs<Tab>
            label="Launcher"
            className="launcher-tabs"
            active={tab}
            onChange={setTab}
            tabs={[
              { id: "library", label: t("launcher.library"), icon: "servers" },
              { id: "discover", label: t("launcher.modpacks"), icon: "search" },
              { id: "screenshots", label: t("launcher.screenshots"), icon: "image" },
              { id: "skin", label: t("launcher.skin"), icon: "users" },
            ]}
          />
          <div role="tabpanel" className="launcher-panel">
            {tab === "library" ? (
              <Library onOpen={(profile) => setOpenId(profile.id)} />
            ) : tab === "discover" ? (
              <Browse modpacks onCreated={(profile) => { setTab("library"); setOpenId(profile.id); }} />
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
