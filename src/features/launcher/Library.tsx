/**
 * The instances, as cards like the server list: picture, name, state, tags
 * and Play right on the card. Tapping a card opens the instance.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, ErrorNotice, Icon, LoaderIcon, Select, StatusDot, statusStyle } from "@/ui";
import { CreateInstanceDialog } from "./CreateInstanceDialog";
import { InstanceTile } from "./InstanceSettings";
import { PlayControls, phaseDot, phaseLabel } from "./InstancePage";
import { LOADERS, loaderInfo } from "./loaderOptions";
import type { LauncherProfile, Loader } from "./types";

export function Library({ onOpen }: { onOpen: (profile: LauncherProfile) => void }) {
  const { t } = useI18n();
  const profiles = useQuery({ queryKey: ["launcher", "profiles"], queryFn: launcherApi.profiles });
  const caps = useQuery({ queryKey: ["launcher", "capabilities"], queryFn: launcherApi.capabilities });
  const statuses = useQuery({ queryKey: ["launcher", "status"], queryFn: launcherApi.status, refetchInterval: 1000 });
  const [creating, setCreating] = useState(false);
  const [search, setSearch] = useState("");
  const [loader, setLoader] = useState<Loader | "all">("all");
  const visible = (profiles.data ?? []).filter(
    (profile) => (loader === "all" || profile.loader === loader) && `${profile.name} ${profile.game_version}`.toLowerCase().includes(search.toLowerCase()),
  );
  const failure = profiles.error || caps.error || statuses.error;

  return (
    <>
      {profiles.data?.length ? (
        <div className="servers-toolbar launcher-toolbar">
          <label className="servers-search">
            <span className="servers-search-icon" aria-hidden="true"><Icon name="search" size={14} /></span>
            <input type="search" className="field" aria-label={t("launcher.searchInstances")} placeholder={t("launcher.searchInstances")} value={search} onChange={(e) => setSearch(e.target.value)} />
          </label>
          <Select className="launcher-filter" aria-label={t("launcher.filterLoader")} value={loader} onChange={(e) => setLoader(e.target.value as Loader | "all")}>
            <option value="all">{t("launcher.allLoaders")}</option>
            {LOADERS.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
          </Select>
          <span className="launcher-toolbar-spacer" />
          <Button variant="primary" icon="plus" onClick={() => setCreating(true)}>{t("launcher.newInstance")}</Button>
        </div>
      ) : null}
      {failure ? <ErrorNotice error={toAppError(failure)} /> : null}
      {profiles.isPending ? <p role="status" className="launcher-muted">{t("launcher.loading")}</p> : null}

      {profiles.data?.length === 0 ? (
        <div className="panel servers-empty"><div className="empty-state">
          <span className="empty-state-icon"><LoaderIcon glyph="cube" size={26} /></span>
          <p className="empty-state-title">{t("launcher.emptyTitle")}</p>
          <p className="empty-state-hint">{t("launcher.emptyDescription")}</p>
          <div className="mt-1"><Button variant="primary" icon="plus" onClick={() => setCreating(true)}>{t("launcher.createFirstInstance")}</Button></div>
        </div></div>
      ) : null}
      {profiles.data?.length && visible.length === 0 ? (
        <div className="panel servers-empty"><div className="empty-state">
          <span className="empty-state-icon"><Icon name="search" size={20} /></span>
          <p className="empty-state-title">{t("launcher.noMatchingInstances")}</p>
          <div className="mt-1"><Button onClick={() => { setSearch(""); setLoader("all"); }}>{t("launcher.clearFilters")}</Button></div>
        </div></div>
      ) : null}

      {visible.length ? (
        <section className="servers-grid launcher-instance-grid" aria-label={t("launcher.yourInstances")}>
          {visible.map((profile) => {
            const phase = statuses.data?.[profile.id]?.phase;
            const canLaunch = !!caps.data?.can_launch && profile.loader !== "quilt";
            return (
              <article key={profile.id} className="panel card-interactive server-card launcher-instance" data-phase={phase}>
                <header className="flex items-start gap-3.5">
                  <InstanceTile profile={profile} size={44} />
                  <div className="flex min-w-0 flex-1 flex-col gap-2">
                    <div className="server-card-title">
                      <h3 className="pt-[1px]">
                        <button type="button" className="server-card-open" title={profile.name} onClick={() => onOpen(profile)}>{profile.name}</button>
                      </h3>
                      <span className="status-chip flex-none" style={statusStyle(phaseDot(phase))}><StatusDot status={phaseDot(phase)} />{t(phaseLabel(phase))}</span>
                    </div>
                    <div className="server-card-tags">
                      <span className="tag">{loaderInfo(profile.loader).name}</span>
                      <span className="tag">{profile.game_version}</span>
                      <span className="tag">{profile.memory_mb / 1024} GB</span>
                    </div>
                  </div>
                </header>
                <footer className="launcher-instance-foot">
                  <Button size="sm" variant="quiet" icon="sliders" onClick={() => onOpen(profile)}>{t("launcher.manage")}</Button>
                  {canLaunch ? <PlayControls profile={profile} compact /> : null}
                </footer>
              </article>
            );
          })}
        </section>
      ) : null}

      {caps.data?.platform === "android" ? <p className="launcher-footnote">{t("launcher.androidRuntime")}</p> : null}
      {creating ? <CreateInstanceDialog onClose={() => setCreating(false)} onCreated={(profile) => { setCreating(false); onOpen(profile); }} /> : null}
    </>
  );
}
