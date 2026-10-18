/**
 * Mods and plugins: what is installed (identified on Modrinth by file hash),
 * and what could be (Modrinth search narrowed to this server's loader and
 * game version). Cards or list, remembered.
 */

import { useState } from "react";

import { useAddonContext } from "@/hooks/useBackend";
import type { InstanceView } from "@/types/api";
import { EmptyState, ErrorNotice, Panel, Segmented, Skeleton, Tabs } from "@/ui";

import { BrowseTab } from "./addons/BrowseTab";
import { InstalledTab } from "./addons/InstalledTab";
import { ProjectDialog, type ProjectTarget } from "./addons/ProjectDialog";
import { primaryLoader, useAddonCopy, useAddonView, type AddonView } from "./addons/util";
import "./AddonsPanel.css";

type Mode = "installed" | "browse";

export function AddonsPanel({ instance }: { instance: InstanceView }) {
  const { t } = useAddonCopy();
  const context = useAddonContext(instance.id);
  const [mode, setMode] = useState<Mode>("installed");
  const [view, setView] = useAddonView();
  const [detail, setDetail] = useState<ProjectTarget | null>(null);

  if (context.error) {
    return <ErrorNotice error={context.error} onRetry={() => void context.refetch()} />;
  }

  if (!context.data) {
    return (
      <div className="flex flex-col gap-3" aria-hidden="true">
        <Skeleton className="h-9 w-64" />
        <Skeleton className="h-40 rounded-[var(--radius-lg)]" />
      </div>
    );
  }

  const ctx = context.data;

  if (!ctx.supported) {
    return (
      <Panel>
        <EmptyState icon="puzzle" title={t("addons.unsupported")} hint={t("addons.unsupportedHint")} />
      </Panel>
    );
  }

  const kind = ctx.project_type === "plugin" ? "plugin" : "mod";

  return (
    <div className="addons flex flex-col gap-4">
      <div className="addons-bar">
        <Tabs<Mode>
          label={t("addons.title")}
          active={mode}
          onChange={setMode}
          tabs={[
            { id: "installed", label: t("addons.installed") },
            { id: "browse", label: t("addons.browse"), icon: "search" },
          ]}
        />
        <p className="addons-target">
          {t(`addons.target_${kind}`, {
            loader: primaryLoader(ctx),
            version: ctx.game_version,
          })}
          {ctx.addon_dir ? <span className="mono">{ctx.addon_dir}/</span> : null}
        </p>
        <Segmented<AddonView>
          label={t("addons.viewLabel")}
          size="sm"
          value={view}
          onChange={setView}
          options={[
            { id: "cards", label: t("servers.densityCards") },
            { id: "list", label: t("servers.densityList") },
          ]}
        />
      </div>

      <div role="tabpanel" aria-label={mode === "installed" ? t("addons.installed") : t("addons.browse")}>
        {mode === "installed" ? (
          <InstalledTab
            instance={instance}
            context={ctx}
            view={view}
            onOpenProject={setDetail}
            onBrowse={() => setMode("browse")}
          />
        ) : (
          <BrowseTab instance={instance} context={ctx} view={view} onOpenProject={setDetail} />
        )}
      </div>

      <ProjectDialog instance={instance} context={ctx} target={detail} onClose={() => setDetail(null)} />
    </div>
  );
}
