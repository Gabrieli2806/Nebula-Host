/**
 * Moving a server to another machine: choose where and what happens to the
 * copy left here, then watch it go phase by phase.
 *
 * The transfer runs in this PC's backend (it may be the source, the target or
 * only the relay between two nodes). The source keeps the server until the
 * target has confirmed it, so a failure at any point leaves it where it was;
 * the dialog says so.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { activeRemote, switchTo } from "@/lib/connection";
import { formatBytes } from "@/lib/format";
import { api, onAppEvent, onLocalAppEvent, toAppError } from "@/lib/ipc";
import type { AppEvent, InstanceView, TransferMode, TransferOutcome, TransferPhase, TransferProgress } from "@/types/api";
import { Button, Dialog, ErrorNotice, Icon } from "@/ui";

import { managedKey, useMachines, type Machine } from "./machines";
import "./transfer.css";

const PHASES: TransferPhase[] = ["prepare", "export", "upload", "verify", "adopt", "finalize"];

/** Reasons the dialog answers by asking again rather than just failing. */
const RETRYABLE = new Set(["players_online", "transfer_diverged"]);

export function TransferDialog({
  instance,
  open,
  onClose,
  bringBackFrom,
}: {
  instance: InstanceView;
  open: boolean;
  onClose: () => void;
  /**
   * Bring the server back: the source is the machine it moved to (this
   * window's machine holds the older copy and is the target).
   */
  bringBackFrom?: Machine;
}) {
  const { t } = useI18n();
  const client = useQueryClient();
  const machines = useMachines(t("transfer.thisPc"));

  const here = machines.data?.find((machine) => machine.key === managedKey());
  const source = bringBackFrom ?? here;
  const destinations = useMemo(
    () => (bringBackFrom ? (here ? [here] : []) : (machines.data ?? []).filter((m) => m.key !== managedKey())),
    [bringBackFrom, here, machines.data],
  );

  const [targetKey, setTargetKey] = useState<string | null>(null);
  const [mode, setMode] = useState<TransferMode>("keep");
  const [allowPlayers, setAllowPlayers] = useState(false);
  const [keepBoth, setKeepBoth] = useState(false);
  const [progress, setProgress] = useState<TransferProgress | null>(null);
  const transferId = useRef("");

  const target = destinations.find((m) => m.key === targetKey) ?? (destinations.length === 1 ? destinations[0] : undefined);
  const players = bringBackFrom ? 0 : instance.status === "online" ? instance.players_online : 0;

  const run = useMutation({
    mutationFn: () => {
      transferId.current = crypto.randomUUID();
      setProgress(null);
      if (!source || !target) throw new Error("no machine");
      return api.transfer.run({
        transferId: transferId.current,
        instanceId: instance.id,
        source: source.endpoint,
        target: target.endpoint,
        mode,
        allowPlayers,
        keepBoth: keepBoth || undefined,
      });
    },
    onSettled: () => {
      void client.invalidateQueries({ queryKey: keys.instances });
      void client.invalidateQueries({ queryKey: keys.instance(instance.id) });
    },
  });

  // Progress comes on this PC's channel (the bytes it moves) and, while the
  // source packs, on the managed machine's.
  useEffect(() => {
    if (!open) return;
    const take = (event: AppEvent) => {
      if (event.type === "transfer" && event.progress.transferId === transferId.current) {
        setProgress(event.progress);
      }
    };
    const stops = [onLocalAppEvent(take)];
    if (activeRemote()) stops.push(onAppEvent(take));
    return () => stops.forEach((stop) => stop());
  }, [open]);

  const close = () => {
    if (run.isPending) return;
    run.reset();
    setProgress(null);
    setKeepBoth(false);
    setAllowPlayers(false);
    onClose();
  };

  const reason = run.error ? toAppError(run.error).params.reason : undefined;
  const names = { source: source?.name ?? "", target: target?.name ?? "", name: instance.name };

  // -- done ----------------------------------------------------------------
  if (run.data && target) {
    return (
      <Dialog
        open={open}
        onClose={close}
        title={t("transfer.doneTitle", names)}
        width={560}
        footer={
          <>
            <Button variant="quiet" onClick={close}>
              {t("dialog.close")}
            </Button>
            {target.key !== managedKey() ? (
              <Button variant="primary" icon="external" onClick={() => openOn(target, run.data)}>
                {t("transfer.openOn", names)}
              </Button>
            ) : null}
          </>
        }
      >
        <Outcome outcome={run.data} names={names} />
      </Dialog>
    );
  }

  // -- running -------------------------------------------------------------
  if (run.isPending) {
    return (
      <Dialog open={open} onClose={close} title={t("transfer.running", names)} width={560} dismissable={false}>
        <div className="flex flex-col gap-4">
          <Phases progress={progress} names={names} />
          <p className="transfer-quiet">{t("transfer.keepOpen")}</p>
        </div>
      </Dialog>
    );
  }

  // -- choose (and failures) -----------------------------------------------
  const failed = run.error && !(reason && RETRYABLE.has(reason));
  const needsPlayers = players > 0 || reason === "players_online";
  const canStart = Boolean(source && target) && (!needsPlayers || allowPlayers);

  return (
    <Dialog
      open={open}
      onClose={close}
      title={t("transfer.title", names)}
      width={560}
      footer={
        <>
          <Button variant="quiet" onClick={close}>
            {t("actions.cancel")}
          </Button>
          <Button variant="primary" icon="send" disabled={!canStart} onClick={() => run.mutate()}>
            {reason === "transfer_diverged" ? t("transfer.keepBoth") : t("transfer.start")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-5">
        <p className="transfer-quiet">{t("transfer.intro")}</p>

        {failed ? (
          <div className="flex flex-col gap-2">
            <ErrorNotice error={run.error} />
            {reason === "finalize_unknown" ? (
              <p className="transfer-quiet">{t("transfer.failedUnknown", names)}</p>
            ) : (
              <p className="transfer-safe">
                <Icon name="check" size={14} />
                {t("transfer.failedSafe", names)}
              </p>
            )}
          </div>
        ) : null}
        {reason === "transfer_diverged" ? <ErrorNotice error={run.error} /> : null}

        {bringBackFrom ? null : (
          <fieldset className="transfer-group">
            <legend className="transfer-legend">{t("transfer.destination")}</legend>
            {destinations.length === 0 && !machines.isLoading ? (
              <p className="transfer-quiet">{t("transfer.noDestinations")}</p>
            ) : (
              <div className="transfer-options" role="radiogroup">
                {destinations.map((machine) => (
                  <Choice
                    key={machine.key}
                    name="transfer-target"
                    checked={target?.key === machine.key}
                    onSelect={() => setTargetKey(machine.key)}
                    title={machine.name}
                    hint={machine.isThisPc ? t("transfer.thisPc") : undefined}
                    icon={machine.isThisPc ? "monitor" : "servers"}
                  />
                ))}
              </div>
            )}
          </fieldset>
        )}

        <fieldset className="transfer-group">
          <legend className="transfer-legend">{t("transfer.mode", names)}</legend>
          <div className="transfer-options" role="radiogroup">
            <Choice
              name="transfer-mode"
              checked={mode === "keep"}
              onSelect={() => setMode("keep")}
              title={t("transfer.modeKeep")}
              hint={t("transfer.modeKeepHint", names)}
            />
            <Choice
              name="transfer-mode"
              checked={mode === "delete"}
              onSelect={() => setMode("delete")}
              title={t("transfer.modeDelete")}
              hint={t("transfer.modeDeleteHint", { ...names, target: target?.name ?? "…" })}
            />
          </div>
        </fieldset>

        {needsPlayers ? (
          <label className="transfer-confirm">
            <input type="checkbox" checked={allowPlayers} onChange={(event) => setAllowPlayers(event.target.checked)} />
            <span>
              <span className="block">
                {t("transfer.playersConfirm", {
                  count: players || Number(toAppError(run.error).params.count ?? 0),
                })}
              </span>
              <span className="block transfer-quiet">{t("transfer.playersOnline")}</span>
            </span>
          </label>
        ) : null}

        {reason === "transfer_diverged" ? (
          <label className="transfer-confirm">
            <input type="checkbox" checked={keepBoth} onChange={(event) => setKeepBoth(event.target.checked)} />
            <span>{t("transfer.keepBoth")}</span>
          </label>
        ) : null}

        <p className="transfer-quiet">{t("transfer.notes", names)}</p>
      </div>
    </Dialog>
  );
}

/** Switch the window to the destination and land on the server there. */
function openOn(target: Machine, outcome: TransferOutcome) {
  window.location.hash = `#/servers/${outcome.adopted.instance.id}/overview`;
  if (target.key === managedKey()) return;
  switchTo(target.key);
}

function Choice({
  name,
  checked,
  onSelect,
  title,
  hint,
  icon,
}: {
  name: string;
  checked: boolean;
  onSelect: () => void;
  title: string;
  hint?: string;
  icon?: "monitor" | "servers";
}) {
  return (
    <label className="transfer-option" data-selected={checked ? "" : undefined}>
      <input type="radio" name={name} className="sr-only" checked={checked} onChange={onSelect} />
      <span className="transfer-option-mark" aria-hidden="true" />
      {icon ? <Icon name={icon} size={15} /> : null}
      <span className="flex min-w-0 flex-col gap-0.5">
        <span className="transfer-option-title">{title}</span>
        {hint ? <span className="transfer-option-hint">{hint}</span> : null}
      </span>
    </label>
  );
}

function Phases({ progress, names }: { progress: TransferProgress | null; names: Record<string, string> }) {
  const { t } = useI18n();
  const current = progress ? PHASES.indexOf(progress.phase) : 0;
  return (
    <ol className="transfer-phases">
      {PHASES.map((phase, index) => {
        const state = index < current ? "done" : index === current ? "active" : "waiting";
        const showBytes = state === "active" && progress && progress.total > 0;
        const share = showBytes ? Math.min(100, (progress.bytes / progress.total) * 100) : 0;
        return (
          <li key={phase} className="transfer-phase" data-state={state}>
            <span className="transfer-phase-mark" aria-hidden="true">
              {state === "done" ? <Icon name="check" size={12} /> : null}
            </span>
            <span className="transfer-phase-label">{t(`transfer.phase.${phase}`, names)}</span>
            {showBytes ? (
              <span className="transfer-phase-bytes readout">
                {formatBytes(progress.bytes)} / {formatBytes(progress.total)}
              </span>
            ) : null}
            {showBytes ? (
              <span className="transfer-phase-bar" aria-hidden="true">
                <span style={{ width: `${share}%` }} />
              </span>
            ) : null}
          </li>
        );
      })}
    </ol>
  );
}

function Outcome({ outcome, names }: { outcome: TransferOutcome; names: Record<string, string> }) {
  const { t } = useI18n();
  const report = outcome.adopted;
  const notes = [
    report.portChanged ? t("transfer.portChanged", { ...names, from: report.portChanged.from, to: report.portChanged.to }) : null,
    report.javaMissing ? t("transfer.javaMissing", { ...names, major: report.javaMissing }) : null,
    report.runtimePathCleared ? t("transfer.runtimeCleared", names) : null,
  ].filter((note): note is string => note !== null);

  return (
    <div className="flex flex-col gap-4">
      <p className="transfer-safe">
        <Icon name="check" size={14} />
        {outcome.mode === "delete" ? t("transfer.doneDeleted", names) : t("transfer.doneKept", names)}
      </p>
      {notes.length ? (
        <ul className="transfer-notes">
          {notes.map((note) => (
            <li key={note}>
              <Icon name="warning" size={14} />
              <span>{note}</span>
            </li>
          ))}
        </ul>
      ) : null}
      <p className="transfer-quiet">{t("transfer.backupsStay", names)}</p>
    </div>
  );
}
