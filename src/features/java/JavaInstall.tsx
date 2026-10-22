/**
 * "This needs Java N": the one-click install the app offers wherever a
 * missing Java stops something (starting a server, the first-run check).
 *
 * Nothing installs on its own. On the desktop it downloads Eclipse Temurin;
 * on Android it unpacks the runtime that ships in the app.
 */

import { useQueryClient } from "@tanstack/react-query";

import { keys, useJavaDownload } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatBytes } from "@/lib/format";
import { Button, ErrorNotice } from "@/ui";

export function JavaInstallButton({ major, size = "sm" }: { major: number; size?: "sm" | "default" }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const { download, progress } = useJavaDownload();
  const mine = progress?.major === major ? progress.progress : null;
  const busy = download.isPending;

  let label = t("javaInstall.install", { major });
  if (busy && mine?.phase === "downloading") {
    label = mine.total
      ? t("javaInstall.progress", { done: formatBytes(mine.received), total: formatBytes(mine.total) })
      : t("javaInstall.installing");
  } else if (busy) {
    label = t("javaInstall.installing");
  }

  return (
    <div className="flex flex-col items-start gap-2">
      <Button
        size={size}
        variant="primary"
        icon="download"
        disabled={busy || download.isSuccess}
        onClick={() =>
          download.mutate(major, {
            onSuccess: () => {
              void client.invalidateQueries({ queryKey: keys.requirements });
              void client.invalidateQueries({ queryKey: keys.instances });
            },
          })
        }
      >
        {download.isSuccess ? t("javaInstall.done", { major }) : label}
      </Button>
      {download.error ? <ErrorNotice error={download.error} compact /> : null}
    </div>
  );
}
