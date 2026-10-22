/**
 * An instance's settings tab: who it is (name, picture), what it runs
 * (Minecraft version, loader), memory, tools, and removing it. Each block
 * saves on its own, so one change is never lost to another.
 */

import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, Field, Icon, LoaderIcon, Panel, Select, TextInput } from "@/ui";
import { RamSlider } from "@/ui/RamSlider";
import { LOADERS, loaderInfo } from "./loaderOptions";
import type { LauncherProfile, Loader } from "./types";

/** The card's colour: the loader's, so each kind of instance reads at a glance. */
const LOADER_COLOUR: Record<Loader, string> = {
  vanilla: "#5cb88e",
  fabric: "#d9b67e",
  forge: "#83a7dd",
  neoforge: "#efa36b",
  quilt: "#ba9be5",
};

/** The instance's picture, or its loader's glyph on the loader's colour. */
export function InstanceTile({ profile, icon, size = 40 }: { profile: LauncherProfile; icon?: string | null; size?: number }) {
  const picture = icon === undefined ? profile.icon : icon;
  return (
    <span
      className="launcher-tile"
      style={{ ["--tile" as string]: LOADER_COLOUR[profile.loader], width: size, height: size }}
      aria-hidden="true"
    >
      {picture ? <img src={picture} alt="" draggable={false} /> : <LoaderIcon glyph={loaderInfo(profile.loader).glyph} size={Math.round(size / 2)} />}
    </span>
  );
}

/** Any picked image as a 128px square PNG, so the stored icon stays small. */
function squareIcon(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const image = new Image();
    image.onload = () => {
      const side = Math.min(image.width, image.height);
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = 128;
      const context = canvas.getContext("2d");
      if (!context) return reject(new Error("canvas"));
      // Pixel art stays sharp when it is scaled up.
      context.imageSmoothingEnabled = side >= 128;
      context.drawImage(image, (image.width - side) / 2, (image.height - side) / 2, side, side, 0, 0, 128, 128);
      URL.revokeObjectURL(url);
      resolve(canvas.toDataURL("image/png"));
    };
    image.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new Error("image"));
    };
    image.src = url;
  });
}

export function InstanceSettingsPanel({ profile, busy, onRemoved }: { profile: LauncherProfile; busy: boolean; onRemoved: () => void }) {
  return (
    <div className="launcher-settings">
      <DetailsBlock key={`d-${profile.id}`} profile={profile} />
      <VersionBlock key={`v-${profile.id}-${profile.game_version}-${profile.loader}`} profile={profile} busy={busy} />
      <MemoryBlock key={`m-${profile.id}`} profile={profile} busy={busy} />
      <ToolsBlock profile={profile} busy={busy} />
      <RemoveBlock profile={profile} busy={busy} onRemoved={onRemoved} />
    </div>
  );
}

function useRefreshProfiles() {
  const client = useQueryClient();
  return () => client.invalidateQueries({ queryKey: ["launcher", "profiles"] });
}

function DetailsBlock({ profile }: { profile: LauncherProfile }) {
  const { t } = useI18n();
  const refresh = useRefreshProfiles();
  const [name, setName] = useState(profile.name);
  const [icon, setIcon] = useState<string | null>(profile.icon ?? null);
  const [pickError, setPickError] = useState(false);
  const picker = useRef<HTMLInputElement>(null);
  const save = useMutation({ mutationFn: () => launcherApi.updateDetails(profile.id, name.trim(), icon), onSuccess: refresh });
  const dirty = name.trim() !== profile.name || icon !== (profile.icon ?? null);
  return (
    <Panel title={t("launcher.general")} bodyClassName="launcher-panel-body">
      <div className="launcher-settings-icon">
        <InstanceTile profile={profile} icon={icon} size={72} />
        <input
          ref={picker}
          type="file"
          accept="image/png,image/jpeg,image/webp,image/gif"
          hidden
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (!file) return;
            setPickError(false);
            squareIcon(file).then(setIcon, () => setPickError(true));
          }}
        />
        <div className="launcher-actions">
          <Button size="sm" icon="image" onClick={() => picker.current?.click()}>{t("launcher.chooseIcon")}</Button>
          {icon ? <Button size="sm" variant="quiet" onClick={() => setIcon(null)}>{t("launcher.resetIcon")}</Button> : null}
        </div>
      </div>
      {pickError ? <p role="alert" className="launcher-muted">{t("launcher.iconFailed")}</p> : null}
      <Field label={t("launcher.instanceName")}>
        <TextInput value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
      </Field>
      <div className="launcher-actions">
        <Button variant="primary" icon="check" disabled={!dirty || !name.trim() || save.isPending} onClick={() => save.mutate()}>{t("launcher.save")}</Button>
        {save.isSuccess && !dirty ? <span className="launcher-muted"><Icon name="check" size={12} /> {t("common.saved")}</span> : null}
      </div>
      {save.error ? <ErrorNotice error={toAppError(save.error)} compact /> : null}
    </Panel>
  );
}

