/**
 * Settings › Remote access (this PC accepting other devices) and the UPnP
 * self-test shown under the "forward ports" switch.
 */

import { useEffect, useId, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { keys, useNode } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import type { RemoteStatus, UpnpCheck } from "@/types/api";
import { Button, ErrorNotice, Icon, Panel, Switch } from "@/ui";

import "./RemoteSettings.css";

const REMOTE_KEY = ["remote", "status"] as const;

export function RemoteSettings() {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const helpId = useId();
  const status = useQuery({ queryKey: REMOTE_KEY, queryFn: api.remote.status });
  const { data: node } = useNode();
  const [code, setCode] = useState<{ code: string; expires_at: string } | null>(null);

  const configure = useMutation({
    mutationFn: api.remote.configure,
    onSuccess: (data: RemoteStatus) => client.setQueryData(REMOTE_KEY, data),
  });
  const issue = useMutation({ mutationFn: api.node.pairingCode, onSuccess: setCode });
  const forget = useMutation({
    mutationFn: api.remote.forgetDevice,
    onSuccess: () => void client.invalidateQueries({ queryKey: keys.node }),
  });

  // Expire the shown code on screen when it expires on the node.
  useEffect(() => {
    if (!code) return;
    const ms = new Date(code.expires_at).getTime() - Date.now();
    const timer = window.setTimeout(() => setCode(null), Math.max(0, ms));
    return () => window.clearTimeout(timer);
  }, [code]);

  const data = status.data;
  const devices = node?.paired_devices ?? [];
  const date = (iso: string) => new Intl.DateTimeFormat(language, { dateStyle: "medium" }).format(new Date(iso));

  return (
    <Panel title={t("remote.accessTitle")}>
      <p className="remote-intro">{t("remote.accessIntro")}</p>

      <div className="settings-row" data-switch="">
        <label className="settings-row-hit">
          <span className="settings-row-text">
            <span className="settings-row-label">{t("remote.accessEnable")}</span>
            <span id={helpId} className="settings-row-help" data-warn="">
              <Icon name="warning" size={12} />
              <span>{t("remote.accessEnableHelp", { port: data?.port ?? 47400 })}</span>
            </span>
          </span>
          <span className="settings-row-control">
            <Switch
              checked={data?.enabled ?? false}
              disabled={!data || configure.isPending}
              onChange={(enabled) => configure.mutate(enabled)}
              label={t("remote.accessEnable")}
              describedBy={helpId}
            />
          </span>
        </label>
      </div>

      {data?.enabled ? (
        <div className="remote-body">
          <div className="remote-status" data-running={data.running ? "" : undefined}>
            <span className="remote-dot" aria-hidden="true" />
            <span>
              {data.running
                ? t("remote.accessRunning", { address: data.addresses[0] ?? `:${data.port}` })
                : t("remote.accessNotRunning")}
              {data.error ? ` — ${data.error}` : ""}
            </span>
          </div>

          <div className="remote-code-row">
            <Button icon="link" disabled={!data.running || issue.isPending} onClick={() => issue.mutate()}>
              {t("remote.newCode")}
            </Button>
            {code ? (
              <div className="remote-code" role="status">
                <span className="remote-code-digits selectable">
                  {code.code.slice(0, 3)} {code.code.slice(3)}
                </span>
                <span className="remote-code-help">{t("remote.codeExpires")}</span>
              </div>
            ) : null}
          </div>

          <div className="remote-devices">
            <span className="label">{t("remote.devices")}</span>
            {devices.length === 0 ? (
              <p className="remote-empty">{t("remote.noDevices")}</p>
            ) : (
              <ul>
                {devices.map((device) => (
                  <li key={device.id}>
                    <span className="remote-device-name">{device.name}</span>
                    <span className="remote-device-date">{t("remote.pairedAt", { date: date(device.paired_at) })}</span>
                    <Button size="sm" variant="danger" disabled={forget.isPending} onClick={() => forget.mutate(device.id)}>
                      {t("remote.removeDevice")}
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      ) : null}

      {status.error || configure.error || issue.error || forget.error ? (
        <ErrorNotice error={status.error ?? configure.error ?? issue.error ?? forget.error} compact />
      ) : null}
    </Panel>
  );
}

/** The "Test UPnP" button and its verdict, shown under the forwarding switch. */
export function UpnpTest() {
  const { t } = useI18n();
  const test = useMutation({ mutationFn: api.upnp.check });
  const result: UpnpCheck | undefined = test.data;

  const verdicts: { tone: "ok" | "warn" | "bad"; text: string }[] = [];
  if (result) {
    if (!result.routerFound) verdicts.push({ tone: "bad", text: t("upnp.upnpNoRouter") });
    else if (!result.mappingWorks) verdicts.push({ tone: "bad", text: t("upnp.upnpRefused") });
    else verdicts.push({ tone: "ok", text: t("upnp.upnpOk") });
    if (result.externalKind === "cgnat") verdicts.push({ tone: "warn", text: t("upnp.upnpCgnat", { ip: result.externalIp ?? "" }) });
    if (result.externalKind === "private") verdicts.push({ tone: "warn", text: t("upnp.upnpDoubleNat", { ip: result.externalIp ?? "" }) });
    if (result.externalKind === "public" && result.externalIp) verdicts.push({ tone: "ok", text: t("upnp.upnpPublic", { ip: result.externalIp }) });
  }

  return (
    <div className="upnp-test">
      <p className="upnp-note">{t("upnp.upnpNote")}</p>
      <div className="upnp-actions">
        <Button size="sm" icon="globe" disabled={test.isPending} onClick={() => test.mutate()}>
          {test.isPending ? t("upnp.upnpTesting") : t("upnp.upnpTest")}
        </Button>
      </div>
      {verdicts.length ? (
        <ul className="upnp-verdicts" aria-live="polite">
          {verdicts.map((verdict) => (
            <li key={verdict.text} data-tone={verdict.tone}>
              <Icon name={verdict.tone === "ok" ? "check" : "warning"} size={13} />
              <span>{verdict.text}</span>
            </li>
          ))}
          {result?.detail && !result.mappingWorks ? <li className="upnp-detail">{result.detail}</li> : null}
        </ul>
      ) : null}
      {test.error ? <ErrorNotice error={test.error} compact /> : null}
    </div>
  );
}
