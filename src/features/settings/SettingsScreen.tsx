/**
 * App settings: the machine, how Nebula Host looks, the language, where
 * things live, how it starts, and which Java it runs.
 *
 * One left-aligned column of cards. Each setting is a preferences-pane row:
 * the label and its consequence on the left, the control on the right, a
 * hairline between rows. Rows save as you change them and say so with a
 * faint "Saved" tick; the machine name is the one explicit Save.
 */

import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ACCENTS, DEFAULT_ACCENT, isAccent, type Accent } from "@/app/AccentSync";
import { useUiDensity, type UiDensity } from "@/app/density";
import { useThemePreference, type ThemePreference } from "@/app/theme";
import { keys, useAppSettings, useNode, useSaveSettings } from "@/hooks/useBackend";
import { useI18n, type Language } from "@/i18n";
import { api } from "@/lib/ipc";
import type { AppSettings } from "@/types/api";
import { isRemote } from "@/lib/connection";
import { McpSettings } from "./McpSettings";
import { RemoteSettings, UpnpTest } from "./RemoteSettings";
import {
  Button,
  ErrorNotice,
  Icon,
  Panel,
  Segmented,
  Select,
  Switch,
  TextInput,
} from "@/ui";

// -- the "Saved" tick ---------------------------------------------------------

type RowId =
  | "theme"
  | "accent"
  | "uiDensity"
  | "listView"
  | "language"
  | "storage"
  | "launchAtStartup"
  | "runInTray"
  | "allowUpnp"
  | "listenOnLan"
  | "java"
  | "name";

const SAVED_MS = 1200;

/** Which row last saved, cleared after 1.2s. Timer cleared on unmount. */
function useSavedTick() {
  const [saved, setSaved] = useState<RowId | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const flash = useCallback((row: RowId) => {
    window.clearTimeout(timer.current);
    setSaved(row);
    timer.current = window.setTimeout(() => setSaved(null), SAVED_MS);
  }, []);

  return [saved, flash] as const;
}

// -- screen -----------------------------------------------------------------

