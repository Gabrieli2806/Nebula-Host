/** App settings: the machine, the language, and where things live. */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ACCENTS, type Accent } from "@/app/AccentSync";
import { keys, useAppSettings, useNode, useSaveSettings } from "@/hooks/useBackend";
import { useI18n, type Language } from "@/i18n";
import { api } from "@/lib/ipc";
import { Button, ErrorNotice, Field, Icon, Panel, Select, TextInput, Toggle } from "@/ui";

export function SettingsScreen() {
  const { t, language, setLanguage } = useI18n();
  const client = useQueryClient();
  const { data: node } = useNode();
  const { data: settings } = useAppSettings();
  const save = useSaveSettings();

  const [nodeName, setNodeName] = useState<string | null>(null);

  const rename = useMutation({
    mutationFn: (name: string) => api.node.rename(name),
    onSuccess: (updated) => {
      client.setQueryData(keys.node, updated);
      setNodeName(null);
    },
  });

  if (!settings) return null;

  const update = (patch: Partial<typeof settings>) => save.mutate({ ...settings, ...patch });

  return (
    <div className="mx-auto flex max-w-[760px] flex-col gap-4 p-6">
      <h1 className="text-[length:var(--text-2xl)] font-semibold tracking-[-0.01em]">
        {t("settings.title")}
      </h1>

      <Panel title={t("node.thisMachine")}>
        <div className="flex flex-col gap-4 p-4">
          <Field label={t("node.rename")}>
            <div className="flex gap-2">
              <TextInput
                value={nodeName ?? node?.name ?? ""}
                onChange={(event) => setNodeName(event.target.value)}
              />
              <Button
                disabled={!nodeName || nodeName === node?.name || rename.isPending}
                onClick={() => nodeName && rename.mutate(nodeName)}
              >
                {t("actions.save")}
              </Button>
            </div>
          </Field>
          {rename.error ? <ErrorNotice error={rename.error} compact /> : null}
        </div>
      </Panel>

      <Panel title={t("settings.title")}>
        <div className="flex flex-col gap-4 p-4">
          <div className="grid grid-cols-2 gap-4">
            <Field label={t("settings.language")}>
              <Select
                value={language}
                onChange={(event) => {
                  const value = event.target.value as Language;
                  setLanguage(value);
                  update({ language: value });
                }}
              >
                <option value="en">English</option>
                <option value="es">Español</option>
              </Select>
            </Field>

            <Field label={t("settings.density")}>
              <Select
                value={settings.density}
                onChange={(event) => update({ density: event.target.value })}
              >
                <option value="cards">{t("servers.densityCards")}</option>
                <option value="list">{t("servers.densityList")}</option>
              </Select>
            </Field>
          </div>

          <Field label={t("settings.accent")}>
            <AccentPicker
              value={(settings.accent as Accent) ?? "cyan"}
              onChange={(accent) => update({ accent })}
            />
          </Field>

          <Field label={t("settings.instancesDir")}>
            <div className="flex gap-2">
              <TextInput
                value={settings.instancesDir ?? ""}
                placeholder="…"
                onChange={(event) => update({ instancesDir: event.target.value })}
              />
              <Button
                icon="folder"
                onClick={async () => {
                  const { open } = await import("@tauri-apps/plugin-dialog");
                  const picked = await open({ directory: true });
                  if (typeof picked === "string") update({ instancesDir: picked });
                }}
              >
                {t("actions.openFolder")}
              </Button>
            </div>
          </Field>

          <div className="divider" />

          <Toggle
            checked={settings.launchAtStartup}
            onChange={(value) => update({ launchAtStartup: value })}
            label={t("settings.launchAtStartup")}
          />
          <Toggle
            checked={settings.runInTray}
            onChange={(value) => update({ runInTray: value })}
            label={t("settings.runInTray")}
          />
          <Toggle
            checked={settings.allowUpnp}
            onChange={(value) => update({ allowUpnp: value })}
            label={t("settings.allowUpnp")}
          />

          {/* Opening the app to the network is the one setting here that
              changes who can reach this machine, so it says so. */}
          <Toggle
            checked={settings.listenOnLan}
            onChange={(value) => update({ listenOnLan: value })}
            label={t("settings.listenOnLan")}
            help={t("settings.listenOnLanWarning")}
          />

          {save.error ? <ErrorNotice error={save.error} compact /> : null}
        </div>
      </Panel>

      <JavaSettings
        paths={settings.javaPaths}
        onChange={(javaPaths) => update({ javaPaths })}
      />
    </div>
  );
}

