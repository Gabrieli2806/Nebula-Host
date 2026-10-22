/**
 * The frame every screen sits in: the title bar, a floating sidebar card on
 * the left and the main column to its right.
 *
 * The sidebar shows the app's navigation and the machine block on the server
 * list, and once a server is open: a way back, that server's Start / Stop /
 * Restart control and its panels. Everything that describes the server (name,
 * state, numbers) lives in the page, never here, so nothing is said twice.
 *
 * The sidebar collapses to an icon rail (title bar toggle or Ctrl+B). The
 * choice persists in localStorage; collapsed items keep their names for
 * assistive tech and show them as a tooltip on hover or keyboard focus.
 */

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type FocusEvent,
  type PointerEvent,
} from "react";
import { createPortal } from "react-dom";
import { NavLink, Outlet, useLocation, useMatch, useNavigate } from "react-router-dom";
import { useQueryClient } from "@tanstack/react-query";

import {
  useBackendEvents,
  useInstance,
  useInstanceActions,
  useInstances,
  useNode,
  useNodeHealth,
} from "@/hooks/useBackend";
import { useRemoteLink } from "@/hooks/useRemoteLink";
import { useI18n } from "@/i18n";
import { switchTo } from "@/lib/connection";
import { formatMegabytes } from "@/lib/format";
import { toAppError } from "@/lib/ipc";
import { useLauncherEnabled } from "@/lib/launcherPref";
import type { InstanceView } from "@/types/api";
import { Button, ErrorNotice, Icon, Ring, StatusDot, type IconName } from "@/ui";
import { useTransferTracking } from "@/features/server/transfer/active";
import { Tour } from "@/features/onboarding/Tour";
import { AccountMenu } from "@/features/launcher/Account";
import { MobileTabs } from "./MobileTabs";
import { TitleBar } from "./TitleBar";

/** The panels a server has, in the order they are worked through. */
export const SERVER_PANELS: {
  id: string;
  icon: IconName;
  labelKey: string;
  group: "operate" | "manage";
}[] = [
  { id: "overview", icon: "servers", labelKey: "overview.title", group: "operate" },
  { id: "console", icon: "terminal", labelKey: "console.title", group: "operate" },
  { id: "files", icon: "folder", labelKey: "files.title", group: "operate" },
  { id: "players", icon: "users", labelKey: "players.title", group: "manage" },
  { id: "addons", icon: "puzzle", labelKey: "addons.title", group: "manage" },
  { id: "backups", icon: "archive", labelKey: "backups.title", group: "manage" },
  { id: "settings", icon: "sliders", labelKey: "settings.title", group: "manage" },
];

const COLLAPSED_KEY = "nebula.sidebarCollapsed";
const RAIL_ID = "app-sidebar";

/** Phone-width windows (the web panel on a phone): the sidebar becomes a
 *  drawer over the content instead of a column beside it. */
const NARROW_QUERY = "(max-width: 760px)";