export function SettingsScreen() {
  const remote = isRemote();
  const { t, language, setLanguage } = useI18n();
  const client = useQueryClient();
  const { data: node } = useNode();
  const { data: settings } = useAppSettings();
  const save = useSaveSettings();
  const [theme, setTheme] = useThemePreference();
  const [uiDensity, setUiDensity] = useUiDensity();
  const [saved, flash] = useSavedTick();
  // The row that sent the last save, so a failure shows where it happened.
  const [lastRow, setLastRow] = useState<RowId | null>(null);

  const [nodeName, setNodeName] = useState<string | null>(null);

  const rename = useMutation({
    mutationFn: (name: string) => api.node.rename(name),
    onSuccess: (updated) => {
      client.setQueryData(keys.node, updated);
      setNodeName(null);
      flash("name");
    },
  });

  if (!settings) return null;

  const update = (row: RowId, patch: Partial<AppSettings>) => {
    setLastRow(row);
    save.mutate({ ...settings, ...patch }, { onSuccess: () => flash(row) });
  };

  const failure = (row: RowId) =>
    save.error && lastRow === row ? <ErrorNotice error={save.error} compact /> : null;

  const accent: Accent = isAccent(settings.accent) ? settings.accent : DEFAULT_ACCENT;
  const listView = settings.density === "list" ? "list" : "cards";
  const draftName = nodeName ?? node?.name ?? "";
  const nameChanged = !!nodeName && nodeName.trim() !== "" && nodeName !== node?.name;

  return (
    <div className="settings-page">
      <header className="settings-header">
        <h1 className="settings-title">{t("settings.title")}</h1>
        <p className="settings-subtitle">
          {t("settings.autosaveNote")}
        </p>
      </header>

      <Panel title={t("node.thisMachine")}>
        <SettingRow
          label={t("node.rename")}
          help={t("settings.machineNameHelp")}
          saved={saved === "name"}
          error={rename.error ? <ErrorNotice error={rename.error} compact /> : null}
        >
          <form
            className="flex items-center gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              if (nameChanged && !rename.isPending) rename.mutate(nodeName);
            }}
          >
            <TextInput
              className="settings-name-input"
              aria-label={t("node.rename")}
              value={draftName}
              onChange={(event) => setNodeName(event.target.value)}
            />
            <Button type="submit" disabled={!nameChanged || rename.isPending}>
              {t("actions.save")}
            </Button>
          </form>
        </SettingRow>
      </Panel>

      <Panel title={t("settings.appearance")}>
        <SettingRow
          label={t("settings.theme.label")}
          help={t("settings.theme.help")}
          saved={saved === "theme"}
        >
          <Segmented<ThemePreference>
            label={t("settings.theme.label")}
            value={theme}
            onChange={(next) => {
              setTheme(next);
              flash("theme");
            }}
            options={[
              { id: "system", label: t("settings.theme.system"), icon: "monitor" },
              { id: "dark", label: t("settings.theme.dark"), icon: "moon" },
              { id: "light", label: t("settings.theme.light"), icon: "sun" },
            ]}
          />
        </SettingRow>

        <SettingRow
          label={t("settings.accent")}
          help={t("settings.accentHelp")}
          saved={saved === "accent"}
          error={failure("accent")}
        >
          <AccentPicker
            label={t("settings.accent")}
            value={accent}
            nameOf={(id) => t(`settings.accentNames.${id}`)}
            onChange={(next) => update("accent", { accent: next })}
          />
        </SettingRow>

        <SettingRow
          label={t("settings.uiDensity.label")}
          help={t("settings.uiDensity.help")}
          saved={saved === "uiDensity"}
        >
          <Segmented<UiDensity>
            label={t("settings.uiDensity.label")}
            value={uiDensity}
            onChange={(next) => {
              setUiDensity(next);
              flash("uiDensity");
            }}
            options={[
              { id: "comfortable", label: t("settings.uiDensity.comfortable") },
              { id: "compact", label: t("settings.uiDensity.compact") },
            ]}
          />
        </SettingRow>

        <SettingRow
          label={t("settings.density")}
          help={t("settings.densityHelp")}
          saved={saved === "listView"}
          error={failure("listView")}
        >
          <Segmented<"cards" | "list">
            label={t("settings.density")}
            value={listView}
            onChange={(next) => update("listView", { density: next })}
            options={[
              { id: "cards", label: t("servers.densityCards") },
              { id: "list", label: t("servers.densityList") },
            ]}
          />
        </SettingRow>
      </Panel>

      <Panel title={t("settings.language")}>
        <SettingRow
          label={t("settings.languageLabel")}
          help={t("settings.languageHelp")}
          saved={saved === "language"}
          error={failure("language")}
        >
          <Select
            className="settings-select"
            aria-label={t("settings.languageLabel")}
            value={language}
            onChange={(event) => {
              const value = event.target.value as Language;
              setLanguage(value);
              update("language", { language: value });
            }}
          >
            <option value="en">English</option>
            <option value="es">Español</option>
          </Select>
        </SettingRow>
      </Panel>

      <Panel title={t("settings.storage")}>
        <SettingRow
          wide
          label={t("settings.instancesDir")}
          help={t("settings.instancesDirHelp")}
          saved={saved === "storage"}
          error={failure("storage")}
        >
          <div className="flex items-center gap-2">
            <TextInput
              className="mono selectable"
              aria-label={t("settings.instancesDir")}
              value={settings.instancesDir ?? ""}
              placeholder={t("settings.instancesDirDefault")}
              spellCheck={false}
              onChange={(event) => update("storage", { instancesDir: event.target.value })}
            />
            <Button
              icon="folder"
              className="flex-none"
              disabled={remote}
              title={remote ? t("remote.localOnly") : undefined}
              onClick={async () => {
                const { open } = await import("@tauri-apps/plugin-dialog");
                const picked = await open({ directory: true });
                if (typeof picked === "string") update("storage", { instancesDir: picked });
              }}
            >
              {t("settings.chooseFolder")}
            </Button>
          </div>
        </SettingRow>
      </Panel>

      <Panel title={t("settings.startupNetwork")}>
        <SwitchRow
          label={t("settings.launchAtStartup")}
          help={t("settings.launchAtStartupHelp")}
          checked={settings.launchAtStartup}
          onChange={(value) => update("launchAtStartup", { launchAtStartup: value })}
          saved={saved === "launchAtStartup"}
          error={failure("launchAtStartup")}
        />
        <SwitchRow
          label={t("settings.runInTray")}
          help={t("settings.runInTrayHelp")}
          checked={settings.runInTray}
          onChange={(value) => update("runInTray", { runInTray: value })}
          saved={saved === "runInTray"}
          error={failure("runInTray")}
        />
        <SwitchRow
          label={t("settings.allowUpnp")}
          help={t("settings.allowUpnpHelp")}
          checked={settings.allowUpnp}
          onChange={(value) => update("allowUpnp", { allowUpnp: value })}
          saved={saved === "allowUpnp"}
          error={failure("allowUpnp")}
        />
        {settings.allowUpnp ? <UpnpTest /> : null}
        {/* Opening the app to the network is the one setting here that
            changes who can reach this machine, so it says so in a warning
            colour, not just in the help text. */}
        <SwitchRow
          label={t("settings.listenOnLan")}
          help={t("settings.listenOnLanWarning")}
          warn
          checked={settings.listenOnLan}
          onChange={(value) => update("listenOnLan", { listenOnLan: value })}
          saved={saved === "listenOnLan"}
          error={failure("listenOnLan")}
        />
      </Panel>

      {remote ? null : <RemoteSettings />}

      {remote ? null : <McpSettings />}

      <JavaSettings
        paths={settings.javaPaths}
        saved={saved === "java"}
        error={failure("java")}
        emptyText={t("settings.javaNone")}
        onChange={(javaPaths) => update("java", { javaPaths })}
      />
    </div>
  );
}

