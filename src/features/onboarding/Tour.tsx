/**
 * A short tour shown once, after the first-run screen: what the main parts of
 * the app are for. It can be opened again from Settings > About.
 */

import { useCallback, useEffect, useState } from "react";

import { useInstances } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { useLauncherEnabled } from "@/lib/launcherPref";
import { DESKTOP_FEATURES, IS_WEB } from "@/lib/web";
import { Button, Dialog, Icon, type IconName } from "@/ui";

import { OPEN_TOUR } from "./tourEvents";
import "./Tour.css";

const TOUR_KEY = "nebula.tourDone";

interface Step {
  id: string;
  icon: IconName;
  items: { icon: IconName; key: string }[];
}

const WELCOME: Step = { id: "welcome", icon: "servers", items: [] };

const STEPS: Step[] = [
  WELCOME,
  {
    id: "servers",
    icon: "servers",
    items: [
      { icon: "plus", key: "create" },
      { icon: "power", key: "control" },
      { icon: "chevron-down", key: "menu" },
    ],
  },
  {
    id: "server",
    icon: "terminal",
    items: [
      { icon: "terminal", key: "console" },
      { icon: "folder", key: "files" },
      { icon: "users", key: "players" },
      { icon: "puzzle", key: "addons" },
      { icon: "archive", key: "backups" },
    ],
  },
  { id: "launcher", icon: "play", items: [] },
  {
    id: "machines",
    icon: "link",
    items: [
      { icon: "monitor", key: "pair" },
      { icon: "activity", key: "fleet" },
    ],
  },
  {
    id: "settings",
    icon: "sliders",
    items: [
      { icon: "sliders", key: "options" },
      { icon: "info", key: "help" },
    ],
  },
];

/** Phones and the web panel are touched, not clicked, and have no sidebar or tray. */
const TOUCH =
  !DESKTOP_FEATURES && typeof window !== "undefined" && (window.matchMedia?.("(pointer: coarse)").matches ?? true);

/** The words of a step's line, in the touch version where it has one. */
const TOUCH_TEXT: Record<string, string> = {
  "servers.lead": "tour.touch.serversLead",
  "servers.control.text": "tour.touch.control",
  "servers.menu.title": "tour.touch.menuTitle",
  "servers.menu.text": "tour.touch.menu",
  "server.lead": "tour.touch.serverLead",
  "machines.pair.text": "tour.touch.pair",
  "settings.options.text": "tour.touch.options",
};

function key(path: string): string {
  // No tray in a browser, mouse or not.
  if (path === "settings.options.text" && !DESKTOP_FEATURES) return "tour.touch.options";
  return (TOUCH && TOUCH_TEXT[path]) || `tour.${path}`;
}

/** Another dialog is up (creating a server, the EULA): the tour waits. */
function otherDialogOpen(): boolean {
  return typeof document !== "undefined" && document.querySelector(".dialog-scrim") !== null;
}

function wantsTour(hasServers: boolean): boolean {
  try {
    // The desktop shows it after its first-run screen; phones and the web
    // panel have none, so there it greets a first visit, before anything
    // else is opened on top of it.
    const setUp = localStorage.getItem("nebula.firstRunDone") === "1" || hasServers || !DESKTOP_FEATURES;
    return setUp && localStorage.getItem(TOUR_KEY) !== "1";
  } catch {
    return false;
  }
}

export function Tour() {
  const { t } = useI18n();
  const [manual, setManual] = useState(false);
  const [closed, setClosed] = useState(false);
  const [index, setIndex] = useState(0);
  const launcher = useLauncherEnabled();
  const { data: instances, isSuccess } = useInstances();

  useEffect(() => {
    const show = () => {
      setIndex(0);
      setManual(true);
    };
    window.addEventListener(OPEN_TOUR, show);
    return () => window.removeEventListener(OPEN_TOUR, show);
  }, []);

  // Once, after first-run setup is done (or for someone who already has
  // servers): never on top of the first-run screen itself.
  const installing = instances?.some((instance) => instance.setup) ?? false;
  const [blocked, setBlocked] = useState(true);
  const candidate = isSuccess && !closed && !installing && wantsTour(instances.length > 0);
  // Never on top of another dialog: wait until none is open.
  useEffect(() => {
    if (!candidate) return;
    const check = () => setBlocked(otherDialogOpen());
    check();
    const timer = window.setInterval(check, 600);
    return () => window.clearInterval(timer);
  }, [candidate]);
  const automatic = candidate && !blocked;
  const [shown, setShown] = useState(false);
  if (automatic && !shown) setShown(true);
  const open = manual || automatic || (shown && !closed);

  const close = useCallback(() => {
    try {
      localStorage.setItem(TOUR_KEY, "1");
    } catch {
      // Storage blocked: the tour just shows again next time.
    }
    setClosed(true);
    setManual(false);
  }, []);

  // The launcher does not exist in the browser panel, and the browser panel
  // manages only the machine that served it.
  const steps = STEPS.filter((step) => (step.id !== "launcher" || launcher) && (step.id !== "machines" || !IS_WEB));
  const step = steps[Math.min(index, steps.length - 1)] ?? WELCOME;
  const last = index >= steps.length - 1;

  return (
    <Dialog
      open={open}
      onClose={close}
      title={t(`tour.${step.id}.title`)}
      width={520}
      header={
        <ol className="tour-dots" aria-label={t("tour.progress", { n: index + 1, total: steps.length })}>
          {steps.map((entry, position) => (
            <li key={entry.id} data-state={position === index ? "current" : position < index ? "done" : ""} />
          ))}
        </ol>
      }
      footer={
        <>
          {last ? null : (
            <Button variant="quiet" onClick={close}>
              {t("tour.skip")}
            </Button>
          )}
          {index > 0 ? <Button onClick={() => setIndex(index - 1)}>{t("tour.back")}</Button> : null}
          <Button variant="primary" onClick={() => (last ? close() : setIndex(index + 1))}>
            {last ? t("tour.done") : t("tour.next")}
          </Button>
        </>
      }
    >
      <div className="tour-body">
        <p className="tour-lead">{t(key(`${step.id}.lead`))}</p>
        {step.items.length ? (
          <ul className="tour-items">
            {step.items.map((item) => (
              <li key={item.key} className="tour-item">
                <span className="tour-item-icon" aria-hidden="true">
                  <Icon name={item.icon} size={16} />
                </span>
                <span className="flex min-w-0 flex-col gap-0.5">
                  <span className="tour-item-title">{t(key(`${step.id}.${item.key}.title`))}</span>
                  <span className="tour-item-text">{t(key(`${step.id}.${item.key}.text`))}</span>
                </span>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    </Dialog>
  );
}
