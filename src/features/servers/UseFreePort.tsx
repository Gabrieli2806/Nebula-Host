/**
 * The fix offered with "port is taken": move the server to the free port the
 * backend found, and start it there.
 */

import { useMutation, useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { Button } from "@/ui";

export function UseFreePortButton({ instanceId, port }: { instanceId: string; port: number }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const move = useMutation({
    mutationFn: async () => {
      await api.instances.update(instanceId, { port });
      await api.instances.start(instanceId);
    },
    onSettled: () => client.invalidateQueries({ queryKey: keys.instances }),
  });
  return (
    <Button size="sm" variant="primary" icon="play" disabled={move.isPending || move.isSuccess} onClick={() => move.mutate()}>
      {t("errors.usePort", { port })}
    </Button>
  );
}
