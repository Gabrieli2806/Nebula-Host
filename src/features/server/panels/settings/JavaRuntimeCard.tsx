/**
 * Which Java this server runs on.
 *
 * Every JVM the node can find (system installs, the registry, PATH, the
 * Minecraft/Modrinth/CurseForge/Prism launchers' bundled runtimes, and ones
 * Nebula downloaded) in one list, with the one this game version is made for
 * marked "Recommended". When that one is missing, it can be downloaded here.
 * "Automatic" is the default and usually right; picking a row pins it.
 *
 * The list is folded by default: the status line (and its download button)
 * stays visible, the full list opens on demand.
 */

import { useState, type ReactNode } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { keys, useJavaDownload, useJavaRuntimes } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { isRemote } from "@/lib/connection";
import { formatBytes } from "@/lib/format";
import { api } from "@/lib/ipc";
import type { InstanceView, JavaInstallation, JavaRuntimes } from "@/types/api";
import { Button, ErrorNotice, Icon, Meter, Panel, Skeleton } from "@/ui";

import "@/features/settings/JavaSlots.css";

export function JavaRuntimeCard({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const runtimes = useJavaRuntimes(instance.id);
  const { download, progress } = useJavaDownload();
  const [open, setOpen] = useState(false);

  const rescan = useMutation({
    mutationFn: () => api.java.runtimes(instance.id, true),
    onSuccess: (data) =>
      client.setQueryData(keys.javaRuntimes(instance.id), data),
  });

  const select = useMutation({
    // An empty path means automatic; the backend clears the pin.
    mutationFn: (path: string) =>
      api.instances.update(instance.id, { runtimePath: path }),
    onSuccess: () =>
      Promise.all([
        client.invalidateQueries({ queryKey: keys.instances }),
        client.invalidateQueries({ queryKey: keys.javaRuntimes(instance.id) }),
      ]),
  });

  const browse = useMutation({
    mutationFn: async () => {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const picked = await open({
        multiple: false,
        filters: [{ name: "java", extensions: ["exe", ""] }],
      });
      if (typeof picked !== "string") return null;
      // Check it is a JVM before pinning it.
      const found = await api.settings.probeJava(picked);
      await api.instances.update(instance.id, { runtimePath: found.path });
      return found;
    },
    onSuccess: () =>
      Promise.all([
        client.invalidateQueries({ queryKey: keys.instances }),
        client.invalidateQueries({ queryKey: keys.javaRuntimes(instance.id) }),
      ]),
  });

  if (runtimes.error) {
    return (
      <Panel title={t("javaRuntime.title")}>
        <div className="p-4">
          <ErrorNotice
            error={runtimes.error}
            onRetry={() => void runtimes.refetch()}
          />
        </div>
      </Panel>
    );
  }

  if (!runtimes.data) {
    return (
      <Panel title={t("javaRuntime.title")}>
        <div className="flex flex-col gap-2 p-4" aria-hidden="true">
          <Skeleton className="h-6 w-2/3" />
          <Skeleton className="h-12" />
          <Skeleton className="h-12" />
        </div>
      </Panel>
    );
  }

  const data = runtimes.data;
  if (!data.applies) return null;

  const major = data.recommended_major ?? null;
  const hasRecommended =
    major !== null &&
    data.installations.some((install) => install.major === major);
  const selected = data.selected ?? null;
  const pinnedUnknown =
    selected !== null &&
    !data.installations.some((install) => samePath(install.path, selected));
  const live =
    progress && major !== null && progress.major === major
      ? progress.progress
      : null;
  const downloading = download.isPending && download.variables === major;
  const failure =
    select.error ?? browse.error ?? download.error ?? rescan.error;
  const pinned =
    selected === null
      ? null
      : data.installations.find((install) => samePath(install.path, selected));

  return (
    <Panel
      title={
        <button
          type="button"
          className="java-fold"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
        >
          <Icon name="chevron-down" size={14} className="java-fold-chevron" />
          {t("javaRuntime.title")}
        </button>
      }
      actions={
        !open ? (
          <span className="java-summary">
            {selected === null
              ? t("javaRuntime.automatic")
              : pinned
                ? `Java ${pinned.major} · ${pinned.version}`
                : t("javaRuntime.custom")}
          </span>
        ) : (
          <Button
            size="sm"
            variant="quiet"
            icon="refresh"
            disabled={rescan.isPending || runtimes.isFetching}
            onClick={() => rescan.mutate()}
          >
            {t("javaRuntime.rescan")}
          </Button>
        )
      }
    >
      <div className="java-card">
        {open ? (
          <p className="pref-help">
            {major !== null
              ? t("javaRuntime.help", {
                  version: instance.build.version,
                  major,
                })
              : t("javaRuntime.helpUnknown")}
          </p>
        ) : null}

        {major !== null ? (
          <div
            className="java-status"
            data-tone={hasRecommended ? "ok" : "warn"}
            role="status"
          >
            <Icon name={hasRecommended ? "check" : "warning"} size={14} />
            <div className="java-status-text">
              <span className="java-status-title">
                {hasRecommended
                  ? t("javaRuntime.statusInstalled", { major })
                  : t("javaRuntime.statusMissing", { major })}
              </span>
              {!hasRecommended && data.can_download ? (
                <span className="java-status-hint">
                  {t("javaRuntime.statusMissingHint")}
                </span>
              ) : null}
            </div>
            {data.can_download && (!hasRecommended || downloading) ? (
              <Button
                size="sm"
                variant={hasRecommended ? "default" : "primary"}
                icon="arrow-down"
                disabled={download.isPending}
                onClick={() => download.mutate(major)}
              >
                {downloading
                  ? t("javaRuntime.downloading", { major })
                  : t("javaRuntime.download", { major })}
              </Button>
            ) : null}
          </div>
        ) : null}

        {downloading && live ? (
          <DownloadProgress major={major!} progress={live} />
        ) : null}
        {download.isSuccess && download.variables === major && !downloading ? (
          <p className="pref-hint">
            <Icon name="check" size={12} />
            {t("javaRuntime.downloaded", { major: major! })}
          </p>
        ) : null}

        {open ? (
          <>
            <div
              className="java-list"
              role="radiogroup"
              aria-label={t("javaRuntime.choiceLabel")}
            >
              <JavaChoice
                checked={selected === null}
                disabled={select.isPending}
                onSelect={() => select.mutate("")}
                title={t("javaRuntime.automatic")}
                detail={
                  data.automatic
                    ? t("javaRuntime.automaticUses", { path: data.automatic })
                    : t("javaRuntime.automaticNone")
                }
                tags={
                  major !== null && hasRecommended ? (
                    <span className="tag" data-tone="accent">
                      {t("javaRuntime.recommended")}
                    </span>
                  ) : null
                }
              />
              {data.installations.map((install) => (
                <InstallationRow
                  key={install.path}
                  install={install}
                  data={data}
                  checked={
                    selected !== null && samePath(install.path, selected)
                  }
                  disabled={select.isPending}
                  onSelect={() => select.mutate(install.path)}
                />
              ))}
              {pinnedUnknown ? (
                <JavaChoice
                  checked
                  disabled={select.isPending}
                  onSelect={() => undefined}
                  title={t("javaRuntime.custom")}
                  detail={selected}
                  mono
                />
              ) : null}
            </div>

            {data.installations.length === 0 ? (
              <p className="pref-hint">{t("javaRuntime.none")}</p>
            ) : null}

            <div className="java-foot">
              {!isRemote() ? (
                <Button
                  size="sm"
                  variant="quiet"
                  icon="folder"
                  disabled={browse.isPending}
                  onClick={() => browse.mutate()}
                >
                  {t("javaRuntime.browse")}
                </Button>
              ) : null}
              <span className="pref-hint">
                <Icon name="restart" size={12} />
                {t("javaRuntime.restartHint")}
              </span>
            </div>
          </>
        ) : null}

        {failure ? <ErrorNotice error={failure} /> : null}
      </div>
    </Panel>
  );
}

function InstallationRow({
  install,
  data,
  checked,
  disabled,
  onSelect,
}: {
  install: JavaInstallation;
  data: JavaRuntimes;
  checked: boolean;
  disabled: boolean;
  onSelect: () => void;
}) {
  const { t } = useI18n();
  const major = data.recommended_major ?? null;
  const fit =
    major === null
      ? null
      : install.major === major
        ? "recommended"
        : install.major < major
          ? "old"
          : "newer";
  const bits32 = install.arch === "x86" || install.arch === "arm";
  const source = install.source
    ? t(`javaRuntime.source_${install.source}`)
    : null;
  const who = [install.vendor, source].filter(Boolean).join(" · ");

  return (
    <JavaChoice
      checked={checked}
      disabled={disabled}
      onSelect={onSelect}
      title={
        <>
          <span className="tag mono tabular-nums">Java {install.major}</span>
          <span className="java-choice-who">
            {who || t("javaRuntime.installed")}
          </span>
          <span className="java-choice-version mono">{install.version}</span>
        </>
      }
      detail={install.path}
      mono
      tags={
        <>
          {fit === "recommended" ? (
            <span className="tag" data-tone="accent">
              {t("javaRuntime.recommended")}
            </span>
          ) : null}
          {fit === "old" ? (
            <span
              className="tag"
              data-tone="warn"
              title={t("javaRuntime.tooOldHint", { major: major ?? "" })}
            >
              {t("javaRuntime.tooOld")}
            </span>
          ) : null}
          {fit === "newer" ? (
            <span className="tag" title={t("javaRuntime.newerHint")}>
              {t("javaRuntime.newer")}
            </span>
          ) : null}
          {bits32 ? (
            <span
              className="tag"
              data-tone="warn"
              title={t("javaRuntime.bits32Hint")}
            >
              {t("javaRuntime.bits32")}
            </span>
          ) : null}
        </>
      }
    />
  );
}

function JavaChoice({
  checked,
  disabled,
  onSelect,
  title,
  detail,
  tags,
  mono = false,
}: {
  checked: boolean;
  disabled: boolean;
  onSelect: () => void;
  title: ReactNode;
  detail?: string | null;
  tags?: ReactNode;
  mono?: boolean;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      disabled={disabled}
      className="java-choice"
      onClick={onSelect}
    >
      <span className="java-choice-radio" aria-hidden="true" />
      <span className="java-choice-body">
        <span className="java-choice-title">
          {title}
          {tags}
        </span>
        {detail ? (
          <span
            className={`java-choice-detail ${mono ? "mono" : ""}`}
            title={detail}
          >
            {detail}
          </span>
        ) : null}
      </span>
    </button>
  );
}

function DownloadProgress({
  major,
  progress,
}: {
  major: number;
  progress: NonNullable<
    ReturnType<typeof useJavaDownload>["progress"]
  >["progress"];
}) {
  const { t } = useI18n();
  switch (progress.phase) {
    case "resolving":
      return (
        <p className="pref-hint">{t("javaRuntime.resolving", { major })}</p>
      );
    case "downloading": {
      const total = progress.total ?? 0;
      return (
        <Meter
          value={progress.received}
          max={total || 1}
          label={
            total
              ? t("javaRuntime.downloadingProgress", {
                  major,
                  received: formatBytes(progress.received),
                  total: formatBytes(total),
                })
              : t("javaRuntime.downloading", { major })
          }
        />
      );
    }
    case "extracting":
      return (
        <p className="pref-hint">{t("javaRuntime.extracting", { major })}</p>
      );
    default:
      return null;
  }
}

/** Windows paths compare without regard to case or slash direction. */
function samePath(a: string, b: string): boolean {
  const norm = (path: string) => path.replace(/\\/g, "/").toLowerCase();
  return norm(a) === norm(b);
}
