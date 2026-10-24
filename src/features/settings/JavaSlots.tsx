/**
 * Settings › Java, modelled on Modrinth's: one slot per Java the games need
 * (25, 21, 17, 8). Each shows what servers needing it will run on — found
 * automatically or pinned by you — with Detect, Browse, Download and Test.
 * Servers pick their slot from their game version on their own; a server can
 * still pin a specific JVM in its own settings.
 *
 * Folded by default: most people never touch it, so it shows one line saying
 * whether every slot has a Java, and opens on demand.
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
import { IS_MOBILE } from "@/lib/web";

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
  const [open, setOpen] = useState(false);
  const overview = useQuery({
    queryKey: OVERVIEW_KEY,
    queryFn: () => api.java.overview(false),
  });
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
    window.setTimeout(
      () => void client.invalidateQueries({ queryKey: OVERVIEW_KEY }),
      400,
    );
  };

  const slots = overview.data?.slots ?? [];
  const ready = slots.filter((slot) => slot.effective).length;
  const pinned = slots.filter((slot) => !slot.automatic).length;

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
          {t("java.title")}
        </button>
      }
      actions={
        open ? (
          <Button
            size="sm"
            variant="quiet"
            icon="search"
            disabled={rescan.isPending}
            onClick={() => rescan.mutate()}
          >
            {rescan.isPending ? t("java.detecting") : t("java.detect")}
          </Button>
        ) : overview.data ? (
          <span
            className="java-summary"
            data-missing={ready < slots.length ? "" : undefined}
          >
            <Icon name={ready < slots.length ? "warning" : "check"} size={13} />
            {t("java.summary", { ready, total: slots.length })}
            {pinned ? ` · ${t("java.summaryPinned", { count: pinned })}` : ""}
          </span>
        ) : null
      }
    >
      {overview.error ? <ErrorNotice error={overview.error} compact /> : null}
      {!open ? null : !overview.data ? (
        <div className="java-slots">
          {[0, 1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-[72px]" />
          ))}
        </div>
      ) : (
        <>
          <p className="java-intro">{t("java.intro")}</p>
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
        </>
      )}
    </Panel>
  );
}

const BROWSE = "__browse";
const DOWNLOAD = "__download";

/** One line per slot: name, what it uses, and a single dropdown for the rest. */
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
  const [browseError, setBrowseError] = useState<unknown>(null);
  const remote = isRemote();

  // Same major first, then newer ones (a newer JVM runs older servers too).
  const choices = installations
    .filter((i) => i.major >= slot.major)
    .sort(
      (a, b) =>
        Number(a.major !== slot.major) - Number(b.major !== slot.major) ||
        a.major - b.major,
    );
  const effective = slot.effective;
  const exact = effective?.major === slot.major;
  const downloading = download.isPending && download.variables === slot.major;
  const pct =
    downloading &&
    progress?.major === slot.major &&
    "received" in progress.progress &&
    progress.progress.total
      ? Math.round((progress.progress.received / progress.progress.total) * 100)
      : null;

  const describe = (java: JavaInstallation) =>
    `${java.vendor ?? "Java"} ${java.version}`;

  const onSelect = async (value: string) => {
    setBrowseError(null);
    if (value === DOWNLOAD) {
      download.mutate(slot.major);
      return;
    }
    if (value === BROWSE) {
      try {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const picked = await open({
          multiple: false,
          filters: [{ name: "java", extensions: ["exe", ""] }],
        });
        if (typeof picked === "string")
          onPin((await api.settings.probeJava(picked)).path);
      } catch (error) {
        setBrowseError(error);
      }
      return;
    }
    onPin(value || null);
  };

  return (
    <div className="java-slot" data-missing={effective ? undefined : ""}>
      <div className="java-slot-head">
        <span className="java-slot-major">Java {slot.major}</span>
        <span className="java-slot-for">{t(`java.for${slot.major}`)}</span>
      </div>

      <span className="java-slot-state" title={effective?.path}>
        {downloading ? (
          <>
            {pct !== null
              ? `${t("java.downloading")} ${pct}%`
              : t("java.downloading")}
          </>
        ) : effective ? (
          <>
            <Icon name={exact ? "check" : "info"} size={13} />
            {describe(effective)}
            {exact ? "" : ` · ${t("java.newer", { major: effective.major })}`}
          </>
        ) : (
          <>
            <Icon name="warning" size={13} /> {t("java.notFoundShort")}
          </>
        )}
      </span>

      <Select
        className="java-slot-select"
        aria-label={t("java.choose", { major: slot.major })}
        value={slot.automatic ? "" : (slot.configured ?? "")}
        disabled={downloading}
        onChange={(event) => void onSelect(event.target.value)}
      >
        <option value="">{t("java.automatic")}</option>
        {choices.length ? (
          <optgroup label={t("java.detected")}>
            {choices.map((install) => (
              <option key={install.path} value={install.path}>
                Java {install.major} · {describe(install)}
              </option>
            ))}
          </optgroup>
        ) : null}
        <optgroup label={t("java.more")}>
          {remote || IS_MOBILE ? null : (
            <option value={BROWSE}>{t("java.browse")}…</option>
          )}
          {canDownload && !exact ? (
            <option value={DOWNLOAD}>
              {t("java.download", { major: slot.major })}
            </option>
          ) : null}
        </optgroup>
      </Select>

      {browseError || download.error ? (
        <div className="java-slot-error">
          <ErrorNotice error={browseError ?? download.error} compact />
        </div>
      ) : null}
    </div>
  );
}