/** Move to another Minecraft version or loader without a new instance. */
function VersionBlock({ profile, busy }: { profile: LauncherProfile; busy: boolean }) {
  const { t } = useI18n();
  const refresh = useRefreshProfiles();
  const caps = useQuery({ queryKey: ["launcher", "capabilities"], queryFn: launcherApi.capabilities });
  const versions = useQuery({ queryKey: ["launcher", "versions"], queryFn: launcherApi.versions, staleTime: 300_000 });
  const [version, setVersion] = useState(profile.game_version);
  const [loader, setLoader] = useState<Loader>(profile.loader);
  const [loaderVersion, setLoaderVersion] = useState(profile.loader_version ?? "");
  const available = versions.data?.filter((v) => (caps.data?.can_launch ? v.client_supported : v.type === "release")) ?? [];
  const modded = loader !== "vanilla" && loader !== "quilt";
  const builds = useQuery({
    queryKey: ["launcher", "loaderVersions", loader, version],
    queryFn: () => launcherApi.loaderVersions(loader, version),
    enabled: modded,
    staleTime: 300_000,
    retry: false,
  });
  const build = modded ? (builds.data?.includes(loaderVersion) ? loaderVersion : (builds.data?.[0] ?? "")) : "";
  const save = useMutation({ mutationFn: () => launcherApi.changeVersion(profile.id, version, loader, build || null), onSuccess: refresh });
  const dirty = version !== profile.game_version || loader !== profile.loader || (modded && build !== (profile.loader_version ?? ""));
  const ready = !modded || (!!build && !builds.isFetching);
  return (
    <Panel title={t("launcher.gameVersion")} bodyClassName="launcher-panel-body">
      <div className="launcher-version-grid">
        <label className="launcher-field">
          <span className="label">{t("launcher.version")}</span>
          <Select value={version} disabled={busy || !available.length} onChange={(e) => setVersion(e.target.value)}>
            {available.some((v) => v.id === version) ? null : <option value={version}>{version}</option>}
            {available.map((v) => <option key={v.id} value={v.id}>{v.id}</option>)}
          </Select>
        </label>
        <label className="launcher-field">
          <span className="label">{t("launcher.loader")}</span>
          <Select value={loader} disabled={busy} onChange={(e) => { setLoader(e.target.value as Loader); setLoaderVersion(""); }}>
            {LOADERS.filter((item) => item.id !== "quilt" || profile.loader === "quilt").map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
          </Select>
        </label>
        {modded ? (
          <label className="launcher-field">
            <span className="label">{t("launcher.loaderBuild")}</span>
            <Select value={build} disabled={busy || !builds.data?.length} onChange={(e) => setLoaderVersion(e.target.value)}>
              {(builds.data ?? []).slice(0, 40).map((v) => <option key={v} value={v}>{v}</option>)}
            </Select>
          </label>
        ) : null}
      </div>
      {modded && builds.isFetching ? <p role="status" className="launcher-muted">{t("launcher.checkingLoader")}</p> : null}
      {modded && !builds.isFetching && builds.data?.length === 0 ? <p className="launcher-notice">{t("launcher.noCompatibleLoader")}</p> : null}
      {dirty ? <p className="launcher-notice"><Icon name="warning" size={14} />{t("launcher.versionChangeNote")}</p> : null}
      <div className="launcher-actions">
        <Button variant="primary" icon="check" disabled={busy || !dirty || !ready || save.isPending} onClick={() => save.mutate()}>
          {save.isPending ? t("launcher.saving") : t("launcher.applyVersion")}
        </Button>
      </div>
      {busy ? <p className="launcher-muted">{t("launcher.closeGameFirst")}</p> : null}
      {save.error || versions.error || builds.error ? <ErrorNotice error={toAppError(save.error || versions.error || builds.error)} compact /> : null}
    </Panel>
  );
}

function MemoryBlock({ profile, busy }: { profile: LauncherProfile; busy: boolean }) {
  const { t } = useI18n();
  const refresh = useRefreshProfiles();
  const [memory, setMemory] = useState(profile.memory_mb);
  const save = useMutation({ mutationFn: (mb: number) => launcherApi.setMemory(profile.id, mb), onSuccess: refresh });
  return (
    <Panel title={t("launcher.memory")} bodyClassName="launcher-panel-body">
      <span id={`memory-${profile.id}`} hidden>{t("launcher.memory")}</span>
      <RamSlider
        value={memory}
        onChange={setMemory}
        onCommit={(mb) => { if (mb !== profile.memory_mb) save.mutate(mb); }}
        projectType={profile.loader === "vanilla" ? null : "mod"}
        labelledBy={`memory-${profile.id}`}
        disabled={busy || save.isPending}
      />
      {busy ? <p className="launcher-muted">{t("launcher.closeGameFirst")}</p> : null}
      {save.error ? <ErrorNotice error={toAppError(save.error)} compact /> : null}
    </Panel>
  );
}

function ToolsBlock({ profile, busy }: { profile: LauncherProfile; busy: boolean }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const caps = useQuery({ queryKey: ["launcher", "capabilities"], queryFn: launcherApi.capabilities });
  const accounts = useQuery({ queryKey: ["launcher", "accounts"], queryFn: launcherApi.accounts });
  const account = accounts.data?.[0]?.id;
  const repair = useMutation({
    mutationFn: () => launcherApi.start(profile.id, "install"),
    onSuccess: () => client.invalidateQueries({ queryKey: ["launcher", "status"] }),
  });
  const shortcut = useMutation({ mutationFn: () => launcherApi.pinShortcut(profile.id, account ? "account" : "demo", account) });
  return (
    <Panel title={t("launcher.tools")} bodyClassName="launcher-panel-body">
      <div className="launcher-tool">
        <div><strong>{t("launcher.installRepair")}</strong><p className="launcher-muted">{t("launcher.repairHelp")}</p></div>
        <Button icon="download" disabled={busy || repair.isPending} onClick={() => repair.mutate()}>{t("launcher.installRepair")}</Button>
      </div>
      {repair.isSuccess ? <p role="status" className="launcher-notice">{t("launcher.repairStarted")}</p> : null}
      {caps.data?.platform === "android" ? (
        <div className="launcher-tool">
          <div><strong>{t("launcher.pinShortcut")}</strong><p className="launcher-muted">{t("launcher.shortcutHelp")}</p></div>
          <Button icon="plus" disabled={shortcut.isPending} onClick={() => shortcut.mutate()}>{t("launcher.pinShortcut")}</Button>
        </div>
      ) : null}
      {shortcut.isSuccess ? <p role="status" className="launcher-notice">{t("launcher.shortcutRequested")}</p> : null}
      {repair.error || shortcut.error ? <ErrorNotice error={toAppError(repair.error || shortcut.error)} compact /> : null}
    </Panel>
  );
}

function RemoveBlock({ profile, busy, onRemoved }: { profile: LauncherProfile; busy: boolean; onRemoved: () => void }) {
  const { t } = useI18n();
  const refresh = useRefreshProfiles();
  const [confirming, setConfirming] = useState(false);
  const remove = useMutation({
    mutationFn: () => launcherApi.remove(profile.id),
    onSuccess: async () => {
      await refresh();
      onRemoved();
    },
  });
  return (
    <Panel title={t("launcher.removeInstance")} bodyClassName="launcher-panel-body">
      <p className="launcher-muted">{t("launcher.keepGameFiles")}</p>
      <div><Button variant="danger" icon="trash" disabled={busy} onClick={() => setConfirming(true)}>{t("launcher.removeInstance")}</Button></div>
      {busy ? <p className="launcher-muted">{t("launcher.closeGameFirst")}</p> : null}
      {confirming ? (
        <Dialog
          open
          onClose={() => setConfirming(false)}
          dismissable={!remove.isPending}
          title={t("launcher.removeInstance")}
          footer={<>
            <Button variant="quiet" disabled={remove.isPending} onClick={() => setConfirming(false)}>{t("launcher.cancel")}</Button>
            <Button variant="danger" disabled={remove.isPending || busy} onClick={() => remove.mutate()}>{t("launcher.removeInstance")}</Button>
          </>}
        >
          <div className="flex flex-col gap-3">
            <p>{t("launcher.confirmRemoveInstance", { name: profile.name })}</p>
            {remove.error ? <ErrorNotice error={toAppError(remove.error)} /> : null}
          </div>
        </Dialog>
      ) : null}
    </Panel>
  );
}
