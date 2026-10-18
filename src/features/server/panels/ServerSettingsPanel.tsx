/**
 * Per-server settings.
 *
 * Two halves. The automation half is ours and reads as plain language. The
 * game half is generated from the manifest — every field, its type, its range
 * and its help text come from the backend, so a second game's settings screen
 * exists the moment its manifest does.
 *
 * Both are laid out as a preferences pane: label and help on the left, the
 * control on the right, a line between rows. The save models stay explicit:
 * the automation card saves as you go and shows a faint "Saved" tick; the
 * game card collects edits behind its Save button.
 */

import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { keys, useGames, useInstalledAddons } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import type { InstanceView, SettingValue } from "@/types/api";
import { Button, ErrorNotice, Icon, Panel, Select, Skeleton, Switch, TextInput } from "@/ui";
import { RamSlider } from "@/ui/RamSlider";

import { JavaRuntimeCard } from "./settings/JavaRuntimeCard";
import { ServerIconCard } from "./settings/ServerIconCard";
import "./settings/settings-cards.css";

const SAVED_TICK_MS = 1200;

export function ServerSettingsPanel({ instance }: { instance: InstanceView }) {
  const { data: games } = useGames();
  const game = games?.find((candidate) => candidate.id === instance.game);
  const accent = game?.distributions.find((candidate) => candidate.id === instance.build.distribution)?.accent;
  return (
    <div className="flex max-w-[880px] flex-col gap-4">
      <AutomationSettings instance={instance} />
      {game?.server_icon ? <ServerIconCard instance={instance} accent={accent} /> : null}
      {game?.runtime.kind === "java" ? <JavaRuntimeCard instance={instance} /> : null}
      <GameSettings instance={instance} />
    </div>
  );
}

// -- preference rows ----------------------------------------------------------

/** A row whose control is a switch: the whole row is the label, so a click
 *  anywhere on it flips the switch. */
function SwitchRow({
  label,
  help,
  hint,
  checked,
  onChange,
  children,
}: {
  label: string;
  help?: string;
  hint?: ReactNode;
  checked: boolean;
  onChange: (value: boolean) => void;
  /** A dependent sub-row, shown under this one (e.g. "after N minutes"). */
  children?: ReactNode;
}) {
  return (
    <div className="pref-item">
      <label className="pref-row" data-control="switch">
        <PrefText label={label} help={help} hint={hint} />
        <Switch checked={checked} onChange={onChange} label={label} />
      </label>
      {children}
    </div>
  );
}

/** A row whose control is a field; the label points at it by id. */
function FieldRow({
  label,
  help,
  hint,
  children,
}: {
  label: string;
  help?: string;
  hint?: ReactNode;
  children: (id: string) => ReactNode;
}) {
  const id = useId();
  return (
    <div className="pref-item">
      <div className="pref-row">
        <PrefText label={label} help={help} hint={hint} htmlFor={id} />
        <div className="pref-control">{children(id)}</div>
      </div>
    </div>
  );
}

function PrefText({
  label,
  help,
  hint,
  htmlFor,
}: {
  label: string;
  help?: string;
  hint?: ReactNode;
  htmlFor?: string;
}) {
  const Label = htmlFor ? "label" : "span";
  return (
    <span className="pref-text">
      <Label className="pref-label" {...(htmlFor ? { htmlFor } : {})}>
        {label}
      </Label>
      {help ? <span className="pref-help">{help}</span> : null}
      {hint}
    </span>
  );
}

/** The faint tick an autosaving card shows after each save. */
function SavedTick({ show }: { show: boolean }) {
  const { t } = useI18n();
  return (
    <span className="saved-tick" data-show={show ? "" : undefined} aria-live="polite">
      {show ? (
        <>
          <Icon name="check" size={14} />
          {t("common.saved")}
        </>
      ) : null}
    </span>
  );
}

/** True for 1.2s after each call of the returned `flash`. */
function useSavedFlash(): [boolean, () => void] {
  const [show, setShow] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const flash = () => {
    window.clearTimeout(timer.current);
    setShow(true);
    timer.current = window.setTimeout(() => setShow(false), SAVED_TICK_MS);
  };
  return [show, flash];
}

// -- automation ---------------------------------------------------------------

