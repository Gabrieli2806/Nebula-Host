/**
 * The Minecraft launcher: your instances (each opens like a server), modpacks
 * to start a new one from, and your skin. The account is in the sidebar, or
 * top right on a phone.
 */

import { useState } from "react";
import { Navigate, useLocation, useSearchParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { useLauncherEnabled } from "@/lib/launcherPref";
import { IS_WEB } from "@/lib/web";
import { ErrorNotice, Segmented } from "@/ui";
import { AccountMenu, SkinTab } from "./Account";
import { Browse } from "./Browse";
import { InstancePage } from "./InstancePage";
import { Library } from "./Library";
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
  const profiles = useQuery({ queryKey: ["launcher", "profiles"], queryFn: launcherApi.profiles });
  const open = profiles.data?.find((profile) => profile.id === openId) ?? null;
  if (IS_WEB) return <div className="launcher"><h1 className="display servers-node">Launcher</h1><p className="launcher-muted">{t("launcher.localOnly")}</p></div>;

  const shortcutError = location.state?.shortcutError;
  return (
    <div className="launcher">
      <header className="launcher-head">
        <div className="launcher-head-row">
          <div className="launcher-head-title">
            <h1 className="display servers-node">Launcher</h1>
            <p className="launcher-muted">{t("launcher.subtitle")}</p>
          </div>
          <div className="launcher-account-top"><AccountMenu /></div>
        </div>
        {open ? null : (
          <Segmented<Tab>
            label="Launcher"
            value={tab}
            onChange={setTab}
            options={[
              { id: "library", label: t("launcher.library"), icon: "servers" },
              { id: "discover", label: t("launcher.modpacks"), icon: "search" },
              { id: "screenshots", label: t("launcher.screenshots"), icon: "image" },
              { id: "skin", label: t("launcher.skin"), icon: "users" },
            ]}
          />
        )}
      </header>
      {shortcutError ? <ErrorNotice error={toAppError(shortcutError)} /> : null}
      {open ? (
        <InstancePage key={open.id} profile={open} onBack={() => setOpenId(null)} onRemoved={() => setOpenId(null)} />
      ) : tab === "library" ? (
        <Library onOpen={(profile) => setOpenId(profile.id)} />
      ) : tab === "discover" ? (
        <Browse modpacks onCreated={(profile) => { setTab("library"); setOpenId(profile.id); }} />
      ) : tab === "screenshots" ? (
        <Screenshots />
      ) : (
        <SkinTab />
      )}
    </div>
  );
}
