/**
 * The foot of Settings: Diagnostics (the app's own log — of the machine being
 * managed, so it works the same for a Raspberry Pi over a remote connection)
 * and About (version, author, community links).
 */

import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { activeRemote } from "@/lib/connection";
import { openExternal } from "@/lib/external";
import { api } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, Icon, Mark, type IconName } from "@/ui";

import { LICENCE_GROUPS } from "./licences";
import "./AboutSettings.css";

const DISCORD_URL = "https://discord.com/invite/s5w2HEGvdX";
const SITE_URL = "https://g2806.net/apps";


export function DiagnosticsSettings() {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const remote = activeRemote();
  const openFolder = useMutation({
    mutationFn: () => api.app.openLogFolder(),
  });

  return (
    <section className="about-section" aria-labelledby="diag-title">
      <h2 id="diag-title" className="about-heading">
        <Icon name="activity" size={16} /> {t("about.diagnostics")}
      </h2>
      <div className="about-card about-row">
        <span className="about-row-icon" aria-hidden="true">
          <Icon name="activity" size={20} />
        </span>
        <span className="about-row-text">
          <span className="about-row-title">{t("about.logsTitle")}</span>
          <span className="about-row-help">
            {remote ? t("about.logsHelpRemote", { name: remote.name }) : t("about.logsHelp")}
          </span>
        </span>
        <span className="about-row-actions">
          {remote ? null : (
            <Button size="sm" variant="quiet" icon="folder" disabled={openFolder.isPending} onClick={() => openFolder.mutate()}>
              {t("about.openFolder")}
            </Button>
          )}
          <Button size="sm" onClick={() => setOpen(true)}>
            {t("about.viewLogs")}
          </Button>
        </span>
      </div>
      {openFolder.error ? <ErrorNotice error={openFolder.error} compact /> : null}
      {open ? <LogDialog onClose={() => setOpen(false)} /> : null}
    </section>
  );
}

function LogDialog({ onClose }: { onClose: () => void }) {
  const { t } = useI18n();
  const log = useQuery({ queryKey: ["app-log"], queryFn: () => api.app.log(400), refetchInterval: 4000 });
  const [copied, setCopied] = useState(false);
  const header = log.data ? `Nebula Host ${log.data.version} · ${log.data.platform}\n${log.data.path ?? ""}\n\n` : "";

  return (
    <Dialog
      open
      onClose={onClose}
      title={t("about.logsTitle")}
      width={880}
      footer={
        <>
          <Button onClick={onClose}>{t("dialog.close")}</Button>
          <Button
            variant="primary"
            icon={copied ? "check" : "copy"}
            disabled={!log.data}
            onClick={() => {
              void navigator.clipboard.writeText(header + (log.data?.text ?? "")).then(() => {
                setCopied(true);
                window.setTimeout(() => setCopied(false), 1500);
              });
            }}
          >
            {copied ? t("common.copied") : t("about.copyLog")}
          </Button>
        </>
      }
    >
      {log.error ? <ErrorNotice error={log.error} compact /> : null}
      <p className="about-log-meta">
        {log.data ? `Nebula Host ${log.data.version} · ${log.data.platform}` : ""}
        {log.data?.path ? <code className="selectable">{log.data.path}</code> : null}
      </p>
      <pre className="about-log selectable">{log.data ? log.data.text || t("about.logEmpty") : "…"}</pre>
    </Dialog>
  );
}

export function AboutSettings() {
  const { t } = useI18n();
  const remote = activeRemote();
  // This app's version; when managing another machine, its version too.
  const remoteVersion = useQuery({
    queryKey: ["app-log", "version"],
    queryFn: () => api.app.log(1),
    enabled: remote !== null,
  });

  return (
    <section className="about-section" aria-labelledby="about-title">
      <h2 id="about-title" className="about-heading">
        <Icon name="info" size={16} /> {t("about.title")}
      </h2>
      <div className="about-card about-hero">
        <Mark size={64} />
        <span className="about-name">Nebula Host</span>
        <span className="about-tagline">{t("about.tagline")}</span>
        <span className="about-version">
          {t("about.version", { version: __APP_VERSION__ })}
          {remote && remoteVersion.data ? ` · ${remote.name} ${remoteVersion.data.version}` : ""}
        </span>
        <span className="about-sep" aria-hidden="true" />
        <span className="about-author">{t("about.createdBy")} Gabrieli2806</span>
      </div>
      <LinkRow icon="chat" title="Discord" help={t("about.discordHelp")} url={DISCORD_URL} />
      <LinkRow icon="globe" title="G2806.net" help={t("about.siteHelp")} url={SITE_URL} />
      <LegalSettings />
    </section>
  );
}

/**
 * The affiliation disclaimer the Minecraft Usage Guidelines ask for, the
 * trademark notices, and the open-source licences.
 */
function LegalSettings() {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <>
      <h3 className="about-heading">
        <Icon name="file" size={16} /> {t("about.legal")}
      </h3>
      <div className="about-card about-legal">
        <p className="about-disclaimer">{t("about.disclaimer")}</p>
        <p className="about-legal-text">{t("about.trademarks")}</p>
        <p className="about-legal-text">{t("about.glyphs")}</p>
      </div>
      <div className="about-card about-row">
        <span className="about-row-icon" aria-hidden="true">
          <Icon name="file" size={20} />
        </span>
        <span className="about-row-text">
          <span className="about-row-title">{t("about.licences")}</span>
          <span className="about-row-help">{t("about.licencesHelp")}</span>
        </span>
        <span className="about-row-actions">
          <Button size="sm" variant="quiet" icon="file" onClick={() => setOpen(true)}>
            {t("about.licencesView")}
          </Button>
        </span>
      </div>
      {open ? <LicencesDialog onClose={() => setOpen(false)} /> : null}
    </>
  );
}

function LicencesDialog({ onClose }: { onClose: () => void }) {
  const { t } = useI18n();
  return (
    <Dialog
      open
      onClose={onClose}
      title={t("about.licences")}
      width={620}
      footer={<Button onClick={onClose}>{t("dialog.close")}</Button>}
    >
      <div className="flex flex-col gap-4">
        {LICENCE_GROUPS.map((group) => (
          <section key={group.key} className="about-licence-group">
            <h4 className="about-licence-heading">{t(`about.licencesGroup.${group.key}`)}</h4>
            <ul className="about-licence-list">
              {group.entries.map((entry) => (
                <li key={entry.name} className="about-licence-item">
                  <button type="button" className="about-licence-name" onClick={() => void openExternal(entry.url)}>
                    {entry.name}
                    <Icon name="external" size={12} />
                  </button>
                  <span className="about-licence-kind">{entry.licence}</span>
                </li>
              ))}
            </ul>
          </section>
        ))}
        <p className="about-legal-text">{t("about.licencesNote")}</p>
      </div>
    </Dialog>
  );
}

function LinkRow({ icon, title, help, url }: { icon: IconName; title: string; help: string; url: string }) {
  const { t } = useI18n();
  return (
    <div className="about-card about-row">
      <span className="about-row-icon" aria-hidden="true">
        <Icon name={icon} size={20} />
      </span>
      <span className="about-row-text">
        <span className="about-row-title">{title}</span>
        <span className="about-row-help">{help}</span>
      </span>
      <span className="about-row-actions">
        <Button size="sm" variant="quiet" icon="external" onClick={() => void openExternal(url)}>
          {t("about.open")}
        </Button>
      </span>
    </div>
  );
}
