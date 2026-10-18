/**
 * Players: who is on right now, and the lists that decide who may be.
 *
 * The list names come from the game's manifest, so a game with different lists
 * renders here without a change. They share one card and switch by tab,
 * because a person edits one list at a time.
 *
 * Quick actions on online players (operator, kick, ban) go through the game's
 * own console commands, so the running server applies them at once and writes
 * its files itself. An action is offered only when the game's command list
 * (manifest builtins, plus whatever the server reports) has that command, so a
 * game without `kick` simply shows no kick.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { findUuid, keys, useConsoleCompletions, usePlayerIdentities } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import type { InstanceView, PlayerEntry } from "@/types/api";
import {
  Button,
  Dialog,
  ErrorNotice,
  Field,
  PlayerHead,
  Skeleton,
  StatusDot,
  Tabs,
  TextInput,
} from "@/ui";

import "./PlayersPanel.css";

/**
 * Player names never hold whitespace. Anything else is left to the game:
 * proxies such as Geyser prefix Bedrock names with `.` or `*`.
 */
const NAME_PATTERN = /^\S+$/;

type Pending = { kind: "kick" | "ban"; name: string } | null;

export function PlayersPanel({ instance }: { instance: InstanceView }) {
  const { t: tr } = useI18n();
  const client = useQueryClient();
  const [chosen, setChosen] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending>(null);

  const online = useQuery({
    queryKey: keys.playersOnline(instance.id),
    queryFn: () => api.players.online(instance.id),
  });

  const lists = useQuery({
    queryKey: keys.players(instance.id),
    queryFn: () => api.players.lists(instance.id),
  });

  const identities = usePlayerIdentities(instance.id);
  const isOnline = instance.status === "online";
  const completions = useConsoleCompletions(instance.id, isOnline);

  const write = useMutation({
    mutationFn: ({ list, entries }: { list: string; entries: PlayerEntry[] }) =>
      api.players.writeList(instance.id, list, entries),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.players(instance.id) }),
  });

  // The server writes its list files a moment after it runs the command; read
  // them back once it has. Timers are cleared if the panel goes away first.
  const timers = useRef<number[]>([]);
  useEffect(() => () => timers.current.forEach((id) => window.clearTimeout(id)), []);

  const command = useMutation({
    mutationFn: (line: string) => api.instances.command(instance.id, line),
    onSuccess: () => {
      const refresh = () => {
        void client.invalidateQueries({ queryKey: keys.players(instance.id) });
        void client.invalidateQueries({ queryKey: keys.playersOnline(instance.id) });
      };
      timers.current.push(window.setTimeout(refresh, 700), window.setTimeout(refresh, 2000));
    },
  });

  const available = useMemo(
    () => new Set((completions.data?.commands ?? []).map((entry) => entry.name)),
    [completions.data],
  );
  const can = (name: string) => isOnline && available.has(name);

  const names = lists.data ? Object.keys(lists.data) : [];
  // Fall back to the first list when nothing is chosen (or the chosen one is gone).
  const active = chosen && names.includes(chosen) ? chosen : names[0];
  const people = online.data ?? [];
  const operators = new Set(
    (lists.data?.operators ?? []).map((entry) => entry.name.toLowerCase()),
  );
  const uuidOf = (name: string, fallback?: string | null) =>
    fallback ?? findUuid(identities.data, name);

  const runAction = (line: string, after?: () => void) =>
    command.mutate(line, { onSuccess: () => after?.() });

  return (
    <div className="players">
      <section className="panel-flush" aria-labelledby="players-online-title">
        <header className="panel-header">
          <h2 id="players-online-title" className="panel-title flex items-center gap-2.5">
            <StatusDot status={people.length ? "online" : "offline"} />
            {tr("players.onlineNow")}
          </h2>
          <span className="readout players-count">
            {people.length}
            {instance.players_max ? (
              <span className="players-count-max">/{instance.players_max}</span>
            ) : null}
          </span>
        </header>

        {people.length ? (
          <ul className="players-online-grid">
            {people.map((name) => {
              const isOp = operators.has(name.toLowerCase());
              const actions = [
                isOp
                  ? can("deop") && { id: "deop", label: tr("players.deop"), run: () => runAction(`deop ${name}`) }
                  : can("op") && { id: "op", label: tr("players.op"), run: () => runAction(`op ${name}`) },
                can("kick") && {
                  id: "kick",
                  label: tr("players.kick"),
                  run: () => setPending({ kind: "kick", name }),
                },
                can("ban") && {
                  id: "ban",
                  label: tr("players.ban"),
                  danger: true,
                  run: () => setPending({ kind: "ban", name }),
                },
              ].filter(Boolean) as { id: string; label: string; danger?: boolean; run: () => void }[];

              return (
                <li key={name} className="player-card">
                  <div className="player-card-main">
                    <PlayerHead name={name} uuid={uuidOf(name)} size={64} className="player-card-head" />
                    <span className="player-card-title">
                      <span className="player-card-name" title={name}>
                        {name}
                      </span>
                      {isOp ? <span className="tag">{tr("players.operatorTag")}</span> : null}
                    </span>
                  </div>
                  {actions.length ? (
                    <div
                      className="player-card-actions"
                      role="group"
                      aria-label={tr("players.actionsFor", { name })}
                    >
                      {actions.map((action) => (
                        <Button
                          key={action.id}
                          size="sm"
                          variant="quiet"
                          className={action.danger ? "player-action-danger" : ""}
                          disabled={command.isPending}
                          onClick={action.run}
                        >
                          {action.label}
                        </Button>
                      ))}
                    </div>
                  ) : null}
                </li>
              );
            })}
          </ul>
        ) : (
          <div className="players-quiet">
            {online.isLoading ? <Skeleton className="h-5 w-40" /> : <p>{tr("players.nobodyOnline")}</p>}
          </div>
        )}

        {command.error ? (
          <div className="px-4 pb-4">
            <ErrorNotice error={command.error} compact />
          </div>
        ) : null}
      </section>

      {lists.error ? <ErrorNotice error={lists.error} onRetry={() => void lists.refetch()} /> : null}

      {lists.isLoading ? <Skeleton className="h-[220px] rounded-[var(--radius-lg)]" /> : null}

      {lists.data && active ? (
        <section className="panel-flush">
          <header className="panel-header">
            <Tabs<string>
              label={tr("players.title")}
              size="sm"
              active={active}
              onChange={setChosen}
              tabs={names.map((list) => ({ id: list, label: tr(`players.${list}`) }))}
            />
            <span className="readout panel-count">{lists.data[active]?.length ?? 0}</span>
          </header>
          <PlayerList
            key={active}
            list={active}
            title={tr(`players.${active}`)}
            entries={lists.data[active] ?? []}
            uuidOf={uuidOf}
            running={isOnline}
            saving={write.isPending}
            onChange={(next) => write.mutate({ list: active, entries: next })}
          />
          {write.error ? (
            <div className="px-4 pb-4">
              <ErrorNotice error={write.error} compact />
            </div>
          ) : null}
        </section>
      ) : null}

      <ActionDialog
        key={pending ? `${pending.kind}:${pending.name}` : "closed"}
        pending={pending}
        busy={command.isPending}
        onClose={() => setPending(null)}
        onConfirm={(reason) => {
          if (!pending) return;
          const tail = reason ? ` ${reason}` : "";
          runAction(`${pending.kind} ${pending.name}${tail}`, () => setPending(null));
        }}
      />
    </div>
  );
}