// -- rows -------------------------------------------------------------------

/** Label and consequence on the left, the control on the right. `wide`
 *  puts the control under the text, for paths that need the full width. */
function SettingRow({
  label,
  help,
  saved,
  error,
  wide = false,
  children,
}: {
  label: string;
  help?: string;
  saved: boolean;
  error?: ReactNode;
  wide?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="settings-row" data-wide={wide ? "" : undefined}>
      <div className="settings-row-text">
        <span className="settings-row-label">
          {label}
          <SavedTick visible={saved} />
        </span>
        {help ? <span className="settings-row-help">{help}</span> : null}
      </div>
      <div className="settings-row-control">
        {children}
      </div>
      {error ? <div className="settings-row-error">{error}</div> : null}
    </div>
  );
}

/** A whole-row switch: clicking the words flips it too. */
function SwitchRow({
  label,
  help,
  warn = false,
  checked,
  onChange,
  saved,
  error,
}: {
  label: string;
  help?: string;
  warn?: boolean;
  checked: boolean;
  onChange: (value: boolean) => void;
  saved: boolean;
  error?: ReactNode;
}) {
  const helpId = useId();
  return (
    <div className="settings-row" data-switch="">
      <label className="settings-row-hit">
        <span className="settings-row-text">
          <span className="settings-row-label">
            {label}
            <SavedTick visible={saved} />
          </span>
          {help ? (
            <span id={helpId} className="settings-row-help" data-warn={warn ? "" : undefined}>
              {warn ? <Icon name="warning" size={12} /> : null}
              <span>{help}</span>
            </span>
          ) : null}
        </span>
        <span className="settings-row-control">
          <Switch
            checked={checked}
            onChange={onChange}
            label={label}
            describedBy={help ? helpId : undefined}
          />
        </span>
      </label>
      {error ? <div className="settings-row-error">{error}</div> : null}
    </div>
  );
}

