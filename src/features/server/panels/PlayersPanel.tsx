/**
 * Players: who is on right now, and the lists that decide who may be.
 *
 * The list names come from the game's manifest, so a game with different lists
 * renders here without a change.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { keys } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import type { InstanceView, PlayerEntry } from "@/types/api";
import { Button, EmptyState, ErrorNotice, Panel, TextInput } from "@/ui";

export function PlayersPanel({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const client = useQueryClient();

  const online = useQuery({
    queryKey: keys.playersOnline(instance.id),
    queryFn: () => api.players.online(instance.id),
  });

  const lists = useQuery({
    queryKey: keys.players(instance.id),
    queryFn: () => api.players.lists(instance.id),
  });

  const write = useMutation({
    mutationFn: ({ list, entries }: { list: string; entries: PlayerEntry[] }) =>
      api.players.writeList(instance.id, list, entries),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.players(instance.id) }),
  });

  return (
    <div className="flex flex-col gap-4">
      <Panel title={t("players.online")}>
        {online.data?.length ? (
          <ul className="flex flex-wrap gap-2 p-4">
            {online.data.map((name) => (
              <li key={name} className="flex items-center gap-2 rounded-full border border-[color:var(--line-strong)] py-1 pl-1 pr-3">
                <Avatar name={name} />
                <span className="text-[length:var(--text-sm)]">{name}</span>
              </li>
            ))}
          </ul>
        ) : (
          <EmptyState icon="users" title={t("players.none")} />
        )}
      </Panel>

      {lists.error ? <ErrorNotice error={lists.error} onRetry={() => void lists.refetch()} /> : null}

      {lists.data
        ? Object.entries(lists.data).map(([list, entries]) => (
            <PlayerList
              key={list}
              title={t(`players.${list}`)}
              entries={entries}
              onChange={(next) => write.mutate({ list, entries: next })}
            />
          ))
        : null}

      {write.error ? <ErrorNotice error={write.error} compact /> : null}
    </div>
  );
}

function PlayerList({
  title,
  entries,
  onChange,
}: {
  title: string;
  entries: PlayerEntry[];
  onChange: (entries: PlayerEntry[]) => void;
}) {
  const { t } = useI18n();
  const [name, setName] = useState("");

  const add = () => {
    const trimmed = name.trim();
    if (!trimmed || entries.some((entry) => entry.name === trimmed)) return;
    onChange([...entries, { name: trimmed }]);
    setName("");
  };

  return (
    <Panel title={title}>
      <div className="flex flex-col gap-3 p-4">
        {entries.length === 0 ? (
          <p className="text-[color:var(--text-muted)]">{t("players.none")}</p>
        ) : (
          <ul className="flex flex-col gap-1">
            {entries.map((entry) => (
              <li
                key={entry.name}
                className="flex items-center gap-3 rounded-[var(--radius)] px-2 py-1.5 hover:bg-[color:var(--surface-raised)]"
              >
                <Avatar name={entry.name} />
                <span className="flex-1 truncate">{entry.name}</span>
                {entry.level ? (
                  <span className="mono text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
                    level {entry.level}
                  </span>
                ) : null}
                <Button
                  size="sm"
                  variant="quiet"
                  icon="close"
                  iconOnly
                  onClick={() => onChange(entries.filter((candidate) => candidate.name !== entry.name))}
                >
                  {t("actions.remove")}
                </Button>
              </li>
            ))}
          </ul>
        )}

        <div className="flex gap-2">
          <TextInput
            value={name}
            placeholder={t("players.namePlaceholder")}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => event.key === "Enter" && add()}
          />
          <Button icon="plus" disabled={!name.trim()} onClick={add}>
            {t("players.add")}
          </Button>
        </div>
      </div>
    </Panel>
  );
}

/**
 * Faces come from the player's name, not their UUID: servers in offline mode
 * hand out name-derived UUIDs, and looking a skin up by one of those returns a
 * placeholder with artefacts.
 */
function Avatar({ name }: { name: string }) {
  return (
    <img
      src={`https://mc-heads.net/avatar/${encodeURIComponent(name)}/32`}
      alt=""
      width={24}
      height={24}
      className="flex-none rounded-[var(--radius-sm)]"
      style={{ imageRendering: "pixelated", background: "var(--surface-raised)" }}
      loading="lazy"
    />
  );
}
