/**
 * The instances, as cards like the server list: picture, name, state, tags
 * and Play right on the card. Tapping a card opens the instance.
 *
 * The cards reflow by the space they have (launcher.css): a grid on wide
 * screens, compact rows with a round Play button on a phone. `InstanceRail`
 * is the same list, condensed, kept beside an open instance when there is
 * room for both.
 */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useContextMenu, type MenuItem } from "@/ui/ContextMenu";
import { usePrimaryAccount } from "./primaryAccount";

import { useI18n } from "@/i18n";
import { formatMegabytes } from "@/lib/format";
import { useManualOrder } from "@/lib/manualOrder";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, EmptyState, ErrorNotice, Icon, Panel, Select, StatusChip, StatusDot, TextInput } from "@/ui";
import { CreateInstanceDialog } from "./CreateInstanceDialog";
import { isBusy, launcherKeys, phaseDot, phaseLabel, useLauncherCapabilities, useLauncherProfiles, useLauncherStatus } from "./hooks";
import { InstanceTile } from "./InstanceSettings";
import { PlayControls } from "./InstancePage";
import { LOADERS, loaderInfo } from "./loaderOptions";
import type { LauncherProfile, Loader } from "./types";

const ORDER_KEY = "nebula.instancesOrder";

export function Library({ onOpen }: { onOpen: (profile: LauncherProfile) => void }) {
  const { t } = useI18n();
  const profiles = useLauncherProfiles();
  const caps = useLauncherCapabilities();
  const statuses = useLauncherStatus();
  const [creating, setCreating] = useState(false);
  const [search, setSearch] = useState("");
  const [loader, setLoader] = useState<Loader | "all">("all");
  const manual = useManualOrder(ORDER_KEY);
  const arranged = manual.arrange(profiles.data ?? [], (profile) => profile.id);
  const arrangedIds = arranged.map((profile) => profile.id);
  const visible = arranged.filter(
    (profile) => (loader === "all" || profile.loader === loader) && `${profile.name} ${profile.game_version}`.toLowerCase().includes(search.toLowerCase()),
  );
  const client = useQueryClient();
  const { primary } = usePrimaryAccount();
  const { open, menu } = useContextMenu();
  const android = caps.data?.platform === "android";
  const action = useMutation({
    mutationFn: async ({ profile, what }: { profile: LauncherProfile; what: "play" | "stop" | "folder" | "shortcut" }) => {
      if (what === "play") return launcherApi.start(profile.id, primary ? "account" : "demo", primary?.id);
      if (what === "stop") return launcherApi.stop(profile.id);
      if (what === "folder") return launcherApi.openFolder(profile.id);
      return launcherApi.pinShortcut(profile.id, primary ? "account" : "demo", primary?.id);
    },
    onSuccess: () => client.invalidateQueries({ queryKey: launcherKeys.status }),
  });
  const menuFor = (profile: LauncherProfile): MenuItem[] => [
    isBusy(statuses.data?.[profile.id]?.phase)
      ? { label: t("launcher.stopGame"), icon: "stop", onSelect: () => action.mutate({ profile, what: "stop" }) }
      : { label: t("launcher.play"), icon: "play", disabled: !caps.data?.can_launch, onSelect: () => action.mutate({ profile, what: "play" }) },
    { label: t("launcher.manage"), icon: "sliders", onSelect: () => onOpen(profile) },
    { kind: "separator" },
    { label: t("menu.moveToStart"), icon: "arrow-up", onSelect: () => manual.moveTo(arrangedIds, profile.id, "start") },
    { label: t("menu.moveToEnd"), icon: "arrow-down", onSelect: () => manual.moveTo(arrangedIds, profile.id, "end") },
    { kind: "separator" },
    { label: t("launcher.openInstanceFolder"), icon: "folder", onSelect: () => action.mutate({ profile, what: "folder" }) },
    { label: t(android ? "launcher.pinShortcut" : "launcher.desktopShortcut"), icon: "plus", onSelect: () => action.mutate({ profile, what: "shortcut" }) },
  ];
  const failure = profiles.error || caps.error || statuses.error || action.error;

  return (
    <>
      {profiles.data?.length ? (
        <div className="launcher-toolbar">
          <label className="launcher-search">
            <Icon name="search" size={14} />
            <TextInput type="search" aria-label={t("launcher.searchInstances")} placeholder={t("launcher.searchInstances")} value={search} onChange={(e) => setSearch(e.target.value)} />
          </label>
          <Select className="launcher-filter" aria-label={t("launcher.filterLoader")} value={loader} onChange={(e) => setLoader(e.target.value as Loader | "all")}>
            <option value="all">{t("launcher.allLoaders")}</option>
            {LOADERS.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
          </Select>
          <Button variant="primary" icon="plus" className="launcher-new" onClick={() => setCreating(true)}>{t("launcher.newInstance")}</Button>
        </div>
      ) : null}
      {failure ? <ErrorNotice error={toAppError(failure)} /> : null}
      {profiles.isPending ? <p role="status" className="launcher-muted">{t("launcher.loading")}</p> : null}

      {profiles.data?.length === 0 ? (
        <Panel>
          <EmptyState
            icon="play"
            title={t("launcher.emptyTitle")}
            hint={t("launcher.emptyDescription")}
            action={<Button variant="primary" icon="plus" onClick={() => setCreating(true)}>{t("launcher.createFirstInstance")}</Button>}
          />
        </Panel>
      ) : null}
      {profiles.data?.length && visible.length === 0 ? (
        <Panel>
          <EmptyState
            icon="search"
            title={t("launcher.noMatchingInstances")}
            action={<Button onClick={() => { setSearch(""); setLoader("all"); }}>{t("launcher.clearFilters")}</Button>}
          />
        </Panel>
      ) : null}

      {visible.length ? (
        <ul className="launcher-grid" aria-label={t("launcher.yourInstances")}>
          {visible.map((profile) => {
            const phase = statuses.data?.[profile.id]?.phase;
            const canLaunch = !!caps.data?.can_launch && profile.loader !== "quilt";
            return (
              <li key={profile.id} {...manual.dragProps(arrangedIds, profile.id)} className="panel card-interactive launcher-card" data-phase={phase} onContextMenu={(event) => open(event, menuFor(profile))}>
                <button type="button" className="launcher-card-open" onClick={() => onOpen(profile)} title={profile.name}>
                  <InstanceTile profile={profile} size={48} />
                  <span className="launcher-card-body">
                    <span className="launcher-card-name">{profile.name}</span>
                    <span className="launcher-card-tags">
                      <span className="tag">{loaderInfo(profile.loader).name}</span>
                      <span className="tag">{profile.game_version}</span>
                      <span className="tag launcher-card-memory">{formatMegabytes(profile.memory_mb)}</span>
                    </span>
                  </span>
                </button>
                <div className="launcher-card-foot">
                  <StatusChip status={phaseDot(phase)} label={t(phaseLabel(phase))} className="launcher-card-status" />
                  {canLaunch ? <PlayControls profile={profile} compact /> : null}
                </div>
              </li>
            );
          })}
        </ul>
      ) : null}

      {android ? <p className="launcher-footnote">{t("launcher.androidRuntime")}</p> : null}
      {menu}
      {action.isSuccess && action.variables?.what === "shortcut" ? <p role="status" className="launcher-notice">{t(android ? "launcher.shortcutRequested" : "launcher.desktopShortcutDone")}</p> : null}
      {creating ? <CreateInstanceDialog onClose={() => setCreating(false)} onCreated={(profile) => { setCreating(false); onOpen(profile); }} /> : null}
    </>
  );
}

/** The instances as a slim list beside an open instance (wide screens only;
 *  hidden by launcher.css when the launcher is narrow). */
export function InstanceRail({ current, onOpen }: { current: string; onOpen: (profile: LauncherProfile) => void }) {
  const { t } = useI18n();
  const profiles = useLauncherProfiles();
  const statuses = useLauncherStatus();
  const manual = useManualOrder(ORDER_KEY);
  const arranged = manual.arrange(profiles.data ?? [], (profile) => profile.id);
  return (
    <nav className="launcher-rail" aria-label={t("launcher.yourInstances")}>
      <p className="label launcher-rail-title">{t("launcher.yourInstances")}</p>
      <ul>
        {arranged.map((profile) => {
          const phase = statuses.data?.[profile.id]?.phase;
          return (
            <li key={profile.id}>
              <button type="button" className="launcher-rail-item" aria-current={profile.id === current ? "page" : undefined} onClick={() => onOpen(profile)}>
                <InstanceTile profile={profile} size={32} />
                <span className="launcher-rail-text">
                  <span className="launcher-rail-name">{profile.name}</span>
                  <span className="launcher-muted">{loaderInfo(profile.loader).name} {profile.game_version}</span>
                </span>
                {phase && phase !== "ready" ? <StatusDot status={phaseDot(phase)} label={t(phaseLabel(phase))} /> : null}
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
