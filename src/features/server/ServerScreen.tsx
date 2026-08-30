/**
 * One server, in depth.
 *
 * The navigation for it lives in the sidebar (see `AppShell`), so this is only
 * a thin header plus whichever panel the URL names.
 */

import { useNavigate, useParams } from "react-router-dom";

import { AddonsPanel } from "./panels/AddonsPanel";
import { BackupsPanel } from "./panels/BackupsPanel";
import { ConsolePanel } from "./panels/ConsolePanel";
import { FilesPanel } from "./panels/FilesPanel";
import { OverviewPanel } from "./panels/OverviewPanel";
import { PlayersPanel } from "./panels/PlayersPanel";
import { ServerSettingsPanel } from "./panels/ServerSettingsPanel";
import { SERVER_PANELS } from "@/app/AppShell";
import { useInstance, useInstanceActions } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatDuration } from "@/lib/format";
import { Button, EmptyState, ErrorNotice, StatusChip } from "@/ui";

export function ServerScreen() {
  const { instanceId = "", panel = "overview" } = useParams();
  const navigate = useNavigate();
  const { t } = useI18n();
  const instance = useInstance(instanceId);
  const actions = useInstanceActions();

  if (!instance) {
    return (
      <div className="p-6">
        <EmptyState
          icon="servers"
          title={t("errors.instance_not_found.title")}
          hint={t("errors.instance_not_found.cause")}
          action={<Button onClick={() => navigate("/servers")}>{t("actions.back")}</Button>}
        />
      </div>
    );
  }

  const current = SERVER_PANELS.find((candidate) => candidate.id === panel) ?? SERVER_PANELS[0]!;
  const failure = actions.start.error ?? actions.stop.error ?? actions.restart.error;

  return (
    <div className="flex h-full flex-col">
      <header className="flex flex-none items-center justify-between gap-4 border-b border-[color:var(--line)] px-6 py-4">
        <div className="min-w-0">
          <h1 className="truncate text-[length:var(--text-xl)] font-semibold">
            {t(current.labelKey)}
          </h1>
          <p className="mono truncate text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
            {instance.name} · {instance.build.distribution} {instance.build.version}
            {instance.build.loader_version ? ` · ${instance.build.loader_version}` : ""}
            {instance.uptime_seconds ? ` · ${formatDuration(instance.uptime_seconds)}` : ""}
            {instance.tick_rate ? ` · ${instance.tick_rate.toFixed(1)} TPS` : ""}
          </p>
        </div>
        <StatusChip status={instance.status} />
      </header>

      <div className="min-h-0 flex-1 overflow-auto p-6">
        {failure ? (
          <div className="mb-4">
            <ErrorNotice error={failure} />
          </div>
        ) : null}

        {current.id === "overview" ? <OverviewPanel instance={instance} /> : null}
        {current.id === "console" ? <ConsolePanel instance={instance} /> : null}
        {current.id === "files" ? <FilesPanel instance={instance} /> : null}
        {current.id === "backups" ? <BackupsPanel instance={instance} /> : null}
        {current.id === "players" ? <PlayersPanel instance={instance} /> : null}
        {current.id === "addons" ? <AddonsPanel instance={instance} /> : null}
        {current.id === "settings" ? <ServerSettingsPanel instance={instance} /> : null}
      </div>
    </div>
  );
}
