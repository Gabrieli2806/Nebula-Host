/**
 * AI agents (MCP): lets Claude Code, Codex or any MCP client manage this
 * machine's servers and launcher, and those of the machines paired with it,
 * through an endpoint only programs on this PC can reach.
 *
 * The card is one row that stays folded: its name, its state and the switch.
 * Opened, it shows how to connect an agent (one prompt to paste in its chat),
 * the address, manual setup, what agents may do, and what they did last.
 *
 * The rows, their order and the per-client snippets are the same in every
 * app of the author's: a shared contract, which `CLAUDE.md` points to.
 */

import { useEffect, useId, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { remoteNodes, subscribeNodes } from "@/lib/connection";
import { api } from "@/lib/ipc";
import { DESKTOP_FEATURES } from "@/lib/web";
import type { AppSettings, McpActivity, McpStatus } from "@/types/api";
import { Button, ErrorNotice, Icon, Panel, Segmented, Switch } from "@/ui";

import "./McpSettings.css";

const KEY = ["mcp", "status"] as const;
const NAME = "nebula-host";

type Client = "claudeCode" | "codex" | "claudeDesktop" | "other";

/** What to type, or paste into a config file, per client. */
function snippet(client: Client, url: string): string {
  switch (client) {
    case "claudeCode":
      return `claude mcp add --transport http --scope user ${NAME} ${url}`;
    case "codex":
      return `codex mcp add ${NAME} --url ${url}`;
    case "claudeDesktop":
      return JSON.stringify({ mcpServers: { [NAME]: { command: "npx", args: ["-y", "mcp-remote", url] } } });
    case "other":
      return JSON.stringify({ mcpServers: { [NAME]: { type: "http", url } } });
  }
}

/** The prompt to paste in an agent's chat: it connects itself. Written for
 *  the agent, in English whatever the app's language. */
function connectPrompt(url: string): string {
  return [
    "Connect to my Nebula Host app over MCP. Nebula Host runs on this PC: it manages my game servers (Minecraft) and my Minecraft launcher instances, here and on the machines I paired with it.",
    "",
    `Add an MCP server named "${NAME}" with the Streamable HTTP transport at ${url}. It needs no token and no headers; it only answers programs on this PC.`,
    "",
    `- Claude Code: run \`${snippet("claudeCode", url)}\``,
    `- Codex: run \`${snippet("codex", url)}\``,
    `- Any other client: add this to its MCP configuration: ${snippet("other", url)}`,
    "",
    "If your client needs a reload to see new MCP servers, tell me. Once connected, call list_servers and list_machines and tell me what you can see.",
  ].join("\n");
}

function useCopy() {
  const [copied, setCopied] = useState<string | null>(null);
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(null), 1600);
    return () => window.clearTimeout(timer);
  }, [copied]);
  const copy = (id: string, text: string) => {
    void navigator.clipboard.writeText(text).then(() => setCopied(id));
  };
  return { copied, copy };
}

/**
 * Keep the backend's copy of the paired machines in step with this window's,
 * so an agent connected to this PC can reach them. Mounted once, by the app.
 */
export function useMachinesSync() {
  useEffect(() => {
    if (!DESKTOP_FEATURES) return;
    const sync = () => {
      const machines = remoteNodes().map(({ nodeId, name, address, token }) => ({ nodeId, name, address, token }));
      void api.mcp.syncMachines(machines).catch(() => undefined);
    };
    sync();
    return subscribeNodes(sync);
  }, []);
}

