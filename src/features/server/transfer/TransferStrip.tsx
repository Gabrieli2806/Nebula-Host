/**
 * The strip on a server's card and overview, and the tag on its table row,
 * while a transfer is running (see active.ts for where the state comes from).
 */

import { useI18n } from "@/i18n";
import { formatBytes } from "@/lib/format";
import { Button, Icon } from "@/ui";

import { cancelTransfer, PHASES, useActiveTransfer } from "./active";
import "./transfer.css";

/** The strip on a card or overview: where it is going and which step. */
export function TransferProgressStrip({ instanceId, compact = false }: { instanceId: string; compact?: boolean }) {
  const { t } = useI18n();
  const transfer = useActiveTransfer(instanceId);
  if (!transfer) return null;

  const target = transfer.target ?? t("transfer.otherMachine");
  const progress = transfer.progress;
  const hasBytes = progress !== null && progress.total > 0;
  const share = hasBytes ? Math.min(100, Math.round((progress.bytes / progress.total) * 100)) : null;
  const step = progress ? PHASES.indexOf(progress.phase) + 1 : 1;
  const phase = t(`transfer.phase.${progress?.phase ?? "prepare"}`, { target });

  return (
    <div className="transfer-strip" data-compact={compact ? "" : undefined} role="status" aria-live="polite">
      <div className="transfer-strip-head">
        <Icon name="send" size={13} />
        <span className="transfer-strip-title">{t("transfer.running", { target })}</span>
        <span className="transfer-strip-step readout">
          {step}/{PHASES.length}
        </span>
      </div>
      <div className="transfer-strip-phase">
        <span>{phase}</span>
        {hasBytes ? (
          <span className="readout">
            {compact ? `${share}%` : `${formatBytes(progress.bytes)} / ${formatBytes(progress.total)}`}
          </span>
        ) : null}
      </div>
      <span className="transfer-strip-bar" aria-hidden="true" data-indeterminate={share === null ? "" : undefined}>
        <span style={share === null ? undefined : { width: `${share}%` }} />
      </span>
      {/* Only the window that started it can call it off, and not once the
          source has begun to retire its copy. */}
      {transfer.target !== undefined && progress?.phase !== "finalize" ? (
        <Button
          size="sm"
          variant="quiet"
          className="transfer-strip-cancel"
          disabled={transfer.cancelling}
          onClick={() => cancelTransfer(instanceId)}
        >
          {transfer.cancelling ? t("transfer.cancelling") : t("transfer.cancel")}
        </Button>
      ) : null}
    </div>
  );
}

/** The compact version for a table row: a tag with the share sent. */
export function TransferTag({ instanceId }: { instanceId: string }) {
  const { t } = useI18n();
  const transfer = useActiveTransfer(instanceId);
  if (!transfer) return null;
  const progress = transfer.progress;
  const share =
    progress && progress.total > 0 ? ` ${Math.min(100, Math.round((progress.bytes / progress.total) * 100))}%` : "";
  return (
    <span className="tag transfer-tag" role="status">
      <Icon name="send" size={11} />
      {t("transfer.tagRunning")}
      {share}
    </span>
  );
}
