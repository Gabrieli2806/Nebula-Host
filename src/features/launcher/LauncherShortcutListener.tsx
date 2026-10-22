import { useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { launcherApi, toAppError } from "@/lib/ipc";
import { IS_WEB } from "@/lib/web";
import { usePrimaryAccount } from "./primaryAccount";

/** Consume native shortcut requests even when the launcher page isn't open. */
export function LauncherShortcutListener() {
  const navigate = useNavigate();
  const client = useQueryClient();
  const handled = useRef<string | null>(null);
  const { primary, loading } = usePrimaryAccount();
  const request = useQuery({
    queryKey: ["launcher", "shortcut"], queryFn: launcherApi.takeShortcut,
    enabled: !IS_WEB,
    refetchInterval: 1000, retry: false,
  });
  useEffect(() => {
    const item = request.data;
    if (!item || handled.current === item.requestId || loading) return;
    handled.current = item.requestId;
    const url = `/launcher?instance=${encodeURIComponent(item.id)}&request=${item.requestId}`;
    navigate(url);
    // Android shortcuts carry their account; desktop ones use the primary.
    const mode = item.mode ?? (primary ? "account" : "demo");
    const account = item.mode ? item.accountId : primary?.id;
    void launcherApi.start(item.id, mode, account).then(() => {
      void client.invalidateQueries({ queryKey: ["launcher", "status"] });
    }).catch(error => navigate(url, { replace: true, state: { shortcutError: toAppError(error) } }));
  }, [request.data, navigate, client, primary, loading]);
  return null;
}
