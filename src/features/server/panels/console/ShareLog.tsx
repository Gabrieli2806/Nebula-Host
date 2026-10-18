/**
 * "Share log": put a log on mclo.gs and hand back a link, the way the
 * Minecraft community asks for logs. Three sources: what the console shows,
 * the game's own latest log, and recent crash reports.
 *
 * Nothing is uploaded without a yes. Logs carry player names and IP
 * addresses, and mclo.gs is public; the confirmation says so once, and can
 * be told not to ask again (remembered on this machine).
 */

import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatBytes } from "@/lib/format";
import { api } from "@/lib/ipc";
import type { LogSource, SharedLog } from "@/types/api";
import { Button, Dialog, ErrorNotice, Icon } from "@/ui";

import { openExternal, relativeDate } from "../addons/util";

const SKIP_CONFIRM_KEY = "nebula.mclogsNoConfirm";

function confirmSkipped(): boolean {
  try {
    return window.localStorage.getItem(SKIP_CONFIRM_KEY) === "1";
  } catch {
    return false;
  }
}

function skipConfirm() {
  try {
    window.localStorage.setItem(SKIP_CONFIRM_KEY, "1");
  } catch {
    // Storage can be blocked; then it asks again next time, which is safe.
  }
}

export function ShareLog({ instanceId, consoleLines }: { instanceId: string; consoleLines: number }) {
  const { t, language } = useI18n();
  const [open, setOpen] = useState(false);
  const [confirming, setConfirming] = useState<LogSource | null>(null);
  const [dontAsk, setDontAsk] = useState(false);
  const [shared, setShared] = useState<{ log: SharedLog; copied: boolean } | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  // Asked for when the menu opens, so the list is current (a crash report
  // written a moment ago shows up).
  const files = useQuery({
    queryKey: keys.logFiles(instanceId),
    queryFn: () => api.instances.logFiles(instanceId),
    enabled: open,
    staleTime: 0,
  });

  const upload = useMutation({
    mutationFn: (source: LogSource) => api.instances.shareLog(instanceId, source),
    onSuccess: async (log) => {
      setConfirming(null);
      setShared({ log, copied: await copy(log.url) });
    },
  });

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => event.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  const choose = (source: LogSource) => {
    setOpen(false);
    upload.reset();
    if (confirmSkipped()) {
      upload.mutate(source);
    } else {
      setDontAsk(false);
      setConfirming(source);
    }
  };

  const confirm = () => {
    if (!confirming) return;
    if (dontAsk) skipConfirm();
    upload.mutate(confirming);
  };

  const latest = files.data?.latest;
  const crashes = files.data?.crash_reports ?? [];
  const nothing = !files.isLoading && consoleLines === 0 && !latest && crashes.length === 0;

  return (
    <div className="share-log" ref={menuRef}>
      <Button
        variant="quiet"
        size="sm"
        icon="upload"
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={upload.isPending}
        onClick={() => setOpen((value) => !value)}
      >
        {upload.isPending && !confirming ? t("console.shareUploading") : t("console.share")}
      </Button>

      {open ? (
        <div className="share-log-menu" role="menu" aria-label={t("console.shareMenu")}>
          <p className="share-log-heading">{t("console.shareMenu")}</p>
          {consoleLines > 0 ? (
            <MenuItem
              icon="terminal"
              title={t("console.shareConsole")}
              detail={t("console.shareConsoleHint")}
              onSelect={() => choose({ kind: "console" })}
            />
          ) : null}
          {latest ? (
            <MenuItem
              icon="file"
              title={t("console.shareLatest")}
              detail={[latest.name, formatBytes(latest.size_bytes), relativeDate(latest.modified, language)]
                .filter(Boolean)
                .join(" · ")}
              onSelect={() => choose({ kind: "latest" })}
            />
          ) : null}
          {crashes.length > 0 ? (
            <>
              <p className="share-log-heading">{t("console.shareCrashes")}</p>
              {crashes.map((report) => (
                <MenuItem
                  key={report.name}
                  icon="warning"
                  title={relativeDate(report.modified, language) ?? t("console.shareCrash")}
                  detail={report.name}
                  mono
                  onSelect={() => choose({ kind: "crash_report", name: report.name })}
                />
              ))}
            </>
          ) : null}
          {files.isLoading ? <p className="share-log-empty">…</p> : null}
          {nothing ? <p className="share-log-empty">{t("console.shareNothing")}</p> : null}
          {files.error ? <ErrorNotice error={files.error} compact className="m-1" /> : null}
        </div>
      ) : null}

      {upload.error && !confirming ? (
        <div className="share-log-error">
          <ErrorNotice error={upload.error} compact />
        </div>
      ) : null}

      <Dialog
        open={confirming !== null}
        onClose={() => {
          setConfirming(null);
          upload.reset();
        }}
        title={t("console.shareConfirmTitle")}
        width={480}
        footer={
          <>
            <label className="share-log-dontask">
              <input type="checkbox" checked={dontAsk} onChange={(event) => setDontAsk(event.target.checked)} />
              <span>{t("console.shareDontAsk")}</span>
            </label>
            <Button
              variant="quiet"
              onClick={() => {
                setConfirming(null);
                upload.reset();
              }}
            >
              {t("actions.cancel")}
            </Button>
            <Button variant="primary" icon="upload" disabled={upload.isPending} onClick={confirm}>
              {upload.isPending ? t("console.shareUploading") : t("console.shareUpload")}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <p>{t("console.shareConfirmBody")}</p>
          <p className="share-log-privacy">
            <Icon name="warning" size={14} />
            <span>{t("console.shareConfirmPrivacy")}</span>
          </p>
          {upload.error ? <ErrorNotice error={upload.error} /> : null}
        </div>
      </Dialog>

      <Dialog
        open={shared !== null}
        onClose={() => setShared(null)}
        title={t("console.sharedTitle")}
        width={480}
        footer={
          <>
            <Button
              variant="quiet"
              icon={shared?.copied ? "check" : "copy"}
              onClick={async () => shared && setShared({ ...shared, copied: await copy(shared.log.url) })}
            >
              {shared?.copied ? t("console.copied") : t("console.copyLink")}
            </Button>
            <Button variant="primary" icon="external" onClick={() => shared && void openExternal(shared.log.url)}>
              {t("console.openInBrowser")}
            </Button>
          </>
        }
      >
        {shared ? (
          <div className="flex flex-col gap-3">
            <p className="text-[color:var(--text-muted)]">
              {shared.copied ? t("console.sharedBody") : t("console.sharedNotCopied")}
            </p>
            <input
              className="field mono selectable"
              readOnly
              value={shared.log.url}
              aria-label={t("console.copyLink")}
              onFocus={(event) => event.currentTarget.select()}
            />
            {shared.log.omitted_lines > 0 ? (
              <p className="pref-hint">
                <Icon name="warning" size={12} />
                {t("console.sharedTrimmed", { count: shared.log.omitted_lines.toLocaleString(language) })}
              </p>
            ) : null}
          </div>
        ) : null}
      </Dialog>
    </div>
  );
}

function MenuItem({
  icon,
  title,
  detail,
  mono = false,
  onSelect,
}: {
  icon: "terminal" | "file" | "warning";
  title: string;
  detail?: string;
  mono?: boolean;
  onSelect: () => void;
}) {
  return (
    <button type="button" role="menuitem" className="share-log-item" onClick={onSelect}>
      <Icon name={icon} size={14} />
      <span className="share-log-text">
        <span className="share-log-title">{title}</span>
        {detail ? <span className={`share-log-detail ${mono ? "mono" : ""}`}>{detail}</span> : null}
      </span>
    </button>
  );
}

/** Copy to the clipboard; false when the webview refuses. */
async function copy(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
