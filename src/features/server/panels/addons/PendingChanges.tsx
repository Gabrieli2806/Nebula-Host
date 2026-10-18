/**
 * Addon changes waiting for the server to stop.
 *
 * While a server runs its jars are loaded (and on Windows, locked), so an
 * install, update, removal or switch made then is queued: downloaded at
 * once, moved into place when the server stops. This is that queue, each
 * item cancellable, with any change that failed to apply and why.
 */

import { usePendingAddonMutations } from "@/hooks/useBackend";
import type { InstanceView, PendingAddonChange } from "@/types/api";
import { Button, ErrorNotice, Icon } from "@/ui";

import { useAddonCopy } from "./util";

export function PendingChanges({ instance, pending }: { instance: InstanceView; pending: PendingAddonChange[] }) {
  const { t, tc } = useAddonCopy();
  const { cancel, apply } = usePendingAddonMutations(instance.id);

  if (pending.length === 0) return null;

  // Process up (or on its way up/down): the queue is waiting, as designed.
  // Otherwise whatever is still here failed when the server stopped.
  const running = instance.status === "online" || instance.status === "starting" || instance.status === "stopping";
  const failed = pending.some((change) => change.error);

  return (
    <section className="addons-pending" aria-label={t("addons.pendingTitle")} data-failed={failed ? "" : undefined}>
      <header className="addons-pending-head">
        <Icon name={failed ? "warning" : "clock"} size={14} />
        <div className="min-w-0 flex-1">
          <p className="addons-pending-title">
            {t("addons.pendingTitle")}
            <span className="tag" data-tone={failed ? "warn" : "accent"}>
              {tc("addons.pendingCount", pending.length)}
            </span>
          </p>
          <p className="addons-pending-hint">{running ? t("addons.pendingHint") : t("addons.pendingHintStopped")}</p>
        </div>
        {!running ? (
          <Button size="sm" icon="refresh" disabled={apply.isPending} onClick={() => apply.mutate()}>
            {t("addons.pendingRetry")}
          </Button>
        ) : null}
      </header>

      <ul className="addons-pending-list">
        {pending.map((change) => {
          const name = change.title ?? fileOf(change);
          return (
            <li key={change.id} className="addons-pending-row">
              <span className="addons-pending-kind" data-action={change.action}>
                <Icon name={iconOf(change)} size={13} />
              </span>
              <div className="min-w-0 flex-1">
                <p className="addons-pending-what">
                  <span className="addons-pending-verb">{describe(change, t)}</span>
                  <span className="truncate" title={name}>
                    {name}
                  </span>
                </p>
                <p className="addons-pending-file mono truncate" title={fileOf(change)}>
                  {fileOf(change)}
                  {change.action === "install" && change.disabled ? ` · ${t("addons.pendingStartsOff")}` : ""}
                </p>
                {change.error ? (
                  <div className="mt-2">
                    <ErrorNotice error={change.error} compact />
                  </div>
                ) : null}
              </div>
              <Button
                size="sm"
                variant="quiet"
                icon="close"
                iconOnly
                disabled={cancel.isPending}
                onClick={() => cancel.mutate(change.id)}
              >
                {t("addons.pendingCancel", { name })}
              </Button>
            </li>
          );
        })}
      </ul>

      {cancel.error || apply.error ? <ErrorNotice error={cancel.error ?? apply.error} compact /> : null}
    </section>
  );
}

function fileOf(change: PendingAddonChange): string {
  return change.file_name;
}

function iconOf(change: PendingAddonChange) {
  switch (change.action) {
    case "install":
      return change.replace ? ("refresh" as const) : ("plus" as const);
    case "remove":
      return "trash" as const;
    case "set_enabled":
      return "power" as const;
  }
}

function describe(change: PendingAddonChange, t: (key: string, params?: Record<string, string | number>) => string) {
  switch (change.action) {
    case "install":
      if (change.replace) return t("addons.pendingUpdate", { version: change.version_number ?? "" });
      return change.version_number
        ? t("addons.pendingInstall", { version: change.version_number })
        : t("addons.pendingInstallPlain");
    case "remove":
      return t("addons.pendingRemove");
    case "set_enabled":
      return change.enabled ? t("addons.pendingEnable") : t("addons.pendingDisable");
  }
}