/** Kick or ban confirmation, with an optional reason the player sees. */
function ActionDialog({
  pending,
  busy,
  onClose,
  onConfirm,
}: {
  pending: Pending;
  busy: boolean;
  onClose: () => void;
  onConfirm: (reason: string) => void;
}) {
  const { t: tr } = useI18n();
  // The parent keys this dialog by player and action, so each opening starts
  // with an empty reason.
  const [reason, setReason] = useState("");

  const isBan = pending?.kind === "ban";
  // Commands are one line; a newline would end this one and start another.
  const clean = reason.replace(/[\r\n]+/g, " ").trim();

  return (
    <Dialog
      open={pending !== null}
      onClose={onClose}
      width={460}
      title={
        pending
          ? tr(isBan ? "players.banTitle" : "players.kickTitle", { name: pending.name })
          : ""
      }
      footer={
        <>
          <Button variant="quiet" onClick={onClose}>
            {tr("actions.cancel")}
          </Button>
          <Button
            variant={isBan ? "danger" : "default"}
            disabled={busy}
            onClick={() => onConfirm(clean)}
          >
            {tr(isBan ? "players.ban" : "players.kick")}
          </Button>
        </>
      }
    >
      {pending ? (
        <form
          className="players-dialog"
          onSubmit={(event) => {
            event.preventDefault();
            if (!busy) onConfirm(clean);
          }}
        >
          <div className="players-dialog-who">
            <PlayerHead name={pending.name} size={48} />
            <div className="min-w-0">
              <p className="players-dialog-name">{pending.name}</p>
              <p className="players-dialog-hint">
                {tr(isBan ? "players.banHint" : "players.kickHint")}
              </p>
            </div>
          </div>
          <Field label={tr("players.reasonOptional")}>
            <TextInput
              value={reason}
              maxLength={200}
              placeholder={tr("players.reasonPlaceholder")}
              onChange={(event) => setReason(event.target.value)}
            />
          </Field>
        </form>
      ) : null}
    </Dialog>
  );
}

