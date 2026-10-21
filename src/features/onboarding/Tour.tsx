/**
 * A short tour shown once, after the first-run screen: what the main parts of
 * the app are for. It can be opened again from Settings > About.
 */

import { useCallback, useEffect, useState } from "react";

import { useInstances } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { IS_WEB } from "@/lib/web";
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

function wantsTour(hasServers: boolean): boolean {
  try {
    const setUp = localStorage.getItem("nebula.firstRunDone") === "1" || hasServers;
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
  const automatic = isSuccess && !closed && wantsTour(instances.length > 0);
  const open = manual || automatic;

  const close = useCallback(() => {
    try {
      localStorage.setItem(TOUR_KEY, "1");
    } catch {
      // Storage blocked: the tour just shows again next time.
    }
    setClosed(true);
    setManual(false);
  }, []);

  // The launcher does not exist in the browser panel.
  const steps = IS_WEB ? STEPS.filter((step) => step.id !== "launcher") : STEPS;
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
        <p className="tour-lead">{t(`tour.${step.id}.lead`)}</p>
        {step.items.length ? (
          <ul className="tour-items">
            {step.items.map((item) => (
              <li key={item.key} className="tour-item">
                <span className="tour-item-icon" aria-hidden="true">
                  <Icon name={item.icon} size={16} />
                </span>
                <span className="flex min-w-0 flex-col gap-0.5">
                  <span className="tour-item-title">{t(`tour.${step.id}.${item.key}.title`)}</span>
                  <span className="tour-item-text">{t(`tour.${step.id}.${item.key}.text`)}</span>
                </span>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    </Dialog>
  );
}
