/**
 * The server list.
 *
 * Two densities, because "an old PC as a server" means ten to twenty servers
 * as often as it means one. Cards for a handful, a table when there are more
 * than fit on a screen.
 */

import { useState } from "react";
import { useNavigate } from "react-router-dom";

import { CreateServerDialog } from "./CreateServerDialog";
import { ServerCard } from "./ServerCard";
import { ServerRow } from "./ServerRow";
import { useAppSettings, useInstances } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { Button, EmptyState, ErrorNotice, Skeleton } from "@/ui";

type Density = "cards" | "list";

export function ServersScreen() {
  const { t } = useI18n();
  const navigate = useNavigate();
  const { data: instances, isLoading, error, refetch } = useInstances();
  const { data: settings } = useAppSettings();
  const [creating, setCreating] = useState(false);
  // The saved preference is the default; a click here overrides it for this
  // visit without writing back, so trying the other density is free.
  const [override, setOverride] = useState<Density | null>(null);
  const density: Density = override ?? (settings?.density === "list" ? "list" : "cards");
  const setDensity = setOverride;

  return (
    <div className="mx-auto flex max-w-[1160px] flex-col gap-5 p-6">
      <header className="flex items-end justify-between gap-4">
        <div>
          <h1 className="text-[length:var(--text-2xl)] font-semibold tracking-[-0.01em]">
            {t("servers.title")}
          </h1>
          {instances?.length ? (
            <p className="text-[color:var(--text-muted)]">
              {t("node.running", {
                count: instances.filter((instance) => instance.status === "online").length,
              })}
            </p>
          ) : null}
        </div>

        <div className="flex items-center gap-2">
          <div className="flex overflow-hidden rounded-[var(--radius)] border border-[color:var(--line-strong)]">
            <DensityButton
              active={density === "cards"}
              onClick={() => setDensity("cards")}
              label={t("servers.densityCards")}
            />
            <DensityButton
              active={density === "list"}
              onClick={() => setDensity("list")}
              label={t("servers.densityList")}
            />
          </div>
          <Button variant="primary" icon="plus" onClick={() => setCreating(true)}>
            {t("actions.create")}
          </Button>
        </div>
      </header>

      {error ? <ErrorNotice error={error} onRetry={() => void refetch()} /> : null}

      {isLoading ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(320px,1fr))] gap-4">
          {[0, 1, 2].map((index) => (
            <Skeleton key={index} className="h-[168px] rounded-[var(--radius-lg)]" />
          ))}
        </div>
      ) : null}

      {instances && instances.length === 0 ? (
        <div className="panel">
          <EmptyState
            icon="servers"
            title={t("servers.empty")}
            hint={t("servers.emptyHint")}
            action={
              <Button variant="primary" icon="plus" onClick={() => setCreating(true)}>
                {t("actions.create")}
              </Button>
            }
          />
        </div>
      ) : null}

      {instances && instances.length > 0 ? (
        density === "cards" ? (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(320px,1fr))] gap-4">
            {instances.map((instance) => (
              <ServerCard
                key={instance.id}
                instance={instance}
                onOpen={() => navigate(`/servers/${instance.id}`)}
              />
            ))}
          </div>
        ) : (
          <div className="panel-flush">
            <table className="w-full border-collapse text-left">
              <thead>
                <tr className="border-b border-[color:var(--line)]">
                  <Th>{t("servers.title")}</Th>
                  <Th>{t("servers.version")}</Th>
                  <Th>{t("servers.players")}</Th>
                  <Th>{t("servers.memory")}</Th>
                  <Th>{t("servers.uptime")}</Th>
                  <Th />
                </tr>
              </thead>
              <tbody>
                {instances.map((instance) => (
                  <ServerRow
                    key={instance.id}
                    instance={instance}
                    onOpen={() => navigate(`/servers/${instance.id}`)}
                  />
                ))}
              </tbody>
            </table>
          </div>
        )
      ) : null}

      <CreateServerDialog
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={(instanceId) => {
          setCreating(false);
          navigate(`/servers/${instanceId}`);
        }}
      />
    </div>
  );
}

function DensityButton({
  active,
  onClick,
  label,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className="h-[34px] px-3 text-[length:var(--text-sm)] transition-colors"
      style={{
        background: active ? "var(--surface-raised)" : "transparent",
        color: active ? "var(--text)" : "var(--text-muted)",
        fontWeight: active ? 600 : 400,
      }}
    >
      {label}
    </button>
  );
}

function Th({ children }: { children?: React.ReactNode }) {
  return <th className="label px-4 py-2.5 font-semibold">{children}</th>;
}
