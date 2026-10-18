/**
 * The game's agreement (the Minecraft EULA): the consent checkbox, and the
 * dialog that asks for it when a server without it is started.
 *
 * The checkbox starts unticked, every time. The link opens the full text in
 * the browser. Acceptance is recorded per server (for Minecraft, in its
 * eula.txt) only after the user ticks it; see src/lib/agreement.ts.
 */

import { useEffect, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { keys, useGames, useInstances } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { clearAgreementRequest, requestAgreement, useAgreementRequest, type AgreementRequest } from "@/lib/agreement";
import { activeRemote } from "@/lib/connection";
import { openExternal } from "@/lib/external";
import { api, onTrayRefused } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, Icon } from "@/ui";

import "./Agreement.css";

export function AgreementConsent({
  gameName,
  url,
  checked,
  onChange,
}: {
  gameName: string;
  url: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  const { t } = useI18n();
  return (
    <div className="agreement">
      <p className="agreement-intro">{t("agreement.intro", { game: gameName })}</p>
      <div className="agreement-row">
        <label className="agreement-check">
          <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
          <span>{t("agreement.accept")}</span>
        </label>
        <button type="button" className="agreement-link" onClick={() => void openExternal(url)}>
          {t("agreement.read")}
          <Icon name="external" size={12} />
        </button>
      </div>
    </div>
  );
}

/** Mounted once (App). Shows itself when a start or sleep needs the agreement. */
export function AgreementGate() {
  const request = useAgreementRequest();
  // Start from the tray menu: it acts on this PC, so it is only answered
  // here while the window manages this PC too.
  useEffect(
    () =>
      onTrayRefused((refusal) => {
        if (activeRemote()) return;
        if (refusal.reason === "agreement_required") {
          requestAgreement({ instanceId: refusal.instanceId, url: refusal.url ?? "", resume: "start" });
        } else {
          window.location.hash = `#/servers/${refusal.instanceId}/overview`;
        }
      }),
    [],
  );
  // Keyed by the request, so a second request starts unticked again.
  return request ? <AgreementDialog key={`${request.instanceId}:${request.resume}`} request={request} /> : null;
}

function AgreementDialog({ request }: { request: AgreementRequest }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const { data: instances } = useInstances();
  const { data: games } = useGames();
  const [accepted, setAccepted] = useState(false);

  const instance = instances?.find((candidate) => candidate.id === request.instanceId);
  const game = games?.find((candidate) => candidate.id === instance?.game);
  const gameName = game?.name ?? "";
  const url = request.url || game?.agreement?.url || "";

  const proceed = useMutation({
    mutationFn: async () => {
      await api.instances.acceptAgreement(request.instanceId);
      if (request.resume === "sleep") await api.instances.sleep(request.instanceId);
      else await api.instances.start(request.instanceId);
    },
    onSuccess: () => clearAgreementRequest(),
    onSettled: () => client.invalidateQueries({ queryKey: keys.instances }),
  });

  return (
    <Dialog
      open
      onClose={() => {
        if (!proceed.isPending) clearAgreementRequest();
      }}
      title={t("agreement.gateTitle", { name: instance?.name ?? "" })}
      width={500}
      footer={
        <>
          <Button variant="quiet" disabled={proceed.isPending} onClick={() => clearAgreementRequest()}>
            {t("actions.cancel")}
          </Button>
          <Button variant="primary" disabled={!accepted || proceed.isPending} onClick={() => proceed.mutate()}>
            {request.resume === "sleep" ? t("agreement.acceptSleep") : t("agreement.acceptStart")}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <AgreementConsent gameName={gameName} url={url} checked={accepted} onChange={setAccepted} />
        <p className="agreement-note">{t("agreement.savedPerServer")}</p>
        {proceed.error ? <ErrorNotice error={proceed.error} /> : null}
      </div>
    </Dialog>
  );
}