export function McpSettings() {
  const { t } = useI18n();
  const client = useQueryClient();
  // Folded until asked for: one line with the name, the state and the switch.
  const [open, setOpen] = useState(false);
  const status = useQuery({
    queryKey: KEY,
    queryFn: api.mcp.status,
    // What agents are doing shows while the section is open.
    refetchInterval: open ? 3000 : false,
  });
  const helpId = useId();
  const bodyId = useId();

  const set = (data: McpStatus) => client.setQueryData(KEY, data);
  const configure = useMutation({
    mutationFn: (change: Partial<{ enabled: boolean; readOnly: boolean; launcherPlay: boolean; remote: boolean }>) =>
      api.mcp.configure({
        enabled: change.enabled ?? data?.enabled ?? false,
        readOnly: change.readOnly ?? data?.readOnly ?? false,
        launcherPlay: change.launcherPlay,
        remote: change.remote,
      }),
    onSuccess: set,
  });

  const data = status.data;
  const state = !data?.enabled
    ? t("mcp.stateOff")
    : data.running
      ? data.readOnly
        ? t("mcp.stateOnReadOnly", { url: data.url })
        : t("mcp.stateOn", { url: data.url })
      : t("mcp.stateFailed", { error: data.error ?? "" });

  return (
    <Panel title={t("mcp.title")}>
      {status.error ? <ErrorNotice error={status.error} compact /> : null}

      <div className="settings-row mcp-head" data-switch="">
        <label className="settings-row-hit">
          <span className="settings-row-text">
            <span className="settings-row-label">{t("mcp.enable")}</span>
            <span
              id={helpId}
              className="settings-row-help"
              data-warn={data?.enabled && !data.running ? "" : undefined}
            >
              {data?.enabled && !data.running ? <Icon name="warning" size={12} /> : null}
              <span>{state}</span>
            </span>
          </span>
          <span className="settings-row-control">
            <Switch
              checked={data?.enabled ?? false}
              disabled={!data || configure.isPending}
              onChange={(enabled) => configure.mutate({ enabled })}
              label={t("mcp.enable")}
              describedBy={helpId}
            />
          </span>
        </label>
        <button
          type="button"
          className="mcp-fold"
          aria-expanded={open}
          aria-controls={bodyId}
          aria-label={t("mcp.details")}
          title={t("mcp.details")}
          onClick={() => setOpen((value) => !value)}
        >
          <Icon name={open ? "chevron-down" : "chevron-right"} size={14} />
        </button>
      </div>

      {configure.error ? <ErrorNotice error={configure.error} compact /> : null}

      {data && open ? (
        <div id={bodyId} className="mcp-body">
          <McpDetails data={data} busy={configure.isPending} onChange={(change) => configure.mutate(change)} />
        </div>
      ) : null}
    </Panel>
  );
}

function McpDetails({
  data,
  busy,
  onChange,
}: {
  data: McpStatus;
  busy: boolean;
  onChange: (change: Partial<{ readOnly: boolean; launcherPlay: boolean; remote: boolean }>) => void;
}) {
  const { t } = useI18n();
  const [target, setTarget] = useState<Client>("claudeCode");
  const { copied, copy } = useCopy();

  return (
      <div className="mcp-details">
        <section className="mcp-section">
          <h3 className="label">{t("mcp.connect")}</h3>
          <div className="mcp-line">
            <p className="mcp-note mcp-note-grow">{t("mcp.connectHelp")}</p>
            <Button
              icon={copied === "prompt" ? "check" : "copy"}
              disabled={!data.running}
              onClick={() => copy("prompt", connectPrompt(data.url))}
            >
              {copied === "prompt" ? t("common.copied") : t("mcp.copyPrompt")}
            </Button>
          </div>
        </section>

        <section className="mcp-section">
          <h3 className="label">{t("mcp.connection")}</h3>
          <div className="mcp-status" data-running={data.running ? "" : undefined}>
            <span className="mcp-status-dot" aria-hidden="true" />
            <span>
              {data.running ? t("mcp.running") : t("mcp.notRunning")}
              {data.client ? ` · ${t("mcp.lastClient", { client: data.client })}` : ""}
              {data.error ? ` · ${data.error}` : ""}
            </span>
          </div>
          <div className="mcp-line">
            <code className="mcp-code selectable">{data.url}</code>
            <Button size="sm" variant="quiet" icon={copied === "url" ? "check" : "copy"} onClick={() => copy("url", data.url)}>
              {copied === "url" ? t("common.copied") : t("common.copy")}
            </Button>
          </div>
          <p className="mcp-note">{t("mcp.localOnly")}</p>
        </section>

        <section className="mcp-section">
          <div className="mcp-setup-head">
            <h3 className="label">{t("mcp.setup")}</h3>
            <Segmented<Client>
              size="sm"
              label={t("mcp.setup")}
              value={target}
              onChange={setTarget}
              options={[
                { id: "claudeCode", label: "Claude Code" },
                { id: "codex", label: "Codex" },
                { id: "claudeDesktop", label: "Claude Desktop" },
                { id: "other", label: t("mcp.otherClients") },
              ]}
            />
          </div>
          <p className="mcp-note">{t(`mcp.setupHelp_${target}`)}</p>
          <div className="mcp-snippet">
            <pre className="mcp-pre selectable">{snippet(target, data.url)}</pre>
            <Button
              size="sm"
              icon={copied === "snippet" ? "check" : "copy"}
              onClick={() => copy("snippet", snippet(target, data.url))}
            >
              {copied === "snippet" ? t("common.copied") : t("common.copy")}
            </Button>
          </div>
        </section>

        <section className="mcp-section">
          <h3 className="label">{t("mcp.permissions")}</h3>
          <div className="mcp-rows">
            <DetailSwitch
              label={t("mcp.readOnly")}
              help={t("mcp.readOnlyHelp")}
              checked={data.readOnly}
              disabled={busy}
              onChange={(readOnly) => onChange({ readOnly })}
            />
            <DetailSwitch
              label={t("mcp.launcherPlay")}
              help={t("mcp.launcherPlayHelp")}
              checked={data.launcherPlay && !data.readOnly}
              disabled={busy || data.readOnly}
              onChange={(launcherPlay) => onChange({ launcherPlay })}
            />
            <DetailSwitch
              label={t("mcp.remote")}
              help={t("mcp.remoteHelp")}
              checked={data.remote}
              disabled={busy}
              onChange={(remote) => onChange({ remote })}
            />
          </div>
        </section>

        <section className="mcp-section">
          <h3 className="label">{t("mcp.machines")}</h3>
          <p className="mcp-note">
            {data.machines.length
              ? t("mcp.machinesList", { machines: data.machines.join(", ") })
              : t("mcp.machinesNone")}
          </p>
        </section>

        <section className="mcp-section">
          <h3 className="label">{t("mcp.activity")}</h3>
          {data.activity.length ? (
            <ol className="mcp-activity">
              {data.activity.slice(0, 12).map((entry, index) => (
                <ActivityRow key={`${entry.at}-${index}`} entry={entry} />
              ))}
            </ol>
          ) : (
            <p className="mcp-note">{t("mcp.activityNone")}</p>
          )}
        </section>

        <details className="mcp-tools">
          <summary>{t("mcp.toolsTitle")}</summary>
          <p>{t("mcp.toolsList")}</p>
        </details>
      </div>
  );
}

