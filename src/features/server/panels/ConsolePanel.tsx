/**
 * The console.
 *
 * Follows new output while you are at the bottom and stops the moment you
 * scroll up — reading back through a crash while the view yanks you away is
 * the fastest way to make a log useless.
 */

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";

import { useConsole } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { formatTime } from "@/lib/format";
import type { InstanceView } from "@/types/api";
import { Button, EmptyState, ErrorNotice, Panel, TextInput } from "@/ui";

export function ConsolePanel({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const { data: lines = [] } = useConsole(instance.id);
  const [command, setCommand] = useState("");
  const [following, setFollowing] = useState(true);
  const [history, setHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const scroller = useRef<HTMLDivElement>(null);

  const send = useMutation({
    mutationFn: (value: string) => api.instances.command(instance.id, value),
  });

  useLayoutEffect(() => {
    if (!following) return;
    const element = scroller.current;
    if (element) element.scrollTop = element.scrollHeight;
  }, [lines, following]);

  useEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const onScroll = () => {
      const distance = element.scrollHeight - element.scrollTop - element.clientHeight;
      setFollowing(distance < 24);
    };
    element.addEventListener("scroll", onScroll);
    return () => element.removeEventListener("scroll", onScroll);
  }, []);

  const submit = () => {
    const value = command.trim();
    if (!value) return;
    send.mutate(value);
    setHistory((previous) => [value, ...previous].slice(0, 50));
    setHistoryIndex(-1);
    setCommand("");
  };

  const running = instance.status === "online" || instance.status === "starting";

  return (
    <Panel
      title={t("console.title")}
      className="h-[calc(100vh-190px)] min-h-[340px]"
      bodyClassName="flex flex-col"
      actions={
        !following ? (
          <Button size="sm" icon="chevron-down" onClick={() => setFollowing(true)}>
            {t("console.follow")}
          </Button>
        ) : null
      }
    >
      <div ref={scroller} className="console selectable min-h-0 flex-1 overflow-auto py-2">
        {lines.length === 0 ? (
          <EmptyState icon="terminal" title={t("console.empty")} />
        ) : (
          lines.map((line, index) => (
            <div key={`${line.timestamp}-${index}`} className={`console-line console-${line.level}`}>
              <span className="flex-none text-[color:var(--text-faint)]">
                {formatTime(line.timestamp, language)}
              </span>
              <span>{line.message}</span>
            </div>
          ))
        )}
      </div>

      <div className="flex flex-none items-center gap-2 border-t border-[color:var(--line)] p-3">
        <TextInput
          className="mono"
          value={command}
          disabled={!running}
          placeholder={t("console.placeholder")}
          onChange={(event) => setCommand(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              submit();
            } else if (event.key === "ArrowUp" && history.length) {
              // Command history: the last thing you typed is nearly always the
              // thing you want to type again.
              event.preventDefault();
              const next = Math.min(historyIndex + 1, history.length - 1);
              setHistoryIndex(next);
              setCommand(history[next] ?? "");
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              const next = historyIndex - 1;
              setHistoryIndex(next);
              setCommand(next >= 0 ? (history[next] ?? "") : "");
            }
          }}
        />
        <Button disabled={!running || !command.trim()} onClick={submit}>
          {t("actions.next")}
        </Button>
      </div>

      {send.error ? (
        <div className="p-3 pt-0">
          <ErrorNotice error={send.error} compact />
        </div>
      ) : null}
    </Panel>
  );
}
