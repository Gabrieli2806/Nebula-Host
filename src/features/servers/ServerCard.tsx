/**
 * One server, as a card.
 *
 * The card answers the three questions someone actually opens this app to ask:
 * is it up, who is on it, and what do I tell my friends to type.
 */

import { useState } from "react";

import { useInstanceActions } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatDuration, formatMegabytes } from "@/lib/format";
import type { InstanceView } from "@/types/api";
import { Button, ErrorNotice, Icon, StatusChip, statusHint, statusStyle } from "@/ui";

export function ServerCard({
  instance,
  onOpen,
}: {
  instance: InstanceView;
  onOpen: () => void;
}) {
  const { t } = useI18n();
  const actions = useInstanceActions();
  const [copied, setCopied] = useState(false);

  const hint = statusHint(instance.status);
  const busy =
    actions.start.isPending || actions.stop.isPending || instance.status === "starting" ||
    instance.status === "stopping";
  const failure = actions.start.error ?? actions.stop.error;

  const copyAddress = async () => {
    if (!instance.address) return;
    await navigator.clipboard.writeText(instance.address);
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  return (
    <article
      className="panel relative flex flex-col gap-4 overflow-hidden p-4 transition-colors hover:border-[color:var(--line-strong)]"
      style={statusStyle(instance.status)}
    >
      {/* State earns colour in exactly one place on the surface: a hairline
          down the left edge, readable at a glance across ten cards. */}
      <span
        aria-hidden="true"
        className="absolute inset-y-0 left-0 w-[2px]"
        style={{ background: "var(--chip-fg)", opacity: 0.7 }}
      />
      <header className="flex items-start justify-between gap-3">
        <button
          type="button"
          onClick={onOpen}
          className="min-w-0 text-left"
        >
          <h3 className="truncate text-[length:var(--text-lg)] font-semibold">{instance.name}</h3>
          <p className="mono truncate text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
            {instance.build.distribution} {instance.build.version}
          </p>
        </button>
        <StatusChip status={instance.status} />
      </header>

      {hint ? (
        <p className="text-[length:var(--text-xs)] text-[color:var(--text-muted)]">{t(hint)}</p>
      ) : null}

      <dl className="grid grid-cols-3 gap-3">
        <Stat
          label={t("servers.players")}
          value={
            instance.status === "online"
              ? `${instance.players_online}${instance.players_max ? ` / ${instance.players_max}` : ""}`
              : "—"
          }
        />
        <Stat
          label={t("servers.memory")}
          value={instance.memory_used_mb ? formatMegabytes(instance.memory_used_mb) : "—"}
        />
        <Stat label={t("servers.uptime")} value={formatDuration(instance.uptime_seconds)} />
      </dl>

      {instance.address ? (
        <button
          type="button"
          onClick={() => void copyAddress()}
          className="flex items-center justify-between gap-2 rounded-[var(--radius)] border border-[color:var(--line)] px-3 py-2 text-left transition-colors hover:border-[color:var(--line-strong)]"
          style={{ background: "var(--surface-sunken)" }}
        >
          <span className="mono truncate text-[length:var(--text-sm)]">{instance.address}</span>
          <span className="flex flex-none items-center gap-1.5 text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
            <Icon name={copied ? "check" : "external"} size={13} />
            {copied ? t("actions.copied") : t("actions.copy")}
          </span>
        </button>
      ) : null}

      {failure ? <ErrorNotice error={failure} compact /> : null}

      <footer className="flex items-center gap-2">
        {instance.status === "online" || instance.status === "starting" ? (
          <Button
            icon="stop"
            disabled={busy}
            onClick={() => actions.stop.mutate(instance.id)}
            className="flex-1"
          >
            {t("actions.stop")}
          </Button>
        ) : (
          <Button
            variant="primary"
            icon="play"
            disabled={busy}
            onClick={() => actions.start.mutate(instance.id)}
            className="flex-1"
          >
            {instance.status === "sleeping" ? t("actions.wake") : t("actions.start")}
          </Button>
        )}
        <Button variant="quiet" icon="chevron-right" iconOnly onClick={onOpen}>
          {instance.name}
        </Button>
      </footer>
    </article>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col gap-0.5">
      <dt className="label">{label}</dt>
      <dd className="mono text-[length:var(--text-sm)]">{value}</dd>
    </div>
  );
}
