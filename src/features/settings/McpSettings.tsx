/**
 * AI assistants (MCP): lets Claude, Cursor or any MCP client manage this
 * machine's servers through a local, token-protected endpoint.
 *
 * Off by default. When on, the card shows the endpoint, the token (hidden
 * until asked) and ready-to-paste setup for the common clients.
 */

import { useEffect, useId, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import type { McpStatus } from "@/types/api";
import { Button, ErrorNotice, Icon, Panel, Segmented, Switch } from "@/ui";

import "./McpSettings.css";

const KEY = ["mcp", "status"] as const;

type Client = "claudeCode" | "claudeDesktop" | "cursor";

function snippet(client: Client, url: string, token: string): string {
  switch (client) {
    case "claudeCode":
      return `claude mcp add --transport http nebula-host ${url} --header "Authorization: Bearer ${token}"`;
    case "claudeDesktop":
      return JSON.stringify(
        {
          mcpServers: {
            "nebula-host": {
              command: "npx",
              args: ["-y", "mcp-remote", url, "--header", "Authorization:${NEBULA_AUTH}"],
              env: { NEBULA_AUTH: `Bearer ${token}` },
            },
          },
        },
        null,
        2,
      );
    case "cursor":
      return JSON.stringify(
        { mcpServers: { "nebula-host": { url, headers: { Authorization: `Bearer ${token}` } } } },
        null,
        2,
      );
  }
}

function useCopy() {
  const [copied, setCopied] = useState<string | null>(null);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(null), 1400);
    return () => window.clearTimeout(timer);
  }, [copied]);
  const copy = (id: string, text: string) => {
    void navigator.clipboard.writeText(text).then(() => setCopied(id));
  };
  return { copied, copy };
}

export function McpSettings() {
  const { t } = useI18n();
  const client = useQueryClient();
  const status = useQuery({ queryKey: KEY, queryFn: api.mcp.status });
  const [reveal, setReveal] = useState(false);
  const [target, setTarget] = useState<Client>("claudeCode");
  const { copied, copy } = useCopy();
  const helpId = useId();
  const readOnlyHelpId = useId();

  const set = (data: McpStatus) => client.setQueryData(KEY, data);
  const configure = useMutation({
    mutationFn: (next: { enabled: boolean; readOnly: boolean }) => api.mcp.configure(next.enabled, next.readOnly),
    onSuccess: set,
  });
  const regenerate = useMutation({ mutationFn: api.mcp.regenerateToken, onSuccess: set });

  const data = status.data;
  const token = data?.token ?? "";
  const masked = token ? `${token.slice(0, 6)}${"•".repeat(18)}${token.slice(-4)}` : "";

  return (
    <Panel title={t("mcp.title")}>
      <p className="mcp-intro">{t("mcp.intro")}</p>

      {status.error ? <ErrorNotice error={status.error} compact /> : null}

      <div className="settings-row" data-switch="">
        <label className="settings-row-hit">
          <span className="settings-row-text">
            <span className="settings-row-label">{t("mcp.enable")}</span>
            <span id={helpId} className="settings-row-help">
              {t("mcp.enableHelp")}
            </span>
          </span>
          <span className="settings-row-control">
            <Switch
              checked={data?.enabled ?? false}
              disabled={!data || configure.isPending}
              onChange={(enabled) => configure.mutate({ enabled, readOnly: data?.readOnly ?? false })}
              label={t("mcp.enable")}
              describedBy={helpId}
            />
          </span>
        </label>
      </div>

      {data?.enabled ? (
        <>
          <div className="settings-row" data-switch="">
            <label className="settings-row-hit">
              <span className="settings-row-text">
                <span className="settings-row-label">{t("mcp.readOnly")}</span>
                <span id={readOnlyHelpId} className="settings-row-help">
                  {t("mcp.readOnlyHelp")}
                </span>
              </span>
              <span className="settings-row-control">
                <Switch
                  checked={data.readOnly}
                  disabled={configure.isPending}
                  onChange={(readOnly) => configure.mutate({ enabled: true, readOnly })}
                  label={t("mcp.readOnly")}
                  describedBy={readOnlyHelpId}
                />
              </span>
            </label>
          </div>

          <div className="mcp-status" data-running={data.running ? "" : undefined}>
            <span className="mcp-status-dot" aria-hidden="true" />
            <span>
              {data.running ? t("mcp.running") : t("mcp.notRunning")}
              {data.error ? ` — ${data.error}` : ""}
            </span>
          </div>

          <dl className="mcp-fields">
            <div className="mcp-field">
              <dt className="label">{t("mcp.endpoint")}</dt>
              <dd>
                <code className="mcp-code selectable">{data.url}</code>
                <Button size="sm" variant="quiet" icon={copied === "url" ? "check" : "copy"} onClick={() => copy("url", data.url)}>
                  {copied === "url" ? t("common.copied") : t("mcp.copy")}
                </Button>
              </dd>
            </div>
            <div className="mcp-field">
              <dt className="label">{t("mcp.token")}</dt>
              <dd>
                <code className="mcp-code selectable">{reveal ? token : masked}</code>
                <Button size="sm" variant="quiet" onClick={() => setReveal((v) => !v)}>
                  {reveal ? t("mcp.hide") : t("mcp.show")}
                </Button>
                <Button size="sm" variant="quiet" icon={copied === "token" ? "check" : "copy"} onClick={() => copy("token", token)}>
                  {copied === "token" ? t("common.copied") : t("mcp.copy")}
                </Button>
                <Button size="sm" variant="quiet" disabled={regenerate.isPending} onClick={() => regenerate.mutate()}>
                  {t("mcp.regenerate")}
                </Button>
              </dd>
            </div>
          </dl>
          <p className="mcp-note">
            <Icon name="warning" size={12} />
            <span>{t("mcp.tokenWarning")}</span>
          </p>

          <div className="mcp-setup">
            <div className="mcp-setup-head">
              <span className="label">{t("mcp.setup")}</span>
              <Segmented<Client>
                size="sm"
                label={t("mcp.setup")}
                value={target}
                onChange={setTarget}
                options={[
                  { id: "claudeCode", label: "Claude Code" },
                  { id: "claudeDesktop", label: "Claude Desktop" },
                  { id: "cursor", label: t("mcp.otherClients") },
                ]}
              />
            </div>
            <p className="mcp-setup-help">{t(`mcp.setupHelp_${target}`)}</p>
            <div className="mcp-snippet">
              <pre className="mcp-pre selectable">{snippet(target, data.url, reveal ? token : masked)}</pre>
              <Button
                size="sm"
                icon={copied === "snippet" ? "check" : "copy"}
                onClick={() => copy("snippet", snippet(target, data.url, token))}
              >
                {copied === "snippet" ? t("common.copied") : t("mcp.copy")}
              </Button>
            </div>
          </div>

          <details className="mcp-tools">
            <summary>{t("mcp.toolsTitle")}</summary>
            <p>{t("mcp.toolsList")}</p>
          </details>
        </>
      ) : null}

      {configure.error || regenerate.error ? (
        <ErrorNotice error={configure.error ?? regenerate.error} compact />
      ) : null}
    </Panel>
  );
}
