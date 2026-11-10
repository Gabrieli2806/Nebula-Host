/**
 * A server that is still being set up, said on its own card.
 *
 * Whoever started the install (this window, an agent, a paired device), the
 * server is in the list from the first moment and says what it is doing,
 * instead of appearing from nowhere once it is done.
 */

import { useI18n } from "@/i18n";
import { formatBytes } from "@/lib/format";
import type { InstallProgress, InstanceView } from "@/types/api";
import { Meter } from "@/ui";

/** The install a server is in the middle of, or that failed. */
export function setupOf(instance: InstanceView): InstallProgress | null {
  return instance.setup ?? null;
}

/** The word for the status chip: "Getting ready" or "Setup failed". */
export function useSetupLabel(setup: InstallProgress | null): string | null {
  const { t } = useI18n();
  if (!setup) return null;
  return setup.phase === "failed" ? t("install.failed") : t("install.gettingReady");
}

function usePhaseText(setup: InstallProgress): string {
  const { t } = useI18n();
  switch (setup.phase) {
    case "resolving":
      return t("install.resolving");
    case "downloading":
      return t("install.downloadingGeneric");
    case "installing_loader":
      return t("install.installingLoader", { distribution: setup.distribution });
    case "configuring":
      return t("install.configuring");
    case "generating_world":
      return t("install.generatingWorld");
    case "failed":
      return t("install.failedHint");
    case "ready":
      return t("install.ready");
  }
}

/** How far along each phase is, when the phase itself gives no number: the
 *  bar moves forward with the phases, and the download fills its own share. */
const PHASE_SHARE: Record<InstallProgress["phase"], [from: number, to: number]> = {
  resolving: [4, 4],
  downloading: [8, 70],
  installing_loader: [75, 75],
  configuring: [90, 90],
  generating_world: [95, 95],
  ready: [100, 100],
  failed: [0, 0],
};

/** What the install is doing now, with the download's progress when known. */
export function SetupStrip({ setup }: { setup: InstallProgress }) {
  const text = usePhaseText(setup);
  const downloading = setup.phase === "downloading" ? setup : null;
  const [from, to] = PHASE_SHARE[setup.phase];
  const within = downloading?.total ? Math.min(1, downloading.received / downloading.total) : 0;
  const share = from + (to - from) * within;

  return (
    <div className="server-card-setup" data-failed={setup.phase === "failed" ? "" : undefined} role="status">
      <p className="server-card-setup-text">
        <span>{text}</span>
        {downloading ? (
          <span className="server-card-setup-bytes">
            {formatBytes(downloading.received)}
            {downloading.total ? ` / ${formatBytes(downloading.total)}` : ""}
          </span>
        ) : null}
      </p>
      {setup.phase === "failed" ? null : <Meter value={share} max={100} />}
    </div>
  );
}
