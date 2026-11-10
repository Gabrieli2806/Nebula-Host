/**
 * Phone navigation: a tab bar along the bottom, where a thumb reaches,
 * instead of the sidebar. Used whenever the window is phone-width — the
 * Android app, the web panel on a phone, or a desktop window made narrow.
 *
 * Outside a server: Servers, Launcher (when on), Machines (once there is
 * another machine) and Settings. Inside a server: back, its four everyday
 * panels, and "More" for the rest, as a sheet.
 */

import { useState } from "react";
import { NavLink, useLocation, useNavigate } from "react-router-dom";

import { useI18n } from "@/i18n";
import { useLauncherEnabled } from "@/lib/launcherPref";
import { remoteNodes } from "@/lib/connection";
import { Icon, type IconName } from "@/ui";

import { SERVER_PANELS } from "./panels";
import "./MobileTabs.css";

/** Panels that get their own tab; the rest go under "More". */
const PRIMARY = ["overview", "console", "players", "files"];

export function MobileTabs({ instanceId }: { instanceId: string | null }) {
  const { t } = useI18n();
  const launcher = useLauncherEnabled();

  if (instanceId) return <ServerTabs instanceId={instanceId} />;

  return (
    <nav className="mobile-tabs" aria-label={t("nav.sidebar")}>
      <Tab to="/servers" icon="servers" label={t("nav.servers")} />
      {launcher ? <Tab to="/launcher" icon="play" label="Launcher" /> : null}
      {remoteNodes().length ? <Tab to="/machines" icon="activity" label={t("fleet.tabMachines")} /> : null}
      <Tab to="/settings" icon="sliders" label={t("nav.settings")} />
    </nav>
  );
}

function ServerTabs({ instanceId }: { instanceId: string }) {
  const { t } = useI18n();
  const { pathname } = useLocation();
  const [more, setMore] = useState(false);
  const base = `/servers/${instanceId}`;
  const extra = SERVER_PANELS.filter((panel) => !PRIMARY.includes(panel.id));

  return (
    <>
      <nav className="mobile-tabs" aria-label={t("nav.sidebar")}>
        <BackTab to="/servers" label={t("phone.back")} />
        {SERVER_PANELS.filter((panel) => PRIMARY.includes(panel.id)).map((panel) => (
          <Tab key={panel.id} to={`${base}/${panel.id}`} icon={panel.icon} label={t(panel.labelKey)} />
        ))}
        <button
          type="button"
          className="mobile-tab"
          aria-haspopup="menu"
          aria-expanded={more}
          data-active={extra.some((panel) => pathname === `${base}/${panel.id}`) ? "" : undefined}
          onClick={() => setMore(true)}
        >
          <Icon name="more" size={20} />
          <span>{t("phone.more")}</span>
        </button>
      </nav>

      {more ? (
        <div className="mobile-sheet-scrim" onClick={() => setMore(false)}>
          <div className="mobile-sheet" role="menu" onClick={(event) => event.stopPropagation()}>
            <span className="mobile-sheet-grip" aria-hidden="true" />
            {extra.map((panel) => (
              <NavLink
                key={panel.id}
                to={`${base}/${panel.id}`}
                role="menuitem"
                className="mobile-sheet-item"
                onClick={() => setMore(false)}
              >
                <Icon name={panel.icon} size={18} />
                <span>{t(panel.labelKey)}</span>
              </NavLink>
            ))}
          </div>
        </div>
      ) : null}
    </>
  );
}

/** Back to the server list. */
function BackTab({ to, label }: { to: string; label: string }) {
  const navigate = useNavigate();
  return (
    <button type="button" className="mobile-tab" onClick={() => navigate(to)}>
      <Icon name="chevron-left" size={20} />
      <span>{label}</span>
    </button>
  );
}

function Tab({ to, icon, label }: { to: string; icon: IconName; label: string }) {
  return (
    <NavLink to={to} className="mobile-tab" data-tab="">
      <Icon name={icon} size={20} />
      <span>{label}</span>
    </NavLink>
  );
}
