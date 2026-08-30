/**
 * Installing takes 30–90 seconds for a modded server. That wait gets a
 * designed surface: the real phases, in order, with the one in progress named
 * — never an unlabelled spinner that leaves someone wondering if it hung.
 */

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

export function InstallProgressPanel({
  instanceId,
  error,
}: {
  instanceId: string | null;
  error?: unknown;
}) {
  const { t } = useI18n();
  const progress = useInstallProgress(instanceId);

  const current: Phase =
    progress?.phase === "generating_world"
      ? "configuring"
      : progress?.phase === "failed"
        ? "resolving"
        : ((progress?.phase ?? "resolving") as Phase);
  const currentIndex = PHASES.indexOf(current);

  return (
    <div className="flex flex-col gap-4">
      <ol className="flex flex-col gap-2.5">
        {PHASES.map((phase, index) => {
          const done = index < currentIndex || progress?.phase === "ready";
          const active = index === currentIndex && progress?.phase !== "ready";

          return (
            <li key={phase} className="flex items-start gap-3">
              <span
                className="mt-[3px] flex h-4 w-4 flex-none items-center justify-center rounded-full border"
                style={{
                  borderColor: done || active ? "var(--accent)" : "var(--line-strong)",
                  background: done ? "var(--accent)" : "transparent",
                  color: "var(--text-on-accent)",
                }}
              >
                {done ? <Icon name="check" size={10} /> : null}
              </span>

              <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                <span
                  className="text-[length:var(--text-sm)]"
                  style={{
                    color: active ? "var(--text)" : "var(--text-muted)",
                    fontWeight: active ? 600 : 400,
                  }}
                >
                  {t(LABEL[phase], phaseParams(progress))}
                </span>

                {active && progress?.phase === "downloading" && progress.total ? (
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

      {progress?.phase === "failed" || error ? (
        <ErrorNotice error={error ?? { code: progress?.phase === "failed" ? progress.code : "unexpected", params: {} }} />
      ) : null}
    </div>
  );
}

function phaseParams(
  progress: ReturnType<typeof useInstallProgress>,
): Record<string, string> | undefined {
  if (!progress) return undefined;
  if (progress.phase === "downloading") return { file: progress.file };
  if (progress.phase === "installing_loader") return { distribution: progress.distribution };
  return undefined;
}
