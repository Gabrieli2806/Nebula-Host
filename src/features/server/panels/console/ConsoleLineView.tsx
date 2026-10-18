/**
 * One console line.
 *
 * Plain lines are the timestamp gutter plus the message, coloured by level.
 * Lines the backend tagged with a `speaker` (chat, join, leave) carry the
 * player's 16px head at the start: chat reads as "head · name · message",
 * joins and leaves as a quieter "head · Steve joined the game". The log
 * prefix (`[Async Chat Thread - #0/INFO]:`) is dropped from view since the
 * gutter already has the time; the raw line stays in the tooltip.
 *
 * Memoised: the buffer re-renders on every new line, the old lines do not.
 */

import { memo } from "react";

import { formatTime } from "@/lib/format";
import type { ConsoleLine } from "@/types/api";
import { PlayerHead } from "@/ui";

export const ConsoleLineView = memo(function ConsoleLineView({
  line,
  language,
  uuid,
}: {
  line: ConsoleLine;
  language: string;
  /** Account UUID of the speaker, when known. */
  uuid?: string;
}) {
  const time = <span className="console-time">{formatTime(line.timestamp, language)}</span>;
  const speaker = line.speaker;

  if (!speaker) {
    return (
      <div
        className={`console-line console-${line.level}`}
        title={line.body ? line.message : undefined}
      >
        {time}
        <span>{line.body ?? line.message}</span>
      </div>
    );
  }

  if (speaker.kind === "chat") {
    return (
      <div className={`console-line console-${line.level} console-chat`} title={line.message}>
        {time}
        <span className="console-said">
          <PlayerHead name={speaker.name} uuid={uuid} size={16} className="console-head" />
          <span className="console-speaker">{speaker.name}</span>
          <span className="console-chat-text">{speaker.message ?? afterName(line.message, speaker.name)}</span>
        </span>
      </div>
    );
  }

  return (
    <div
      className={`console-line console-${line.level} console-presence`}
      data-kind={speaker.kind}
      title={line.message}
    >
      {time}
      <span className="console-said">
        <PlayerHead name={speaker.name} uuid={uuid} size={16} className="console-head" />
        <span>{fromName(line.message, speaker.name)}</span>
      </span>
    </div>
  );
});

/** "…]: Steve joined the game" → "Steve joined the game". */
function fromName(message: string, name: string): string {
  const prefix = message.lastIndexOf("]: ");
  const tail = prefix >= 0 ? message.slice(prefix + 3) : message;
  const at = tail.indexOf(name);
  return at >= 0 ? tail.slice(at) : tail;
}

/** "…<Steve> hola" → "hola" (only when the backend left `message` out). */
function afterName(message: string, name: string): string {
  const marker = `${name}> `;
  const at = message.indexOf(marker);
  return at >= 0 ? message.slice(at + marker.length) : message;
}
