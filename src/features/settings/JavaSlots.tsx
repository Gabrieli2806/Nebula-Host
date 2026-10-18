/**
 * Settings › Java, modelled on Modrinth's: one slot per Java the games need
 * (25, 21, 17, 8). Each shows what servers needing it will run on — found
 * automatically or pinned by you — with Detect, Browse, Download and Test.
 * Servers pick their slot from their game version on their own; a server can
 * still pin a specific JVM in its own settings.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useJavaDownload } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { isRemote } from "@/lib/connection";
import { api } from "@/lib/ipc";
import type { JavaInstallation, JavaOverview, JavaSlot } from "@/types/api";
import { Button, ErrorNotice, Icon, Panel, Select, Skeleton } from "@/ui";

import "./JavaSlots.css";

const OVERVIEW_KEY = ["java", "overview"] as const;

export function JavaSlots({
  paths,
  onChange,
}: {
  paths: Record<string, string>;
  onChange: (paths: Record<string, string>) => void;
}) {
  const { t } = useI18n();
  const client = useQueryClient();
  const overview = useQuery({ queryKey: OVERVIEW_KEY, queryFn: () => api.java.overview(false) });
  const rescan = useMutation({
    mutationFn: () => api.java.overview(true),
    onSuccess: (data: JavaOverview) => client.setQueryData(OVERVIEW_KEY, data),
  });

  const pin = (major: number, path: string | null) => {
    const next = { ...paths };
    if (path) next[String(major)] = path;
    else delete next[String(major)];
    onChange(next);
    // The slot's effective JVM changes with the pin.
    window.setTimeout(() => void client.invalidateQueries({ queryKey: OVERVIEW_KEY }), 400);
  };

  return (
    <Panel
      title={t("java.title")}
      actions={
        <Button size="sm" variant="quiet" icon="search" disabled={rescan.isPending} onClick={() => rescan.mutate()}>
          {rescan.isPending ? t("java.detecting") : t("java.detect")}
        </Button>
      }
    >
      <p className="java-intro">{t("java.intro")}</p>
      {overview.error ? <ErrorNotice error={overview.error} compact /> : null}
      {!overview.data ? (
        <div className="java-slots">
          {[0, 1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-[72px]" />
          ))}
        </div>
      ) : (
        <div className="java-slots">
          {overview.data.slots.map((slot) => (
            <SlotRow
              key={slot.major}
              slot={slot}
              installations={overview.data.installations}
              canDownload={overview.data.canDownload}
              onPin={(path) => pin(slot.major, path)}
            />
          ))}
        </div>
      )}
    </Panel>
  );
}

function SlotRow({
  slot,
  installations,
  canDownload,
  onPin,
}: {
  slot: JavaSlot;
  installations: JavaInstallation[];
  canDownload: boolean;
  onPin: (path: string | null) => void;
}) {
  const { t } = useI18n();
  const { download, progress } = useJavaDownload();
  const [tested, setTested] = useState<string | null>(null);
  const test = useMutation({
    mutationFn: (path: string) => api.settings.probeJava(path),
    onSuccess: (java) => setTested(t("java.testOk", { version: java.version, major: java.major })),
  });

  // Choices for this slot: same major first, then newer ones (a newer JVM
  // usually runs an older server, the way Modrinth allows it too).
  const choices = installations
    .filter((i) => i.major >= slot.major)
    .sort((a, b) => Number(a.major !== slot.major) - Number(b.major !== slot.major) || a.major - b.major);
  const effective = slot.effective;
  const downloading = download.isPending && download.variables === slot.major;
  const pct =
    downloading && progress?.major === slot.major && "received" in progress.progress && progress.progress.total
      ? Math.round((progress.progress.received / progress.progress.total) * 100)
      : null;

  const browse = async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const picked = await open({ multiple: false, filters: [{ name: "java", extensions: ["exe", ""] }] });
    if (typeof picked !== "string") return;
    try {
      const java = await api.settings.probeJava(picked);
      onPin(java.path);
    } catch (error) {
      test.reset();
      setTested(null);
      throw error;
    }
  };

  return (
    <div className="java-slot" data-missing={effective ? undefined : ""}>
      <div className="java-slot-head">
        <span className="java-slot-major">Java {slot.major}</span>
        <span className="java-slot-for">{t(`java.for${slot.major}`)}</span>
      </div>

      <div className="java-slot-body">
        <div className="java-slot-path">
          {effective ? (
            <>
              <code className="selectable" title={effective.path}>
                {effective.path}
              </code>
              <span className="java-slot-meta">
                <span className="java-tag" data-kind={slot.automatic ? "auto" : "pinned"}>
                  {slot.automatic ? t("java.auto") : t("java.pinned")}
                </span>
                {effective.vendor ?? "Java"} {effective.version}
                {effective.major !== slot.major ? ` · ${t("java.newer", { major: effective.major })}` : ""}
              </span>
            </>
          ) : (
            <span className="java-slot-none">
              <Icon name="warning" size={13} /> {t("java.notFound")}
            </span>
          )}
        </div>

        <div className="java-slot-actions">
          {choices.length > 0 ? (
            <Select
              aria-label={t("java.choose", { major: slot.major })}
              value={slot.automatic ? "" : (slot.configured ?? "")}
              onChange={(event) => onPin(event.target.value || null)}
            >
              <option value="">{t("java.automatic")}</option>
              {choices.map((install) => (
                <option key={install.path} value={install.path}>
                  Java {install.major} · {install.vendor ?? ""} {install.version}
                </option>
              ))}
            </Select>
          ) : null}
          <Button size="sm" icon="folder" disabled={isRemote()} title={isRemote() ? t("remote.localOnly") : undefined} onClick={() => void browse()}>
            {t("java.browse")}
          </Button>
          {!effective || effective.major !== slot.major ? (
            canDownload ? (
              <Button size="sm" icon="download" disabled={download.isPending} onClick={() => download.mutate(slot.major)}>
                {downloading ? (pct !== null ? `${pct}%` : t("java.downloading")) : t("java.download", { major: slot.major })}
              </Button>
            ) : null
          ) : (
            <Button size="sm" variant="quiet" disabled={test.isPending} onClick={() => effective && test.mutate(effective.path)}>
              {t("java.test")}
            </Button>
          )}
        </div>
      </div>

      {tested ? (
        <p className="java-slot-ok">
          <Icon name="check" size={13} /> {tested}
        </p>
      ) : null}
      {test.error || download.error ? <ErrorNotice error={test.error ?? download.error} compact /> : null}
    </div>
  );
}
