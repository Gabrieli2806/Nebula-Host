/**
 * Installing takes 30 to 90 seconds for a modded server. That wait gets a
 * designed surface: the real phases, in order, with the one in progress named
 * (never an unlabelled spinner that leaves someone wondering if it hung).
 *
 * Each phase row carries its state as `data-state` (done / active / failed /
 * pending); the lamp, check and weight are CSS (servers.css).
 */

import { useState } from "react";

import { useInstallProgress } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatBytes } from "@/lib/format";
import { ErrorNotice, Icon, Meter } from "@/ui";

const PHASES = ["resolving", "downloading", "installing_loader", "configuring", "ready"] as const;
type Phase = (typeof PHASES)[number];

const LABEL: Record<Phase, string> = {
  resolving: "install.resolving",
  downloading: "install.downloading",
  installing_loader: "install.installingLoader",
  configuring: "install.configuring",
  ready: "install.ready",
};

type RowState = "done" | "active" | "failed" | "pending";

export function InstallProgressPanel({
  instanceId,
  error,
}: {
  instanceId: string | null;
  error?: unknown;
}) {
  const { t } = useI18n();
  const progress = useInstallProgress(instanceId);

  // The file and loader names only arrive while their phase runs. Remember
  // them, so a finished row keeps its name and a row not reached yet reads
  // generically instead of showing a raw {file} / {distribution} placeholder.
  const [seen, setSeen] = useState<{ file?: string; distribution?: string }>({});
  if (progress?.phase === "downloading" && progress.file && progress.file !== seen.file) {
    setSeen((prev) => ({ ...prev, file: progress.file }));
  }
  if (progress?.phase === "installing_loader" && progress.distribution !== seen.distribution) {
    setSeen((prev) => ({ ...prev, distribution: progress.distribution }));
  }
  const label = (phase: Phase): string => {
    if (phase === "downloading") return seen.file ? t(LABEL.downloading, { file: seen.file }) : t("install.downloadingGeneric");
    if (phase === "installing_loader")
      return seen.distribution ? t(LABEL.installing_loader, { distribution: seen.distribution }) : t("install.installingLoaderGeneric");
    return t(LABEL[phase]);
  };

  const current: Phase =
    progress?.phase === "generating_world"
      ? "configuring"
      : progress?.phase === "failed"
        ? "resolving"
        : ((progress?.phase ?? "resolving") as Phase);
  const currentIndex = PHASES.indexOf(current);
  const failed = progress?.phase === "failed" || Boolean(error);
  const ready = progress?.phase === "ready";

  return (
    <div className="flex flex-col gap-4">
      <ol className="install-phases">
        {PHASES.map((phase, index) => {
          const state: RowState =
            index < currentIndex || ready
              ? "done"
              : index === currentIndex
                ? failed
                  ? "failed"
                  : "active"
                : "pending";

          return (
            <li
              key={phase}
              className="install-phase"
              data-state={state}
              aria-current={state === "active" ? "step" : undefined}
            >
              <span className="install-phase-mark" aria-hidden="true">
                {state === "done" ? <Icon name="check" size={10} /> : null}
              </span>

              <div className="flex min-w-0 flex-1 flex-col gap-2">
                <span className="install-phase-label">
                  {label(phase)}
                </span>

                {state === "active" && progress?.phase === "downloading" && progress.total ? (
                  <Meter
                    value={progress.received}
                    max={progress.total}
                    detail={`${formatBytes(progress.received)} / ${formatBytes(progress.total)}`}
                  />
                ) : null}
              </div>
            </li>
          );
        })}
      </ol>

      {failed ? (
        <ErrorNotice
          error={
            error ?? {
              code: progress?.phase === "failed" ? progress.code : "unexpected",
              params: {},
            }
          }
        />
      ) : null}
    </div>
  );
}

