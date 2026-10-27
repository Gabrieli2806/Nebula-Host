/**
 * Drop .jar files from this computer onto Mods / Plugins to install them in
 * the server's addon folder (mods/ or plugins/). Works for another machine
 * too: the import sends the files there. A jar of the same name is replaced;
 * anything that is not a .jar is left out and said so. A running server
 * loads them on its next start.
 */

import { useRef, useState } from "react";

import { useImportFiles, useWindowDragDrop } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import type { ImportSummary, InstanceView } from "@/types/api";
import { ErrorNotice, Icon } from "@/ui";

import "../FilesPanel.css";

const isJar = (path: string) => path.toLowerCase().endsWith(".jar");

export function useAddonDrop(instance: InstanceView, folder: string | null | undefined, disabled: boolean) {
  const { t } = useI18n();
  const importer = useImportFiles(instance.id);
  const [over, setOver] = useState<{ jars: number; others: number } | null>(null);
  const [result, setResult] = useState<{ summary: ImportSummary; skipped: number } | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [noJars, setNoJars] = useState(false);
  const [busy, setBusy] = useState(false);
  const counts = useRef({ jars: 0, others: 0 });

  const inside = (x: number, y: number) => !!document.elementFromPoint(x, y)?.closest(".addons");

  useWindowDragDrop(
    (event) => {
      if (event.type === "leave") return setOver(null);
      if (event.type === "enter") {
        const jars = event.paths.filter(isJar).length;
        counts.current = { jars, others: event.paths.length - jars };
      }
      if (!folder || busy) return setOver(null);
      const hit = inside(event.position.x, event.position.y);
      if (event.type !== "drop") return setOver(hit ? { ...counts.current } : null);
      setOver(null);
      if (!hit) return;
      const jars = event.paths.filter(isJar);
      const skipped = event.paths.length - jars.length;
      setResult(null);
      setError(null);
      setNoJars(jars.length === 0);
      if (jars.length === 0) return;
      setBusy(true);
      importer
        .run(folder, jars, "overwrite")
        .then((summary) => setResult({ summary, skipped }))
        .catch(setError)
        .finally(() => setBusy(false));
    },
    !disabled && !!folder,
  );

  const running = instance.status === "online" || instance.status === "starting";

  const overlay = over ? (
    <div className="files-drop addons-drop" aria-hidden="true">
      <div className="files-drop-banner">
        <Icon name="arrow-down" size={14} />
        <span className="min-w-0 truncate">{t("addons.dropInto", { folder: `${folder}/` })}</span>
        <span className="files-drop-count mono">
          {over.jars === 1 ? t("addons.dropOne") : t("addons.dropMany", { count: over.jars })}
          {over.others ? ` · ${t("addons.dropIgnored", { count: over.others })}` : ""}
        </span>
      </div>
    </div>
  ) : null;

  const notice = busy ? (
    <p className="addons-drop-note" role="status">
      <Icon name="upload" size={13} /> {t("addons.dropInstalling")}
    </p>
  ) : noJars ? (
    <p className="addons-drop-note" data-tone="warn" role="status">
      <Icon name="warning" size={13} /> {t("addons.dropNoJars")}
    </p>
  ) : error ? (
    <ErrorNotice error={error} compact />
  ) : result ? (
    <p className="addons-drop-note" role="status">
      <Icon name="check" size={13} />
      {t("addons.dropDone", { count: result.summary.imported.length })}
      {result.skipped ? ` · ${t("addons.dropIgnored", { count: result.skipped })}` : ""}
      {running ? ` · ${t("addons.dropRestart")}` : ""}
      <button type="button" className="addons-drop-dismiss" aria-label={t("dialog.close")} onClick={() => setResult(null)}>
        <Icon name="close" size={12} />
      </button>
    </p>
  ) : null;

  return { overlay, notice };
}