/**
 * The faint confirmation beside a label. The visible tick stays in the
 * layout and only fades, so it can fade out as well as in; the words are
 * announced once through a polite status that holds text only while shown.
 */
function SavedTick({ visible }: { visible: boolean }) {
  const { t } = useI18n();
  return (
    <>
      <span className="settings-saved" data-visible={visible ? "" : undefined} aria-hidden="true">
        <Icon name="check" size={12} />
        {t("common.saved")}
      </span>
      <span className="sr-only" role="status">
        {visible ? t("common.saved") : ""}
      </span>
    </>
  );
}

// -- java -------------------------------------------------------------------

function JavaSettings({
  paths,
  saved,
  error,
  emptyText,
  onChange,
}: {
  paths: Record<string, string>;
  saved: boolean;
  error?: ReactNode;
  emptyText: string;
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

  const entries = Object.entries(paths);

  return (
    <Panel
      title={t("settings.javaPaths")}
      actions={
        <>
          <SavedTick visible={saved} />
          <Button
            size="sm"
            icon="plus"
            disabled={probe.isPending || isRemote()}
            title={isRemote() ? t("remote.localOnly") : undefined}
            onClick={async () => {
              const { open } = await import("@tauri-apps/plugin-dialog");
              const picked = await open({
                multiple: false,
                filters: [{ name: "java", extensions: ["exe", ""] }],
              });
              if (typeof picked === "string") probe.mutate(picked);
            }}
          >
            {t("settings.addJava")}
          </Button>
        </>
      }
    >
      <p className="settings-java-help">{t("settings.javaPathsHelp")}</p>

      {entries.length ? (
        <ul className="settings-java-list">
          {entries.map(([major, path]) => (
            <li key={major} className="settings-java-row">
              <span className="tag mono flex-none tabular-nums">Java {major}</span>
              <span
                className="mono selectable min-w-0 flex-1 truncate text-[length:var(--text-xs)] text-[color:var(--text-muted)]"
                title={path}
              >
                {path}
              </span>
              {probed[path] ? (
                <span className="settings-java-found">
                  <Icon name="check" size={12} />
                  <span className="mono">{probed[path]}</span>
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
      ) : (
        <p className="settings-java-empty">{emptyText}</p>
      )}

      {probe.error || error ? (
        <div className="settings-java-error">
          {probe.error ? <ErrorNotice error={probe.error} /> : error}
        </div>
      ) : null}
    </Panel>
  );
}

// -- accent -----------------------------------------------------------------

/**
 * Six accents, each shown as a plain circle of its own colour. Each swatch
 * carries its own data-accent, and tokens.css redeclares the derived set on
 * every [data-accent] element, so each swatch renders ITS colour (in either theme),
 * not the page's. A radiogroup, so arrow keys move between them.
 */
function AccentPicker({
  label,
  value,
  nameOf,
  onChange,
}: {
  label: string;
  value: Accent;
  nameOf: (accent: Accent) => string;
  onChange: (accent: Accent) => void;
}) {
  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    const step =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? 1
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? -1
          : 0;
    if (!step) return;
    event.preventDefault();
    const index = ACCENTS.indexOf(value);
    const next = ACCENTS[(index + step + ACCENTS.length) % ACCENTS.length] ?? value;
    onChange(next);
    event.currentTarget.querySelector<HTMLButtonElement>(`[data-accent="${next}"]`)?.focus();
  };

  return (
    <div role="radiogroup" aria-label={label} className="settings-swatches" onKeyDown={move}>
      {ACCENTS.map((accent) => {
        const selected = accent === value;
        return (
          <button
            key={accent}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-label={nameOf(accent)}
            title={nameOf(accent)}
            tabIndex={selected ? 0 : -1}
            data-accent={accent}
            className="settings-swatch"
            onClick={() => onChange(accent)}
          >
            {selected ? <Icon name="check" size={14} /> : null}
          </button>
        );
      })}
    </div>
  );
}
