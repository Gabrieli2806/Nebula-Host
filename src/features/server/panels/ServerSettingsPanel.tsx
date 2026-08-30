/**
 * Per-server settings.
 *
 * Two halves. The automation half is ours and reads as plain language. The
 * game half is generated from the manifest — every field, its type, its range
 * and its help text come from the backend, so a second game's settings screen
 * exists the moment its manifest does.
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { keys, useGames } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import type { InstanceView, SettingValue } from "@/types/api";
import { Button, ErrorNotice, Field, Panel, Select, TextInput, Toggle } from "@/ui";

export function ServerSettingsPanel({ instance }: { instance: InstanceView }) {
  return (
    <div className="flex flex-col gap-4">
      <AutomationSettings instance={instance} />
      <GameSettings instance={instance} />
    </div>
  );
}

function AutomationSettings({ instance }: { instance: InstanceView }) {
  const { t } = useI18n();
  const client = useQueryClient();

  const patch = useMutation({
    mutationFn: (values: Parameters<typeof api.instances.update>[1]) =>
      api.instances.update(instance.id, values),
    onSuccess: () => client.invalidateQueries({ queryKey: keys.instances }),
  });

  return (
    <Panel title={t("settings.groupAutomation")}>
      <div className="flex flex-col gap-4 p-4">
        <Toggle
          checked={instance.automation.sleep_when_empty}
          onChange={(value) => patch.mutate({ sleepWhenEmpty: value })}
          label={t("settings.sleepWhenEmpty")}
          help={t("settings.sleepWhenEmptyHelp")}
        />

        {instance.automation.sleep_when_empty ? (
          <div className="ml-[44px] flex max-w-[280px] items-center gap-2">
            <span className="text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
              {t("settings.sleepAfter")}
            </span>
            <TextInput
              type="number"
              min={1}
              max={720}
              className="w-20"
              value={instance.automation.sleep_after_minutes}
              onChange={(event) => patch.mutate({ sleepAfterMinutes: Number(event.target.value) })}
            />
            <span className="text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
              {t("settings.minutes")}
            </span>
          </div>
        ) : null}

        <Toggle
          checked={instance.automation.wake_on_join}
          onChange={(value) => patch.mutate({ wakeOnJoin: value })}
          label={t("settings.wakeOnJoin")}
          help={t("settings.wakeOnJoinHelp")}
        />

        <Toggle
          checked={instance.automation.restart_after_crash}
          onChange={(value) => patch.mutate({ restartAfterCrash: value })}
          label={t("settings.restartAfterCrash")}
        />

        <div className="divider" />

        <div className="grid max-w-[520px] grid-cols-2 gap-4">
          <Field label={t("create.memory")}>
            <Select
              value={String(instance.resources.memory_mb)}
              onChange={(event) => patch.mutate({ memoryMb: Number(event.target.value) })}
            >
              {[2048, 4096, 6144, 8192, 12288, 16384].map((value) => (
                <option key={value} value={value}>
                  {value / 1024} GB
                </option>
              ))}
            </Select>
          </Field>

          <Field label={t("create.port")}>
            <TextInput
              type="number"
              min={1024}
              max={65535}
              defaultValue={Object.values(instance.ports)[0] ?? 25565}
              onBlur={(event) => patch.mutate({ port: Number(event.target.value) })}
            />
          </Field>
        </div>

        {patch.error ? <ErrorNotice error={patch.error} /> : null}
      </div>
    </Panel>
  );
}

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
      title={games?.find((game) => game.id === instance.game)?.name ?? instance.game}
      actions={
        <Button
          size="sm"
          variant="primary"
          disabled={!dirty || save.isPending}
          onClick={() => save.mutate(edits)}
        >
          {save.isSuccess && !dirty ? t("actions.saved") : t("actions.save")}
        </Button>
      }
    >
      {settings.error ? (
        <div className="p-4">
          <ErrorNotice error={settings.error} onRetry={() => void settings.refetch()} />
        </div>
      ) : null}

      <div className="flex flex-col gap-6 p-4">
        {groups.map(([group, fields]) => (
          <section key={group} className="flex flex-col gap-4">
            <h3 className="label">{t(`settings.group${capitalise(group)}`)}</h3>
            <div className="grid max-w-[720px] grid-cols-2 gap-4">
              {fields.map((setting) => (
                <SettingControl
                  key={setting.key}
                  setting={setting}
                  value={valueOf(setting)}
                  onChange={(value) => change(setting.key, value)}
                />
              ))}
            </div>
          </section>
        ))}
      </div>

      {save.error ? (
        <div className="p-4 pt-0">
          <ErrorNotice error={save.error} />
        </div>
      ) : null}
    </Panel>
  );
}

function SettingControl({
  setting,
  value,
  onChange,
}: {
  setting: SettingValue;
  value: string;
  onChange: (value: string) => void;
}) {
  const { t } = useI18n();
  const label = t(setting.label_key);
  const help = setting.help_key ? t(setting.help_key) : undefined;
  const hint = setting.restart_required ? (
    <span className="text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
      {t("settings.restartRequired")}
    </span>
  ) : undefined;

  if (setting.type === "bool") {
    return (
      <div className="col-span-2">
        <Toggle checked={value === "true"} onChange={(next) => onChange(String(next))} label={label} help={help} />
      </div>
    );
  }

  if (setting.type === "choice") {
    return (
      <Field label={label} help={help} hint={hint}>
        <Select value={value} onChange={(event) => onChange(event.target.value)}>
          {setting.options.map((option) => (
            <option key={option.value} value={option.value}>
              {t(option.label_key)}
            </option>
          ))}
        </Select>
      </Field>
    );
  }

  if (setting.type === "int") {
    return (
      <Field label={label} help={help} hint={hint}>
        <TextInput
          type="number"
          min={setting.min}
          max={setting.max}
          value={value}
          onChange={(event) => onChange(event.target.value)}
        />
      </Field>
    );
  }

  return (
    <Field label={label} help={help} hint={hint}>
      <TextInput
        value={value}
        maxLength={setting.max_len}
        onChange={(event) => onChange(event.target.value)}
      />
    </Field>
  );
}

function capitalise(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
