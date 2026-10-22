import { useEffect, useRef } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";
import { launcherApi, toAppError } from "@/lib/ipc";
import { IS_MOBILE } from "@/lib/web";

/** Consume native shortcut requests even when the launcher page isn't open. */
export function LauncherShortcutListener() {
  const navigate = useNavigate();
  const client = useQueryClient();
  const handled = useRef<string | null>(null);
  const request = useQuery({
    queryKey: ["launcher", "shortcut"], queryFn: launcherApi.takeShortcut,
    enabled: IS_MOBILE && /Android/i.test(navigator.userAgent),
    refetchInterval: 1000, retry: false,
  });
  useEffect(() => {
    const item = request.data;
    if (!item || handled.current === item.requestId) return;
    handled.current = item.requestId;
    const url = `/launcher?instance=${encodeURIComponent(item.id)}&request=${item.requestId}`;
    navigate(url);
    void launcherApi.start(item.id, item.mode, item.accountId).then(() => {
      void client.invalidateQueries({ queryKey: ["launcher", "status"] });
    }).catch(error => navigate(url, { replace: true, state: { shortcutError: toAppError(error) } }));
  }, [request.data, navigate, client]);
  return null;
}