function useNarrow(): boolean {
  const [narrow, setNarrow] = useState(() => window.matchMedia(NARROW_QUERY).matches);
  useEffect(() => {
    const query = window.matchMedia(NARROW_QUERY);
    const update = () => setNarrow(query.matches);
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);
  return narrow;
}

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

export function AppShell() {
  const navigate = useNavigate();
  const { t } = useI18n();
  const { data: node, isSuccess } = useNode();
  const { data: instances } = useInstances();
  const match = useMatch("/servers/:instanceId/:panel");
  const instanceId = match?.params.instanceId ?? null;

  useBackendEvents();
  useTransferTracking();

  const narrow = useNarrow();
  // Tells the stylesheet to leave room for the tab bar.
  useEffect(() => {
    if (narrow) document.documentElement.dataset.tabs = "";
    else delete document.documentElement.dataset.tabs;
  }, [narrow]);
  const { pathname } = useLocation();
  // The drawer belongs to the page it was opened on: picking a destination
  // in it closes it.
  const [drawerFor, setDrawerFor] = useState<string | null>(null);
  const drawerOpen = drawerFor === pathname;
  const setDrawerOpen = (open: boolean) => setDrawerFor(open ? pathname : null);
  const [storedCollapsed, setCollapsed] = useState(readCollapsed);
  // In a drawer the sidebar is always full width; "collapsed" means closed.
  const collapsed = narrow ? false : storedCollapsed;
  const toggleSidebar = useCallback(() => {
    if (window.matchMedia(NARROW_QUERY).matches) setDrawerFor((open) => (open === pathname ? null : pathname));
    else setCollapsed((value) => !value);
  }, [pathname]);
  const expandSidebar = useCallback(() => setCollapsed(false), []);

  useEffect(() => {
    try {
      localStorage.setItem(COLLAPSED_KEY, storedCollapsed ? "1" : "0");
    } catch {
      // Storage blocked: the choice simply lasts for this session.
    }
  }, [storedCollapsed]);

  // Ctrl+B (Cmd+B), the common sidebar shortcut. Nothing in the app binds it.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (
        (event.ctrlKey || event.metaKey) &&
        !event.altKey &&
        !event.shiftKey &&
        event.key.toLowerCase() === "b"
      ) {
        event.preventDefault();
        toggleSidebar();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [toggleSidebar]);

  const tooltip = useRailTooltip(collapsed);

  // One scroller (main) serves every route, so a new page must not inherit
  // the last page's offset.
  const mainRef = useRef<HTMLElement>(null);
  useLayoutEffect(() => {
    mainRef.current?.scrollTo({ top: 0 });
  }, [pathname]);

  useEffect(() => {
    // A node that has never been named and has no servers is a first run.
    if (pathname !== "/launcher" && isSuccess && node && instances?.length === 0 && !node.paired_devices.length) {
      const seen = localStorage.getItem("nebula.firstRunDone");
      if (!seen) navigate("/welcome", { replace: true });
    }
  }, [isSuccess, node, instances, navigate, pathname]);

  return (
    <div className="flex h-full flex-col">
      {/* Phone width: the tab bar along the bottom replaces the sidebar. */}
      <TitleBar sidebar={narrow ? undefined : { collapsed, onToggle: toggleSidebar, controls: RAIL_ID }} />
      <div className="relative flex min-h-0 flex-1">
        {narrow && drawerOpen ? (
          <button
            type="button"
            className="rail-scrim"
            aria-label={t("nav.collapseSidebar")}
            onClick={() => setDrawerOpen(false)}
          />
        ) : null}
        {narrow ? null : (
        <nav
          id={RAIL_ID}
          className="rail"
          data-collapsed={collapsed ? "" : undefined}
          data-drawer={narrow ? "" : undefined}
          data-open={narrow && drawerOpen ? "" : undefined}
          inert={narrow && !drawerOpen ? true : undefined}
          aria-label={t("nav.sidebar")}
          {...tooltip.handlers}
        >
          {/* The machine block scrolls with the nav: it sits at the foot when
              there is room (margin-top: auto) and never covers a panel link on
              a short window. */}
          <div className="rail-body" onScroll={tooltip.hide}>
            {instanceId ? (
              <ServerNav
                key={instanceId}
                instanceId={instanceId}
                collapsed={collapsed} onExpand={expandSidebar} />
            ) : (
              <>
                <AppNav />
                {/* In the launcher the foot of the rail is the account, not the machine. */}
                {pathname.startsWith("/launcher") ? (collapsed ? null : <div className="rail-account"><AccountMenu placement="up" /></div>) : collapsed ? <NodeMini /> : <NodeBlock />}
              </>
            )}
          </div>
        </nav>
        )}

        <main ref={mainRef} className="app-main min-w-0 flex-1 overflow-auto">
          <RemoteLinkBanner />
          <Outlet />
        </main>
      </div>
      {narrow ? <MobileTabs instanceId={instanceId} /> : null}
      <Tour />
      {tooltip.element}
    </div>
  );
}

