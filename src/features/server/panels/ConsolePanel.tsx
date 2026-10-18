/**
 * The console.
 *
 * A deep, inset console window inside a card, dark in both themes. It follows new output
 * while you are at the bottom and stops the moment you scroll up: reading
 * back through a crash while the view yanks you away is the fastest way to
 * make a log useless. While follow is broken, a "Jump to latest" button counts
 * what arrived below.
 *
 * The card takes the rest of the ServerScreen column (flex-1, at least
 * 360px), so its height never depends on the header above it.
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { keys, useConsole, useConsoleCompletions, usePlayerIdentities } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import type { ConsoleLine, InstanceView } from "@/types/api";
import { Button, EmptyState, ErrorNotice, Icon, Panel } from "@/ui";

import { CommandPrompt } from "./console/CommandPrompt";
import { ConsoleLineView } from "./console/ConsoleLineView";
import { ShareLog } from "./console/ShareLog";
import "./ConsolePanel.css";

/** How close to the bottom (px) still counts as "at the bottom". */
const FOLLOW_THRESHOLD = 24;

export function ConsolePanel({ instance }: { instance: InstanceView }) {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const { data: lines = [] } = useConsole(instance.id);
  const [following, setFollowing] = useState(true);
  /** The last line on screen when follow broke; everything after it is new. */
  const [lastSeen, setLastSeen] = useState<ConsoleLine | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const latest = useRef<ConsoleLine[]>(lines);

  const running = instance.status === "online" || instance.status === "starting";
  // Suggestions: the builtin list right away, the server's own once it has
  // answered `help` (the hook refetches when the status changes).
  const { data: completions, isFetching: loadingCompletions } = useConsoleCompletions(
    instance.id,
    running,
  );
  const { data: identities } = usePlayerIdentities(instance.id);
  /** Lower-cased name → UUID, built once per identities change. */
  const uuids = useMemo(() => {
    const map = new Map<string, string>();
    for (const [name, uuid] of Object.entries(identities ?? {})) map.set(name.toLowerCase(), uuid);
    return map;
  }, [identities]);
  const uuidOf = useCallback((name: string) => uuids.get(name.toLowerCase()), [uuids]);

  const send = useMutation({
    mutationFn: (value: string) => api.instances.command(instance.id, value),
  });

  useEffect(() => {
    latest.current = lines;
  }, [lines]);

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
      const atBottom = distance < FOLLOW_THRESHOLD;
      setFollowing(atBottom);
      setLastSeen((previous) =>
        atBottom ? null : (previous ?? latest.current[latest.current.length - 1] ?? null),
      );
    };
    element.addEventListener("scroll", onScroll);
    return () => element.removeEventListener("scroll", onScroll);
  }, []);

  const unseen = following ? 0 : countUnseen(lines, lastSeen);

  const jumpToLatest = () => {
    setLastSeen(null);
    setFollowing(true);
  };

  const clear = () => {
    client.setQueryData<ConsoleLine[]>(keys.console(instance.id), []);
    jumpToLatest();
  };


  return (
    <Panel
      title={
        lines.length ? (
          <span className="panel-count">
            {lines.length === 1
              ? t("console.lineCount_one")
              : t("console.lineCount", { count: lines.length.toLocaleString(language) })}
          </span>
        ) : undefined
      }
      className="console-plate"
      bodyClassName="console-plate-body"
      actions={
        <>
          <ShareLog instanceId={instance.id} consoleLines={lines.length} />
          <Button variant="quiet" size="sm" disabled={!lines.length} onClick={clear}>
            {t("console.clear")}
          </Button>
        </>
      }
    >
      <div className="console-window">
        <div
          ref={scroller}
          className="console selectable h-full overflow-auto py-2"
          role="log"
          aria-label={t("console.title")}
          tabIndex={0}
        >
          {lines.length === 0 ? (
            <EmptyState icon="terminal" title={t("console.empty")} className="console-empty" />
          ) : (
            lines.map((line) => (
              <ConsoleLineView
                key={lineKey(line)}
                line={line}
                language={language}
                uuid={line.speaker ? uuidOf(line.speaker.name) : undefined}
              />
            ))
          )}
        </div>

        {!following ? (
          <button type="button" className="console-jump console-jump-pos" onClick={jumpToLatest}>
            <Icon name="arrow-down" size={13} />
            {unseen > 0
              ? t("console.jumpToLatest", { count: unseen })
              : t("console.jumpToLatestPlain")}
          </button>
        ) : null}
      </div>

      <CommandPrompt
        running={running}
        online={instance.status === "online"}
        completions={completions}
        loadingCompletions={loadingCompletions}
        uuidOf={uuidOf}
        onSubmit={(value) => send.mutate(value)}
      />

      {send.error ? (
        <div className="px-4 pb-4">
          <ErrorNotice error={send.error} compact />
        </div>
      ) : null}
    </Panel>
  );
}

/**
 * Lines that arrived after `lastSeen`, the line on screen when follow broke.
 * Once the cache is trimmed to CONSOLE_LIMIT that object can fall out of the
 * buffer; then count by timestamp instead of reporting the whole buffer.
 */
function countUnseen(lines: ConsoleLine[], lastSeen: ConsoleLine | null): number {
  if (!lastSeen) return lines.length;
  const index = lines.lastIndexOf(lastSeen);
  if (index >= 0) return lines.length - 1 - index;
  const seenAt = Date.parse(lastSeen.timestamp);
  if (Number.isNaN(seenAt)) return 0;
  return lines.filter((line) => Date.parse(line.timestamp) > seenAt).length;
}

/**
 * A stable key per line object. The buffer drops its oldest line once full,
 * so an index-based key would change for every row on every new line and
 * remount them all (heads included). Line objects keep their identity
 * across those slices, so a sequence number handed out on first sight holds.
 */
const lineIds = new WeakMap<ConsoleLine, number>();
let nextLineId = 0;
function lineKey(line: ConsoleLine): number {
  let id = lineIds.get(line);
  if (id === undefined) {
    id = nextLineId++;
    lineIds.set(line, id);
  }
  return id;
}