function JavaSettings({
  paths,
  onChange,
}: {
  paths: Record<string, string>;
  onChange: (paths: Record<string, string>) => void;
}) {
  const { t } = useI18n();
  const [probed, setProbed] = useState<Record<string, string>>({});

  const probe = useMutation({
    mutationFn: (path: string) => api.settings.probeJava(path),
    onSuccess: (installation) => {
      setProbed((current) => ({ ...current, [installation.path]: installation.version }));
      onChange({ ...paths, [String(installation.major)]: installation.path });
    },
  });

  return (
    <Panel title={t("settings.javaPaths")}>
      <div className="flex flex-col gap-4 p-4">
        <p className="text-[length:var(--text-sm)] text-[color:var(--text-muted)]">
          {t("settings.javaPathsHelp")}
        </p>

        {Object.entries(paths).length ? (
          <ul className="flex flex-col gap-2">
            {Object.entries(paths).map(([major, path]) => (
              <li key={major} className="flex items-center gap-3 rounded-[var(--radius)] border border-[color:var(--line)] px-3 py-2">
                <span className="mono flex-none rounded-[var(--radius-sm)] px-2 py-0.5 text-[length:var(--text-xs)]" style={{ background: "var(--accent-dim)", color: "var(--accent)" }}>
                  Java {major}
                </span>
                <span className="mono selectable min-w-0 flex-1 truncate text-[length:var(--text-xs)] text-[color:var(--text-muted)]">
                  {path}
                </span>
                {probed[path] ? (
                  <span className="flex flex-none items-center gap-1 text-[length:var(--text-xs)] text-[color:var(--status-online)]">
                    <Icon name="check" size={12} />
                    {probed[path]}
                  </span>
                ) : null}
                <Button
                  size="sm"
                  variant="quiet"
                  icon="close"
                  iconOnly
                  onClick={() => {
                    const next = { ...paths };
                    delete next[major];
                    onChange(next);
                  }}
                >
                  {t("actions.remove")}
                </Button>
              </li>
            ))}
          </ul>
        ) : null}

        <Button
          icon="plus"
          onClick={async () => {
            const { open } = await import("@tauri-apps/plugin-dialog");
            const picked = await open({
              multiple: false,
              filters: [{ name: "java", extensions: ["exe", ""] }],
            });
            if (typeof picked === "string") probe.mutate(picked);
          }}
        >
          {t("players.add")}
        </Button>

        {probe.error ? <ErrorNotice error={probe.error} /> : null}
      </div>
    </Panel>
  );
}

/**
 * Six accents, shown as what they are. The swatch is the label — a dropdown
 * listing colour names would be a worse control than the colours themselves.
 */
function AccentPicker({ value, onChange }: { value: Accent; onChange: (accent: Accent) => void }) {
  return (
    <div className="flex flex-wrap gap-2">
      {ACCENTS.map((accent) => {
        const selected = accent === value;
        return (
          <button
            key={accent}
            type="button"
            aria-label={accent}
            aria-pressed={selected}
            onClick={() => onChange(accent)}
            // Each swatch previews its own accent by scoping the token to
            // itself, so the row shows real colours rather than six greys.
            data-accent={accent}
            className="flex h-9 w-9 items-center justify-center rounded-full border transition-transform"
            style={{
              borderColor: selected ? "var(--accent)" : "var(--line-strong)",
              background: "var(--accent-dim)",
              transform: selected ? "scale(1.05)" : undefined,
            }}
          >
            <span
              className="block h-4 w-4 rounded-full"
              style={{ background: "var(--accent)" }}
            />
          </button>
        );
      })}
    </div>
  );
}