/**
 * Names for the collapsed rail. One tooltip for the whole sidebar, driven by
 * `data-tip` on whatever the pointer or keyboard focus is on. It is drawn in a
 * portal so the rail's overflow cannot clip it, and is decorative (the items
 * keep their own accessible names).
 */
function useRailTooltip(enabled: boolean) {
  const [tip, setTip] = useState<{ label: string; top: number; left: number } | null>(null);
  const hide = useCallback(() => setTip(null), []);

  const showFor = useCallback(
    (target: EventTarget | null) => {
      if (!enabled || !(target instanceof Element)) return;
      const anchor = target.closest<HTMLElement>("[data-tip]");
      const label = anchor?.dataset.tip;
      if (!anchor || !label) {
        setTip(null);
        return;
      }
      const rect = anchor.getBoundingClientRect();
      setTip({ label, top: rect.top + rect.height / 2, left: rect.right + 12 });
    },
    [enabled],
  );

  const visible = enabled && tip !== null;

  // WCAG 1.4.13: a tooltip must be dismissible without moving the pointer.
  useEffect(() => {
    if (!visible) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setTip(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [visible]);

  return {
    hide,
    handlers: {
      onPointerOver: (event: PointerEvent) => showFor(event.target),
      onPointerLeave: hide,
      onFocus: (event: FocusEvent) => {
        if (event.target instanceof Element && event.target.matches(":focus-visible")) {
          showFor(event.target);
        }
      },
      onBlur: hide,
    },
    element:
      visible && tip
        ? createPortal(
            <div
              className="rail-tooltip"
              aria-hidden="true"
              style={{ top: tip.top, left: tip.left }}
            >
              {tip.label}
            </div>,
            document.body,
          )
        : null,
  };
}

function AppNav() {
  const { t } = useI18n();
  const launcher = useLauncherEnabled();
  return (
    <div className="flex flex-col gap-0.5">
      <NavItem to="/servers" icon="servers" label={t("nav.servers")} />
      {launcher ? <NavItem to="/launcher" icon="play" label="Launcher" /> : null}
      <NavItem to="/settings" icon="sliders" label={t("nav.settings")} />
    </div>
  );
}

/** The rail a server owns while it is open: back, its control, its panels. */
function ServerNav({
  instanceId,
  collapsed,
  onExpand,
}: {
  instanceId: string;
  collapsed: boolean;
  onExpand: () => void;
}) {
  const { t } = useI18n();
  const instance = useInstance(instanceId);

  if (!instance) return <AppNav />;

  const back = t("nav.backToServers");

  return (
    <div className="flex flex-col gap-3">
      <NavLink to="/servers" className="rail-back" data-tip={back}>
        <Icon name="chevron-left" size={14} />
        <span className="rail-label">{back}</span>
      </NavLink>

      <ServerControl instance={instance} collapsed={collapsed} onExpand={onExpand} />

      <div className="rail-rule" aria-hidden="true" />

      <div className="flex flex-col gap-3">
        {(["operate", "manage"] as const).map((group) => (
          <div key={group} className="flex flex-col gap-0.5">
            <p className="label rail-group">{t(`nav.${group}`)}</p>
            {SERVER_PANELS.filter((panel) => panel.group === group).map((panel) => (
              <NavItem
                key={panel.id}
                to={`/servers/${instanceId}/${panel.id}`}
                icon={panel.icon}
                label={t(panel.labelKey)}
              />
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * Start / Stop / Restart for the open server. The state itself is shown by
 * the page; the keys only offer what makes sense for it. Their failures are
 * shown here, under the keys (collapsed: a warning key that opens the rail).
 */
function ServerControl({
  instance,
  collapsed,
  onExpand,
}: {
  instance: InstanceView;
  collapsed: boolean;
  onExpand: () => void;
}) {
  const { t, describeError } = useI18n();
  const actions = useInstanceActions();

  const running = instance.status === "online" || instance.status === "starting";
  // A server that is still shutting down takes no keys (same rule as the
  // dashboard card and row).
  const busy =
    actions.start.isPending ||
    actions.stop.isPending ||
    actions.restart.isPending ||
    instance.status === "stopping";

  // Only the most recently pressed key speaks: a stale start error should not
  // linger after a successful stop.
  const latest = [actions.start, actions.stop, actions.restart].reduce((a, b) =>
    b.submittedAt > a.submittedAt ? b : a,
  );
  const failure = latest.error;

  const startLabel = instance.status === "sleeping" ? t("actions.wake") : t("actions.start");
  // An older copy left behind by a transfer starts only from its overview
  // ("Play this copy anyway").
  const olderCopy = instance.transfer?.state === "transferred_out";
  const start = () => actions.start.mutate(instance.id);
  const stop = () => actions.stop.mutate(instance.id);
  const restart = () => actions.restart.mutate(instance.id);

  if (collapsed) {
    const failureTitle = failure ? describeError(toAppError(failure)).title : "";
    return (
      <section className="rail-control" data-collapsed="" aria-label={instance.name}>
        {running ? (
          <>
            <RailKey icon="stop" label={t("actions.stop")} disabled={busy} onClick={stop} />
            <RailKey icon="restart" label={t("actions.restart")} disabled={busy} onClick={restart} />
          </>
        ) : (
          <RailKey
            icon="play"
            label={startLabel}
            primary
            disabled={busy || olderCopy}
            onClick={start}
          />
        )}
        {failure ? (
          <button
            type="button"
            className="rail-key rail-key-alert"
            data-tip={failureTitle}
            aria-label={failureTitle}
            onClick={onExpand}
          >
            <Icon name="warning" />
          </button>
        ) : null}
      </section>
    );
  }

  return (
    <section className="rail-control" aria-label={instance.name}>
      <div className="flex gap-2">
        {running ? (
          <Button size="sm" icon="stop" className="flex-1" disabled={busy} onClick={stop}>
            {t("actions.stop")}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="primary"
            icon="play"
            className="flex-1"
            disabled={busy || olderCopy}
            title={
              instance.transfer?.state === "transferred_out"
                ? t("transfer.startDisabled", { node: instance.transfer.to_node_name })
                : undefined
            }
            onClick={start}
          >
            {startLabel}
          </Button>
        )}
        {running ? (
          <Button size="sm" icon="restart" className="flex-1" disabled={busy} onClick={restart}>
            {t("actions.restart")}
          </Button>
        ) : null}
      </div>

      {failure ? <ErrorNotice error={failure} compact /> : null}
    </section>
  );
}

/** An icon-only key for the collapsed rail; its name shows as the tooltip. */
function RailKey({
  icon,
  label,
  primary = false,
  disabled,
  onClick,
}: {
  icon: IconName;
  label: string;
  primary?: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`btn btn-sm rail-key ${primary ? "btn-primary" : ""}`}
      data-tip={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
    >
      <Icon name={icon} />
    </button>
  );
}

function NavItem({ to, icon, label }: { to: string; icon: IconName; label: string }) {
  // Same rule NavLink uses (prefix match), surfaced as an attribute so the
  // active look is a CSS concern.
  const active = useMatch({ path: to, end: false }) !== null;
  return (
    <NavLink
      to={to}
      data-active={active ? "" : undefined}
      data-tip={label}
      className={({ isActive }) => `rail-item ${isActive ? "active" : ""}`}
    >
      <Icon name={icon} />
      <span className="rail-label">{label}</span>
    </NavLink>
  );
}

/**
 * While managing another machine that stops answering: say so above every
 * page, keep retrying (the event stream and the queries already do), and
 * offer the way back to this PC.
 */
function RemoteLinkBanner() {
  const { t } = useI18n();
  const client = useQueryClient();
  const { remote, reachable } = useRemoteLink();
  if (!remote || reachable) return null;

  return (
    <div className="remote-banner" role="status">
      <Icon name="warning" size={16} />
      <div className="remote-banner-text">
        <p className="remote-banner-title">{t("remote.unreachable", { name: remote.name })}</p>
        <p className="remote-banner-body">{t("remote.unreachableBody")}</p>
      </div>
      <div className="remote-banner-actions">
        <Button size="sm" variant="quiet" onClick={() => void client.invalidateQueries()}>
          {t("actions.retry")}
        </Button>
        <Button size="sm" onClick={() => switchTo("local")}>
          {t("remote.useThisPc")}
        </Button>
      </div>
    </div>
  );
}

/**
 * The machine being managed and its load: this PC, or the remote node by its
 * own name with its own numbers (every query is routed to it). The lamp says
 * whether it answers.
 */
function useMachine() {
  const { t } = useI18n();
  const { remote, reachable } = useRemoteLink();
  const { data: node } = useNode();
  const health = useNodeHealth();
  // The last numbers stay on screen (dimmed) when a reading fails or the link
  // drops: taking the block away and putting it back on every missed poll made
  // the sidebar jump. Before the first reading, the dials show empty.
  const stale = !reachable || health.isError;
  const title = remote ? (node?.name ?? remote.name) : t("node.thisMachine");
  return { remote, reachable, up: !stale && health.data !== undefined, stale, title, health: health.data };
}

function NodeBlock() {
  const { t } = useI18n();
  const { remote, reachable, up, stale, title, health } = useMachine();

  return (
    <section className="node-block" aria-label={title} data-stale={stale || !health ? "" : undefined}>
      <div className="flex items-center justify-between gap-2">
        <span className="label node-block-title" title={title}>
          {title}
        </span>
        <StatusDot status={up ? "online" : "offline"} size={7} label={t(up ? "status.online" : "status.offline")} />
      </div>

      {remote && !reachable ? <p className="node-block-reconnecting">{t("remote.reconnecting")}</p> : null}

      <div className="node-rings">
        <Ring size={52} label={t("node.cpu")} value={health?.cpu_percent ?? 0} />
        <Ring
          size={52}
          label={t("node.memory")}
          value={health?.memory_used_mb ?? 0}
          max={health?.memory_total_mb ?? 1}
          detail={health ? formatMegabytes(health.memory_total_mb) : "—"}
        />
        <Ring
          size={52}
          label={t("node.disk")}
          value={health?.disk_used_gb ?? 0}
          max={health?.disk_total_gb ?? 1}
          detail={health ? `${health.disk_total_gb} GB` : "—"}
        />
      </div>
    </section>
  );
}

/** The machine block, folded into the collapsed rail: three small dials. */
function NodeMini() {
  const { t } = useI18n();
  const { remote, reachable, up, title, health } = useMachine();
  const state = remote && !reachable ? t("remote.reconnecting") : t(up ? "status.online" : "status.offline");

  return (
    <section className="node-mini" aria-label={title}>
      <span className="node-mini-lamp" data-tip={`${title} · ${state}`}>
        <StatusDot status={up ? "online" : "offline"} size={7} label={state} />
      </span>
      {health ? (
        <>
          <span data-tip={`${t("node.cpu")} ${Math.round(health.cpu_percent)}%`}>
            <Ring size={30} stroke={3} label={t("node.cpu")} value={health.cpu_percent} />
          </span>
          <span
            data-tip={`${t("node.memory")} ${formatMegabytes(health.memory_used_mb)} / ${formatMegabytes(
              health.memory_total_mb,
            )}`}
          >
            <Ring
              size={30}
              stroke={3}
              label={t("node.memory")}
              value={health.memory_used_mb}
              max={health.memory_total_mb}
            />
          </span>
          <span
            data-tip={`${t("node.disk")} ${Math.round(health.disk_used_gb)} / ${health.disk_total_gb} GB`}
          >
            <Ring
              size={30}
              stroke={3}
              label={t("node.disk")}
              value={health.disk_used_gb}
              max={health.disk_total_gb}
            />
          </span>
        </>
      ) : null}
    </section>
  );
}