function DetailSwitch({
  label,
  help,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  help: string;
  checked: boolean;
  disabled: boolean;
  onChange: (value: boolean) => void;
}) {
  const helpId = useId();
  return (
    <label className="mcp-row">
      <span className="mcp-row-text">
        <span className="mcp-row-label">{label}</span>
        <span id={helpId} className="mcp-row-help">
          {help}
        </span>
      </span>
      <Switch checked={checked} disabled={disabled} onChange={onChange} label={label} describedBy={helpId} />
    </label>
  );
}

function ActivityRow({ entry }: { entry: McpActivity }) {
  const { t, language } = useI18n();
  const time = new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit", second: "2-digit" }).format(
    new Date(entry.at),
  );
  return (
    <li className="mcp-activity-row" data-failed={entry.ok ? undefined : ""}>
      <span className="mcp-activity-time">{time}</span>
      <code className="mcp-activity-tool">{entry.tool}</code>
      <span className="mcp-activity-target">
        {entry.target ?? ""}
        {entry.machine ? ` · ${entry.machine}` : ""}
      </span>
      {entry.ok ? null : <span className="mcp-activity-failed">{t("mcp.activityFailed")}</span>}
    </li>
  );
}

/**
 * While this window manages another machine: the one agent setting that
 * machine's owner decides there. (The rest belongs to the PC an agent is
 * connected to, and is set at that PC.)
 */
export function RemoteAgentsSetting({ settings }: { settings: AppSettings }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const set = useMutation({
    mutationFn: api.mcp.setRemote,
    onSuccess: (next) => client.setQueryData(keys.settings, next),
  });
  return (
    <Panel title={t("mcp.title")}>
      <div className="mcp-rows mcp-rows-panel">
        <DetailSwitch
          label={t("mcp.remote")}
          help={t("mcp.remoteHelp")}
          checked={settings.mcpRemote ?? false}
          disabled={set.isPending}
          onChange={(enabled) => set.mutate(enabled)}
        />
      </div>
      {set.error ? <ErrorNotice error={set.error} compact /> : null}
    </Panel>
  );
}
