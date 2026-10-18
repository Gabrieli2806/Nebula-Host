/**
 * What a copy's lineage means for the person looking at it.
 *
 * - An older copy left behind by a transfer says where the server went, and
 *   offers to bring it back, delete this copy, or play it anyway (a fork,
 *   behind a strong confirmation).
 * - A copy that was forced into play says it has diverged from the other one.
 */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";

import { keys, useInstanceActions } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatDateTime } from "@/lib/format";
import { api } from "@/lib/ipc";
import type { InstanceView } from "@/types/api";
import { Button, Dialog, ErrorNotice, Icon } from "@/ui";

import { machineByNodeId, movedTo, useMachines } from "./machines";
import { TransferDialog } from "./TransferDialog";
import "./transfer.css";

/** One line for a card or row: where the server went. */
export function TransferNote({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const node = movedTo(instance);
  if (!node) return null;
  return (
    <p className="transfer-note">
      <Icon name="send" size={13} />
      <span>{t("transfer.cardMoved", { node })}</span>
    </p>
  );
}

/** The overview banner, with the actions an older or diverged copy needs. */
export function TransferBanner({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const navigate = useNavigate();
  const actions = useInstanceActions();
  const machines = useMachines(t("transfer.thisPc"));

  const [bringBack, setBringBack] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [confirmPlay, setConfirmPlay] = useState(false);
  const [understood, setUnderstood] = useState(false);

  const force = useMutation({
    mutationFn: () => api.instances.forcePlay(instance.id),
    onSuccess: () => {
      setConfirmPlay(false);
      void client.invalidateQueries({ queryKey: keys.instances });
      void client.invalidateQueries({ queryKey: keys.instance(instance.id) });
    },
  });

  const transfer = instance.transfer;
  if (transfer?.state === "transferred_out") {
    const node = transfer.to_node_name;
    const holder = machineByNodeId(machines.data, transfer.to_node_id);
    const names = { node, name: instance.name };

    return (
      <section className="transfer-banner" role="status">
        <Icon name="send" />
        <div className="transfer-banner-text">
          <p className="transfer-banner-title">
            {t("transfer.movedTitle", { node, date: formatDateTime(transfer.at, language) })}
          </p>
          <p className="transfer-banner-body">{t("transfer.movedBody")}</p>
        </div>
        <div className="transfer-banner-actions">
          <Button
            size="sm"
            variant="primary"
            icon="download"
            disabled={!holder}
            title={holder ? undefined : t("transfer.bringBackUnavailable", names)}
            onClick={() => setBringBack(true)}
          >
            {t("transfer.bringBack")}
          </Button>
          <Button size="sm" variant="danger" icon="trash" onClick={() => setConfirmDelete(true)}>
            {t("transfer.deleteCopy")}
          </Button>
          <Button size="sm" variant="quiet" icon="play" onClick={() => setConfirmPlay(true)}>
            {t("transfer.playAnyway")}
          </Button>
        </div>

        {holder ? (
          <TransferDialog instance={instance} open={bringBack} onClose={() => setBringBack(false)} bringBackFrom={holder} />
        ) : null}

        <Dialog
          open={confirmDelete}
          onClose={() => setConfirmDelete(false)}
          title={t("transfer.deleteCopyTitle", names)}
          footer={
            <>
              <Button variant="quiet" onClick={() => setConfirmDelete(false)}>
                {t("actions.cancel")}
              </Button>
              <Button
                variant="danger"
                disabled={actions.remove.isPending}
                onClick={() =>
                  actions.remove.mutate({ id: instance.id, deleteFiles: true }, { onSuccess: () => navigate("/servers") })
                }
              >
                {t("actions.delete")}
              </Button>
            </>
          }
        >
          <div className="flex flex-col gap-3">
            <p>{t("transfer.deleteCopyBody", names)}</p>
            {actions.remove.error ? <ErrorNotice error={actions.remove.error} /> : null}
          </div>
        </Dialog>

        <Dialog
          open={confirmPlay}
          onClose={() => {
            setConfirmPlay(false);
            setUnderstood(false);
          }}
          title={t("transfer.playAnywayTitle", names)}
          footer={
            <>
              <Button variant="quiet" onClick={() => setConfirmPlay(false)}>
                {t("actions.cancel")}
              </Button>
              <Button variant="danger" disabled={!understood || force.isPending} onClick={() => force.mutate()}>
                {t("transfer.playAnywayConfirm")}
              </Button>
            </>
          }
        >
          <div className="flex flex-col gap-4">
            <p className="transfer-warning">
              <Icon name="warning" size={15} />
              <span>{t("transfer.playAnywayBody", names)}</span>
            </p>
            <label className="transfer-confirm">
              <input type="checkbox" checked={understood} onChange={(event) => setUnderstood(event.target.checked)} />
              <span>{t("transfer.playAnywayCheck")}</span>
            </label>
            {force.error ? <ErrorNotice error={force.error} /> : null}
          </div>
        </Dialog>
      </section>
    );
  }

  if (instance.forked_from) {
    const fork = instance.forked_from;
    return (
      <section className="transfer-banner" data-tone="forked" role="status">
        <Icon name="warning" />
        <div className="transfer-banner-text">
          <p className="transfer-banner-title">
            {t("transfer.forkedTitle", { node: fork.node_name, date: formatDateTime(fork.at, language) })}
          </p>
          <p className="transfer-banner-body">{t("transfer.forkedBody", { node: fork.node_name })}</p>
        </div>
      </section>
    );
  }

  return null;
}