function PlayerList({
  list,
  title,
  entries,
  uuidOf,
  running,
  saving,
  onChange,
}: {
  list: string;
  title: string;
  entries: PlayerEntry[];
  uuidOf: (name: string, fallback?: string | null) => string | undefined | null;
  running: boolean;
  saving: boolean;
  onChange: (entries: PlayerEntry[]) => void;
}) {
  const { t: tr } = useI18n();
  const [name, setName] = useState("");
  const [reason, setReason] = useState("");
  const [removing, setRemoving] = useState<PlayerEntry | null>(null);
  // Bans carry a reason; the other lists do not. The list id is the
  // manifest's, the same one the tab label is keyed on.
  const takesReason = list === "banned";

  const trimmed = name.trim();
  const duplicate = entries.some((entry) => entry.name.toLowerCase() === trimmed.toLowerCase());
  const invalid = trimmed !== "" && !NAME_PATTERN.test(trimmed);
  const problem = invalid
    ? tr("players.invalidName")
    : duplicate
      ? tr("players.duplicateName", { name: trimmed })
      : undefined;

  const add = () => {
    if (!trimmed || problem) return;
    const entry: PlayerEntry = { name: trimmed };
    const why = reason.trim();
    if (takesReason && why) entry.reason = why;
    onChange([...entries, entry]);
    setName("");
    setReason("");
  };

  return (
    <div role="tabpanel" aria-label={title}>
      {entries.length === 0 ? (
        <p className="players-quiet">{tr("players.none")}</p>
      ) : (
        <ul className="players-list">
          {entries.map((entry) => (
            <li key={entry.name} className="panel-row players-row">
              <PlayerHead name={entry.name} uuid={uuidOf(entry.name, entry.uuid)} size={32} />
              <span className="players-row-text">
                <span className="players-row-name">{entry.name}</span>
                {entry.reason ? (
                  <span className="players-row-reason" title={entry.reason}>
                    {entry.reason}
                  </span>
                ) : null}
              </span>
              {entry.level ? (
                <span className="tag">{tr("players.level", { level: entry.level })}</span>
              ) : null}
              <Button
                size="sm"
                variant="quiet"
                icon="close"
                iconOnly
                className="row-action"
                disabled={saving}
                onClick={() => setRemoving(entry)}
              >
                {`${tr("actions.remove")} ${entry.name}`}
              </Button>
            </li>
          ))}
        </ul>
      )}

      <form
        className="players-add"
        data-reason={takesReason ? "" : undefined}
        onSubmit={(event) => {
          event.preventDefault();
          add();
        }}
      >
        <div className="players-add-fields">
          <TextInput
            value={name}
            maxLength={40}
            spellCheck={false}
            autoComplete="off"
            aria-label={tr("players.namePlaceholder")}
            aria-invalid={problem ? true : undefined}
            aria-describedby={problem ? "players-add-problem" : undefined}
            placeholder={tr("players.namePlaceholder")}
            className="players-add-name"
            onChange={(event) => setName(event.target.value)}
          />
          {takesReason ? (
            <TextInput
              value={reason}
              maxLength={200}
              aria-label={tr("players.reasonOptional")}
              placeholder={tr("players.reasonOptional")}
              className="players-add-reason"
              onChange={(event) => setReason(event.target.value)}
            />
          ) : null}
          <Button type="submit" icon="plus" disabled={!trimmed || Boolean(problem) || saving}>
            {tr("players.add")}
          </Button>
        </div>
        {problem ? (
          <p id="players-add-problem" className="players-add-problem" role="alert">
            {problem}
          </p>
        ) : running ? (
          <p className="players-add-note">{tr("players.runningListHint")}</p>
        ) : null}
      </form>

      <Dialog
        open={removing !== null}
        onClose={() => setRemoving(null)}
        width={440}
        title={removing ? tr("players.removeTitle", { name: removing.name, list: title }) : ""}
        footer={
          <>
            <Button variant="quiet" onClick={() => setRemoving(null)}>
              {tr("actions.cancel")}
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                if (removing) {
                  onChange(entries.filter((candidate) => candidate.name !== removing.name));
                }
                setRemoving(null);
              }}
            >
              {tr("actions.remove")}
            </Button>
          </>
        }
      >
        {removing ? (
          <div className="players-dialog-who">
            <PlayerHead name={removing.name} uuid={uuidOf(removing.name, removing.uuid)} size={48} />
            <div className="min-w-0">
              <p className="players-dialog-name">{removing.name}</p>
              <p className="players-dialog-hint">{tr("players.removeHint")}</p>
            </div>
          </div>
        ) : null}
      </Dialog>
    </div>
  );
}
