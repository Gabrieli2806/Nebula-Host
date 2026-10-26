/**
 * Windows Firewall blocking the server's Java. Windows writes those block
 * rules when its "allow access?" prompt is dismissed, and they beat any open
 * port, so the server runs and nobody can join. Checked while the server is
 * up; the fix asks for an administrator on the machine that runs it.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { isRemote } from "@/lib/connection";
import { api } from "@/lib/ipc";
import type { InstanceView } from "@/types/api";
import { Button, ErrorNotice, Icon } from "@/ui";

export function FirewallNotice({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const live = instance.status === "online" || instance.status === "starting";
  const key = ["firewall", instance.id] as const;
  const status = useQuery({
    queryKey: key,
    queryFn: () => api.instances.firewall(instance.id),
    enabled: live,
    staleTime: 5 * 60_000,
  });
  const fix = useMutation({
    mutationFn: () => api.instances.firewallFix(instance.id),
    onSuccess: (next) => client.setQueryData(key, next),
  });

  if (!live || !status.data?.applies || !status.data.blocked) return null;
  return (
    <div className="notice" role="alert">
      <span className="notice-icon">
        <Icon name="warning" />
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="font-[650]">{t("firewall.title")}</p>
        <p className="text-[color:var(--text-muted)]">{t("firewall.cause")}</p>
        <p>{isRemote() ? t("firewall.actionRemote") : t("firewall.action")}</p>
        {fix.error ? <ErrorNotice error={fix.error} compact /> : null}
      </div>
      <Button size="sm" variant="primary" icon="check" disabled={fix.isPending} onClick={() => fix.mutate()}>
        {fix.isPending ? t("firewall.fixing") : t("firewall.fix")}
      </Button>
    </div>
  );
}
