/** The game's log for an instance, with sharing on mclo.gs. */

import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { openExternal } from "@/lib/external";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, CopyAddress, Dialog, ErrorNotice } from "@/ui";
import type { LauncherProfile } from "./types";

export function InstanceLog({ profile }: { profile: LauncherProfile }) {
  const { t } = useI18n();
  const logs = useQuery({ queryKey: ["launcher", "logs", profile.id], queryFn: () => launcherApi.logs(profile.id), refetchInterval: 1500 });
  const [sharing, setSharing] = useState(false);
  const share = useMutation({ mutationFn: () => launcherApi.shareLog(profile.id) });

  return (
    <section className="launcher-log" aria-label={t("launcher.gameLogs")}>
      <div className="launcher-log-bar">
        <span className="launcher-muted">{t("launcher.logHint")}</span>
        <Button size="sm" icon="upload" onClick={() => { share.reset(); setSharing(true); }}>{t("launcher.shareLog")}</Button>
      </div>
      {logs.error ? <ErrorNotice error={toAppError(logs.error)} compact /> : null}
      <pre className="launcher-game-log selectable mono">{logs.data?.join("\n") || t("launcher.noLogs")}</pre>
      {sharing ? (
        <Dialog
          open
          onClose={() => setSharing(false)}
          dismissable={!share.isPending}
          title={t("launcher.shareLog")}
          footer={share.data ? (
            <>
              <Button onClick={() => setSharing(false)}>{t("launcher.close")}</Button>
              <Button variant="primary" icon="external" onClick={() => void openExternal(share.data!.url)}>{t("launcher.openLink")}</Button>
            </>
          ) : (
            <>
              <Button variant="quiet" disabled={share.isPending} onClick={() => setSharing(false)}>{t("launcher.cancel")}</Button>
              <Button variant="primary" icon="upload" disabled={share.isPending} onClick={() => share.mutate()}>
                {share.isPending ? t("launcher.uploading") : t("launcher.upload")}
              </Button>
            </>
          )}
        >
          {share.data ? (
            <div className="flex flex-col gap-3"><p>{t("launcher.logShared")}</p><CopyAddress value={share.data.url} /></div>
          ) : (
            <p>{t("launcher.shareLogWarning")}</p>
          )}
          {share.error ? <ErrorNotice error={toAppError(share.error)} compact /> : null}
        </Dialog>
      ) : null}
    </section>
  );
}
