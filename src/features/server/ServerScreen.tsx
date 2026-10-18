/**
 * One server, in depth.
 *
 * Its navigation and Start / Stop / Restart live in the sidebar (see
 * `AppShell`); everything that describes the server is said once, here:
 *
 * - Overview: no header from this screen. OverviewPanel renders the server's
 *   own header (name, state, tags) and its resources.
 * - Every other panel: a light title row, the panel's name with the server's
 *   name and state small beside it, for orientation.
 *
 * The root is a flex column that fills `main`, so a panel that wants the rest
 * of the height (the console) can take it with flex-1.
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
import { useInstance } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { Button, EmptyState, StatusDot } from "@/ui";

export function ServerScreen() {
  const { instanceId = "", panel = "overview" } = useParams();
  const navigate = useNavigate();
  const { t } = useI18n();
  const instance = useInstance(instanceId);

  if (!instance) {
    return (
      <div className="server-screen">
        <div className="plate">
          <EmptyState
            icon="servers"
            title={t("errors.instance_not_found.title")}
            hint={t("errors.instance_not_found.cause")}
            action={<Button onClick={() => navigate("/servers")}>{t("actions.back")}</Button>}
          />
        </div>
      </div>
    );
  }

  const current = SERVER_PANELS.find((candidate) => candidate.id === panel) ?? SERVER_PANELS[0]!;
  const status = t(`status.${instance.status}`);

  return (
    <div className="server-screen">
      {current.id === "overview" ? null : (
        <header className="server-pagehead">
          <h1 className="server-pagehead-title">{t(current.labelKey)}</h1>
          <p className="server-pagehead-server" title={instance.name}>
            <span className="server-pagehead-name">{instance.name}</span>
            <span className="server-pagehead-status" data-status={instance.status}>
              <StatusDot status={instance.status} size={7} />
              {status}
            </span>
          </p>
        </header>
      )}

      <div className="server-panel">
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
