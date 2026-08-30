/** Mods and plugins: what is installed, and what could be. */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { keys, useGames } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { formatBytes, formatCount } from "@/lib/format";
import type { InstanceView } from "@/types/api";
import { Button, EmptyState, ErrorNotice, Panel, Tabs, TextInput } from "@/ui";

type Mode = "installed" | "browse";

export function AddonsPanel({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const { data: games } = useGames();
  const [mode, setMode] = useState<Mode>("installed");

  const distribution = games
    ?.find((game) => game.id === instance.game)
    ?.distributions.find((candidate) => candidate.id === instance.build.distribution);

  if (distribution && !distribution.addon_dir) {
    return (
      <Panel title={t("addons.title")}>
        <EmptyState icon="puzzle" title={t("addons.unsupported")} />
      </Panel>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <Tabs<Mode>
        active={mode}
        onChange={setMode}
        tabs={[
          { id: "installed", label: t("addons.installed") },
          { id: "browse", label: t("addons.browse") },
        ]}
      />
      {mode === "installed" ? <Installed instance={instance} /> : <Browse instance={instance} />}
    </div>
  );
}

function Installed({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const client = useQueryClient();

  const installed = useQuery({
    queryKey: keys.addons(instance.id),
    queryFn: () => api.addons.installed(instance.id),
  });

  const invalidate = () => client.invalidateQueries({ queryKey: keys.addons(instance.id) });

  const toggle = useMutation({
    mutationFn: ({ fileName, enabled }: { fileName: string; enabled: boolean }) =>
      api.addons.setEnabled(instance.id, fileName, enabled),
    onSuccess: invalidate,
  });
  const remove = useMutation({
    mutationFn: (fileName: string) => api.addons.remove(instance.id, fileName),
    onSuccess: invalidate,
  });
  const update = useMutation({
    mutationFn: ({ url, fileName, old }: { url: string; fileName: string; old: string }) =>
      api.addons.remove(instance.id, old).then(() => api.addons.install(instance.id, url, fileName)),
    onSuccess: invalidate,
  });

  const [entries = [], meta = []] = installed.data ?? [];

  if (installed.error) {
    return <ErrorNotice error={installed.error} onRetry={() => void installed.refetch()} />;
  }

  if (entries.length === 0) {
    return (
      <Panel title={t("addons.installed")}>
        <EmptyState icon="puzzle" title={t("addons.none")} hint={t("addons.noneHint")} />
      </Panel>
    );
  }

  return (
    <Panel title={t("addons.installed")}>
      <ul>
        {entries.map((entry) => {
          const known = meta.find((candidate) => candidate.file_name === entry.file_name);
          return (
            <li
              key={entry.id}
              className="flex items-center gap-3 border-b border-[color:var(--line)] px-4 py-3 last:border-0"
            >
              {known?.icon_url ? (
                <img src={known.icon_url} alt="" width={32} height={32} className="flex-none rounded-[var(--radius-sm)]" />
              ) : (
                <span className="h-8 w-8 flex-none rounded-[var(--radius-sm)]" style={{ background: "var(--surface-raised)" }} />
              )}

              <div className="min-w-0 flex-1">
                <p className="truncate font-medium" style={{ opacity: entry.enabled ? 1 : 0.55 }}>
                  {known?.title ?? entry.name}
                </p>
                <p className="mono truncate text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
                  {known?.installed_version_number ?? entry.file_name}
                </p>
              </div>

              {known?.update_available && known.latest_download_url && known.latest_file_name ? (
                <Button
                  size="sm"
                  disabled={update.isPending}
                  onClick={() =>
                    update.mutate({
                      url: known.latest_download_url!,
                      fileName: known.latest_file_name!,
                      old: entry.file_name,
                    })
                  }
                >
                  {t("addons.update", { version: known.latest_version_number ?? "" })}
                </Button>
              ) : null}

              <Button
                size="sm"
                variant="quiet"
                onClick={() => toggle.mutate({ fileName: entry.file_name, enabled: !entry.enabled })}
              >
                {entry.enabled ? t("addons.disable") : t("addons.enable")}
              </Button>
              <Button
                size="sm"
                variant="quiet"
                icon="trash"
                iconOnly
                onClick={() => remove.mutate(entry.file_name)}
              >
                {t("actions.delete")}
              </Button>
            </li>
          );
        })}
      </ul>
    </Panel>
  );
}

function Browse({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const [query, setQuery] = useState("");
  const [submitted, setSubmitted] = useState("");

  const results = useQuery({
    queryKey: ["addons", instance.id, "search", submitted],
    queryFn: () => api.addons.search(instance.id, submitted),
  });

  const install = useMutation({
    mutationFn: ({ url, fileName }: { url: string; fileName: string }) =>
      api.addons.install(instance.id, url, fileName),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.addons(instance.id) }),
  });

  return (
    <Panel
      title={t("addons.browse")}
      actions={
        <div className="flex gap-2">
          <TextInput
            value={query}
            placeholder={t("addons.searchPlaceholder")}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => event.key === "Enter" && setSubmitted(query)}
          />
          <Button size="sm" icon="search" onClick={() => setSubmitted(query)}>
            {t("actions.search")}
          </Button>
        </div>
      }
    >
      {results.error ? (
        <div className="p-4">
          <ErrorNotice error={results.error} onRetry={() => void results.refetch()} />
        </div>
      ) : null}

      {install.error ? (
        <div className="p-4">
          <ErrorNotice error={install.error} compact />
        </div>
      ) : null}

      <ul>
        {results.data?.entries.map((entry) => (
          <li
            key={entry.project_id}
            className="flex items-start gap-3 border-b border-[color:var(--line)] px-4 py-3 last:border-0"
          >
            {entry.icon_url ? (
              <img src={entry.icon_url} alt="" width={40} height={40} className="flex-none rounded-[var(--radius-sm)]" />
            ) : (
              <span className="h-10 w-10 flex-none rounded-[var(--radius-sm)]" style={{ background: "var(--surface-raised)" }} />
            )}

            <div className="min-w-0 flex-1">
              <p className="font-medium">{entry.title}</p>
              <p className="line-clamp-2 text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
                {entry.description}
              </p>
              <p className="mono text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
                {t("addons.downloads", { count: formatCount(entry.downloads, language) })} ·{" "}
                {formatBytes(entry.featured_version.file_size)}
              </p>
            </div>

            <Button
              size="sm"
              disabled={install.isPending}
              onClick={() =>
                install.mutate({
                  url: entry.featured_version.download_url,
                  fileName: entry.featured_version.file_name,
                })
              }
            >
              {install.isPending ? t("addons.installing") : t("addons.install")}
            </Button>
          </li>
        ))}
      </ul>
    </Panel>
  );
}