function AutomationSettings({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const [saved, flashSaved] = useSavedFlash();

  const patch = useMutation({
    mutationFn: (values: Parameters<typeof api.instances.update>[1]) =>
      api.instances.update(instance.id, values),
    onSuccess: () => {
      flashSaved();
      return client.invalidateQueries({ queryKey: keys.instances });
    },
  });

  return (
    <Panel title={t("settings.groupAutomation")} actions={<SavedTick show={saved} />}>
      <div className="pref-list">
        <SwitchRow
          checked={instance.automation.sleep_when_empty}
          onChange={(value) => patch.mutate({ sleepWhenEmpty: value })}
          label={t("settings.sleepWhenEmpty")}
          help={t("settings.sleepWhenEmptyHelp")}
        >
          {instance.automation.sleep_when_empty ? (
            <div className="pref-sub">
              <label className="flex items-center gap-2">
                <span className="pref-sub-text">{t("settings.sleepAfter")}</span>
                <TextInput
                  type="number"
                  min={1}
                  max={720}
                  className="w-20"
                  value={instance.automation.sleep_after_minutes}
                  onChange={(event) => patch.mutate({ sleepAfterMinutes: Number(event.target.value) })}
                />
                <span className="pref-sub-text">{t("settings.minutes")}</span>
              </label>
            </div>
          ) : null}
        </SwitchRow>

        <SwitchRow
          checked={instance.automation.wake_on_join}
          onChange={(value) => patch.mutate({ wakeOnJoin: value })}
          label={t("settings.wakeOnJoin")}
          help={t("settings.wakeOnJoinHelp")}
        />

        <SwitchRow
          checked={instance.automation.restart_after_crash}
          onChange={(value) => patch.mutate({ restartAfterCrash: value })}
          label={t("settings.restartAfterCrash")}
        />

        <MemoryRow
          instance={instance}
          onSave={(memoryMb) => patch.mutate({ memoryMb })}
        />

        <FieldRow label={t("create.port")} help={t("settings.portHelp")}>
          {(id) => (
            <TextInput
              id={id}
              type="number"
              min={1024}
              max={65535}
              className="pref-number"
              defaultValue={Object.values(instance.ports)[0] ?? 25565}
              onBlur={(event) => patch.mutate({ port: Number(event.target.value) })}
            />
          )}
        </FieldRow>
      </div>

      {patch.error ? (
        <div className="px-5 pb-4">
          <ErrorNotice error={patch.error} />
        </div>
      ) : null}
    </Panel>
  );
}

/**
 * Memory as a slider across the full row width. The draft follows the thumb;
 * a save goes out once per gesture (see RamSlider's onCommit) and only when
 * the amount actually changed.
 */
function MemoryRow({
  instance,
  onSave,
}: {
  instance: InstanceView;
  onSave: (memoryMb: number) => void;
}) {
  const { t } = useI18n();
  const { data: games } = useGames();
  const labelId = useId();
  const sliderId = useId();

  const saved = instance.resources.memory_mb;
  const [draft, setDraft] = useState(saved);
  const [base, setBase] = useState(saved);
  // A save (or a change from elsewhere) moves the stored value: follow it.
  if (base !== saved) {
    setBase(saved);
    setDraft(saved);
  }

  const distribution = games
    ?.find((game) => game.id === instance.game)
    ?.distributions.find((candidate) => candidate.id === instance.build.distribution);
  // The recommendation counts the installed addons, so it must not depend on
  // whether Mods & Plugins was opened first: fetch the list here too (cached
  // ten minutes, three Modrinth requests per folder). Vanilla has none.
  const { data: addons } = useInstalledAddons(
    instance.id,
    Boolean(distribution?.addon_project_type),
  );

  return (
    <div className="pref-item">
      <div className="pref-row" data-layout="stacked">
        <span className="pref-text">
          <label id={labelId} className="pref-label" htmlFor={sliderId}>
            {t("create.memory")}
          </label>
          <span className="pref-help">{t("settings.memoryHelp")}</span>
          <span className="pref-hint">
            <Icon name="restart" size={12} />
            {t("settings.restartRequired")}
          </span>
        </span>
        <RamSlider
          id={sliderId}
          labelledBy={labelId}
          value={draft}
          onChange={setDraft}
          onCommit={(mb) => {
            if (mb !== saved) onSave(mb);
          }}
          projectType={distribution?.addon_project_type}
          addonCount={addons?.addons.length ?? 0}
          instanceId={instance.id}
        />
      </div>
    </div>
  );
}

// -- game ---------------------------------------------------------------------

function GameSettings({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const { data: games } = useGames();
  const [edits, setEdits] = useState<Record<string, string>>({});

  const settings = useQuery({
    queryKey: keys.instanceSettings(instance.id),
    queryFn: () => api.settings.instanceGet(instance.id),
  });

  const save = useMutation({
    mutationFn: (values: Record<string, string>) => api.settings.instanceSet(instance.id, values),
    onSuccess: () => {
      setEdits({});
      void client.invalidateQueries({ queryKey: keys.instanceSettings(instance.id) });
    },
  });

  const groups = useMemo(() => {
    const map = new Map<string, SettingValue[]>();
    for (const setting of settings.data ?? []) {
      const list = map.get(setting.group) ?? [];
      list.push(setting);
      map.set(setting.group, list);
    }
    return [...map.entries()];
  }, [settings.data]);

  const dirty = Object.keys(edits).length > 0;
  const valueOf = (setting: SettingValue) => edits[setting.key] ?? setting.value;
  const change = (key: string, value: string) => setEdits((current) => ({ ...current, [key]: value }));

  return (
    <Panel
      className="props-card"
      title={games?.find((game) => game.id === instance.game)?.name ?? instance.game}
      actions={
        <>
          {dirty ? (
            <span className="tag" data-tone="warn">
              {t("files.unsaved")}
            </span>
          ) : null}
          <Button
            size="sm"
            variant="primary"
            disabled={!dirty || save.isPending}
            onClick={() => save.mutate(edits)}
          >
            {save.isSuccess && !dirty ? t("actions.saved") : t("actions.save")}
          </Button>
        </>
      }
    >
      {settings.error ? (
        <div className="p-4">
          <ErrorNotice error={settings.error} onRetry={() => void settings.refetch()} />
        </div>
      ) : null}

      {settings.isLoading ? (
        <div className="flex flex-col gap-3 p-5" aria-hidden="true">
          {[0, 1, 2, 3].map((index) => (
            <Skeleton key={index} className="h-10" />
          ))}
        </div>
      ) : null}

      {groups.map(([group, fields]) => (
        <section key={group} className="pref-group">
          <h3 className="pref-group-title">{t(`settings.group${capitalise(group)}`)}</h3>
          <div className="pref-list">
            {fields.map((setting) => (
              <SettingControl
                key={setting.key}
                setting={setting}
                value={valueOf(setting)}
                edited={setting.key in edits}
                onChange={(value) => change(setting.key, value)}
              />
            ))}
          </div>
        </section>
      ))}

      {save.error ? (
        <div className="px-5 pb-4">
          <ErrorNotice error={save.error} />
        </div>
      ) : null}
    </Panel>
  );
}

function SettingControl({
  setting,
  value,
  edited,
  onChange,
}: {
  setting: SettingValue;
  value: string;
  edited: boolean;
  onChange: (value: string) => void;
}) {
  const { t } = useI18n();
  const label = t(setting.label_key);
  const help = setting.help_key ? t(setting.help_key) : undefined;
  const hint = setting.restart_required ? (
    <span className="pref-hint">
      <Icon name="restart" size={12} />
      {t("settings.restartRequired")}
    </span>
  ) : undefined;

  const row = (() => {
    if (setting.type === "bool") {
      return (
        <SwitchRow
          checked={value === "true"}
          onChange={(next) => onChange(String(next))}
          label={label}
          help={help}
          hint={hint}
        />
      );
    }

    if (setting.type === "choice") {
      return (
        <FieldRow label={label} help={help} hint={hint}>
          {(id) => (
            <Select id={id} value={value} onChange={(event) => onChange(event.target.value)}>
              {setting.options.map((option) => (
                <option key={option.value} value={option.value}>
                  {t(option.label_key)}
                </option>
              ))}
            </Select>
          )}
        </FieldRow>
      );
    }

    if (setting.type === "int") {
      return (
        <FieldRow label={label} help={help} hint={hint}>
          {(id) => (
            <TextInput
              id={id}
              type="number"
              className="pref-number"
              min={setting.min}
              max={setting.max}
              value={value}
              onChange={(event) => onChange(event.target.value)}
            />
          )}
        </FieldRow>
      );
    }

    return (
      <FieldRow label={label} help={help} hint={hint}>
        {(id) => (
          <TextInput
            id={id}
            value={value}
            maxLength={setting.max_len}
            onChange={(event) => onChange(event.target.value)}
          />
        )}
      </FieldRow>
    );
  })();

  // The edited mark is a 2px accent notch at the row's left, from CSS.
  return <div data-edited={edited ? "" : undefined}>{row}</div>;
}

function capitalise(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
