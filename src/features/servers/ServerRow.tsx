/** One server, as a table row — the density for ten or twenty of them. */

import { useInstanceActions } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatDuration, formatMegabytes } from "@/lib/format";
import type { InstanceView } from "@/types/api";
import { Button, StatusDot } from "@/ui";

export function ServerRow({ instance, onOpen }: { instance: InstanceView; onOpen: () => void }) {
  const { t } = useI18n();
  const actions = useInstanceActions();
  const running = instance.status === "online" || instance.status === "starting";

  return (
    <tr
      className="border-b border-[color:var(--line)] last:border-0 hover:bg-[color:var(--surface-raised)]"
      onDoubleClick={onOpen}
    >
      <td className="px-4 py-2.5">
        <button type="button" onClick={onOpen} className="flex items-center gap-2.5 text-left">
          <StatusDot status={instance.status} />
          <span className="font-medium">{instance.name}</span>
          <span className="text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
            {t(`status.${instance.status}`)}
          </span>
        </button>
      </td>
      <td className="mono px-4 py-2.5 text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
        {instance.build.distribution} {instance.build.version}
      </td>
      <td className="mono px-4 py-2.5 text-[length:var(--text-sm)]">
        {instance.status === "online" ? instance.players_online : "—"}
      </td>
      <td className="mono px-4 py-2.5 text-[length:var(--text-sm)]">
        {instance.memory_used_mb ? formatMegabytes(instance.memory_used_mb) : "—"}
      </td>
      <td className="mono px-4 py-2.5 text-[length:var(--text-sm)]">
        {formatDuration(instance.uptime_seconds)}
      </td>
      <td className="px-4 py-2.5 text-right">
        <Button
          size="sm"
          icon={running ? "stop" : "play"}
          iconOnly
          onClick={() =>
            running ? actions.stop.mutate(instance.id) : actions.start.mutate(instance.id)
          }
        >
          {running ? t("actions.stop") : t("actions.start")}
        </Button>
      </td>
    </tr>
  );
}
