/**
 * An instance's settings in one place: name, picture, memory, and removing
 * it. Opened from the gear on the instance, so the main view stays about
 * playing.
 */

import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, Field, LoaderIcon, Tabs, TextInput } from "@/ui";
import { RamSlider } from "@/ui/RamSlider";
import { loaderInfo } from "./loaderOptions";
import type { LauncherProfile, Loader } from "./types";

/** The card's colour: the loader's, so each kind of instance reads at a glance. */
export const LOADER_COLOUR: Record<Loader, string> = {
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

type Section = "general" | "memory" | "tools" | "danger";

export function InstanceSettings({
  profile,
  busy,
  onClose,
  onRemove,
}: {
  profile: LauncherProfile;
  /** Running or installing: memory and removal wait. */
  busy: boolean;
  onClose: () => void;
  onRemove: () => void;
}) {
  const { t } = useI18n();
  const client = useQueryClient();
  const [section, setSection] = useState<Section>("general");
  const [name, setName] = useState(profile.name);
  const [icon, setIcon] = useState<string | null>(profile.icon ?? null);
  const [memory, setMemory] = useState(profile.memory_mb);
  const [pickError, setPickError] = useState(false);
  const picker = useRef<HTMLInputElement>(null);

  const refresh = () => client.invalidateQueries({ queryKey: ["launcher", "profiles"] });
  const caps = useQuery({ queryKey: ["launcher", "capabilities"], queryFn: launcherApi.capabilities });
  const accounts = useQuery({ queryKey: ["launcher", "accounts"], queryFn: launcherApi.accounts });
  const account = accounts.data?.[0]?.id;
  const repair = useMutation({
    mutationFn: () => launcherApi.start(profile.id, "install"),
    onSuccess: () => client.invalidateQueries({ queryKey: ["launcher", "status"] }),
  });
  const shortcut = useMutation({ mutationFn: () => launcherApi.pinShortcut(profile.id, account ? "account" : "demo", account) });
  const details = useMutation({ mutationFn: () => launcherApi.updateDetails(profile.id, name.trim(), icon), onSuccess: refresh });
  const ram = useMutation({ mutationFn: () => launcherApi.setMemory(profile.id, memory), onSuccess: refresh });
  const save = useMutation({
    mutationFn: async () => {
      if (name.trim() !== profile.name || icon !== (profile.icon ?? null)) await details.mutateAsync();
      if (memory !== profile.memory_mb) await ram.mutateAsync();
    },
    onSuccess: onClose,
  });
  const dirty = name.trim() !== profile.name || icon !== (profile.icon ?? null) || memory !== profile.memory_mb;

  return (
    <Dialog
      open
      onClose={onClose}
      dismissable={!save.isPending}
      title={t("launcher.instanceSettings")}
      width={600}
      header={
        <Tabs<Section>
          label={t("launcher.instanceSettings")}
          active={section}
          onChange={setSection}
          tabs={[
            { id: "general", label: t("launcher.general") },
            { id: "memory", label: t("launcher.memory") },
            { id: "tools", label: t("launcher.tools") },
            { id: "danger", label: t("launcher.removeInstance") },
          ]}
        />
      }
      footer={
        <>
          <Button variant="quiet" disabled={save.isPending} onClick={onClose}>{t("launcher.cancel")}</Button>
          <Button variant="primary" icon="check" disabled={!dirty || !name.trim() || save.isPending} onClick={() => save.mutate()}>
            {t("launcher.save")}
          </Button>
        </>
      }
    >
      {section === "general" ? (
        <div className="launcher-settings-general">
          <div className="launcher-settings-icon">
            <InstanceTile profile={profile} icon={icon} size={88} />
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
            {pickError ? <p role="alert" className="launcher-muted">{t("launcher.iconFailed")}</p> : null}
          </div>
          <Field label={t("launcher.instanceName")}>
            <TextInput value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
          </Field>
          <dl className="launcher-facts">
            <div><dt>{t("launcher.version")}</dt><dd className="readout">{profile.game_version}</dd></div>
            <div><dt>{t("launcher.loader")}</dt><dd>{loaderInfo(profile.loader).name}{profile.loader_version ? <span className="launcher-muted"> {profile.loader_version}</span> : null}</dd></div>
          </dl>
        </div>
      ) : null}

      {section === "memory" ? (
        <div className="launcher-memory">
          <span id={`memory-${profile.id}`} className="label">{t("launcher.memory")}</span>
          <RamSlider
            value={memory}
            onChange={setMemory}
            projectType={profile.loader === "vanilla" ? null : "mod"}
            labelledBy={`memory-${profile.id}`}
            disabled={busy}
          />
          {busy ? <p className="launcher-muted">{t("launcher.closeGameFirst")}</p> : null}
        </div>
      ) : null}

      {section === "tools" ? (
        <div className="launcher-settings-tools">
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
        </div>
      ) : null}

      {section === "danger" ? (
        <div className="flex flex-col gap-3">
          <p>{t("launcher.confirmRemoveInstance", { name: profile.name })}</p>
          <p className="launcher-muted">{t("launcher.keepGameFiles")}</p>
          <div>
            <Button variant="danger" icon="trash" disabled={busy} onClick={onRemove}>{t("launcher.removeInstance")}</Button>
          </div>
          {busy ? <p className="launcher-muted">{t("launcher.closeGameFirst")}</p> : null}
        </div>
      ) : null}

      {save.error ? <ErrorNotice error={toAppError(save.error)} compact /> : null}
    </Dialog>
  );
}
