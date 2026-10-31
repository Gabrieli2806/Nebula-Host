/**
 * Copy mods between a server and a launcher instance on this PC: from a server
 * to play on it, or from an instance to host what was tested. Both sides need
 * the same game version and loader, so only those are offered. The backend
 * leaves behind what cannot run on the other side and says so.
 */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { keys, useInstances } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { isRemote } from "@/lib/connection";
import { launcherApi, type ModShareReport } from "@/lib/ipc";
import { useLauncherEnabled } from "@/lib/launcherPref";
import { IS_WEB } from "@/lib/web";
import type { AddonContext, InstanceView } from "@/types/api";
import { Button, Dialog, ErrorNotice, Select } from "@/ui";

import { launcherKeys, useLauncherProfiles } from "./hooks";
import type { LauncherProfile, Loader } from "./types";

const NEW_INSTANCE = "new";

interface Target {
  id: string;
  label: string;
  disabled?: boolean;
}

function ShareDialog({
  title,
  label,
  targets,
  initial,
  onClose,
  run,
}: {
  title: string;
  label: string;
  targets: Target[];
  initial: string;
  onClose: () => void;
  run: (target: string) => Promise<ModShareReport>;
}) {
  const { t } = useI18n();
  const [target, setTarget] = useState(initial);
  const share = useMutation({ mutationFn: () => run(target) });
  const report = share.data;
  const wrongSide = report?.skipped.filter((item) => item.reason === "wrong_side").length ?? 0;
  const disabled = report?.skipped.filter((item) => item.reason === "disabled").length ?? 0;
  const failed = report?.skipped.filter((item) => item.reason === "failed") ?? [];

  return (
    <Dialog
      open
      onClose={() => {
        if (!share.isPending) onClose();
      }}
      title={title}
      width={460}
      footer={
        report ? (
          <Button variant="primary" onClick={onClose}>{t("dialog.close")}</Button>
        ) : (
          <>
            <Button variant="quiet" disabled={share.isPending} onClick={onClose}>{t("actions.cancel")}</Button>
            <Button variant="primary" disabled={!target || share.isPending} onClick={() => share.mutate()}>
              {share.isPending ? t("addons.shareCopying") : t("addons.shareCopy")}
            </Button>
          </>
        )
      }
    >
      {report ? (
        <div className="flex flex-col gap-2" role="status">
          <p>{t("addons.shareCopied", { count: report.copied.length })}</p>
          {report.already.length ? <p className="launcher-muted">{t("addons.shareAlready", { count: report.already.length })}</p> : null}
          {wrongSide ? <p className="launcher-muted">{t("addons.shareWrongSide", { count: wrongSide })}</p> : null}
          {disabled ? <p className="launcher-muted">{t("addons.shareDisabled", { count: disabled })}</p> : null}
          {failed.length ? <p>{t("addons.shareFailed", { files: failed.map((item) => item.file).join(", ") })}</p> : null}
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          <Select aria-label={label} value={target} disabled={share.isPending} onChange={(event) => setTarget(event.target.value)}>
            {targets.map((item) => <option key={item.id} value={item.id} disabled={item.disabled}>{item.label}</option>)}
          </Select>
          {share.error ? <ErrorNotice error={share.error} compact /> : null}
        </div>
      )}
    </Dialog>
  );
}

/** On a server's Mods tab: copy them to an instance, new or matching. */
export function ShareToLauncher({ instance, context }: { instance: InstanceView; context: AddonContext }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const launcherOn = useLauncherEnabled();
  const [open, setOpen] = useState(false);
  const local = !isRemote() && !IS_WEB;
  const profiles = useLauncherProfiles();
  const loader = context.loaders[0] as Loader | undefined;
  if (!local || !launcherOn || context.project_type !== "mod" || !loader) return null;

  const matching = (profiles.data ?? []).filter((profile) => profile.game_version === context.game_version && profile.loader === loader);
  const targets: Target[] = [
    { id: NEW_INSTANCE, label: t("addons.shareNewInstance", { name: instance.name }) },
    ...matching.map((profile) => ({ id: profile.id, label: profile.name })),
  ];
  const run = async (target: string) => {
    const id = target === NEW_INSTANCE
      ? (await launcherApi.create({ name: instance.name, game_version: context.game_version, loader, memory_mb: 4096 })).id
      : target;
    const report = await launcherApi.modsShare(instance.id, id, "to_instance");
    await client.invalidateQueries({ queryKey: launcherKeys.profiles });
    await client.invalidateQueries({ queryKey: ["launcher", "content", id] });
    return report;
  };

  return (
    <>
      <Button size="sm" icon="upload" onClick={() => setOpen(true)}>{t("addons.shareToLauncher")}</Button>
      {open ? <ShareDialog title={t("addons.shareToLauncher")} label={t("addons.shareInstance")} targets={targets} initial={matching[0]?.id ?? NEW_INSTANCE} onClose={() => setOpen(false)} run={run} /> : null}
    </>
  );
}

/** On an instance's mods: copy them to a stopped server of the same kind. */
export function ShareToServer({ profile }: { profile: LauncherProfile }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const [open, setOpen] = useState(false);
  const { data: instances } = useInstances();
  const servers = (instances ?? []).filter((server) => server.build.version === profile.game_version && server.build.distribution === profile.loader);
  if (isRemote() || IS_WEB || servers.length === 0) return null;

  const stopped = (server: InstanceView) => server.status === "offline" || server.status === "crashed";
  const targets: Target[] = servers.map((server) => ({
    id: server.id,
    label: stopped(server) ? server.name : `${server.name} · ${t("addons.shareStopFirst")}`,
    disabled: !stopped(server),
  }));
  const run = async (target: string) => {
    const report = await launcherApi.modsShare(target, profile.id, "to_server");
    await client.invalidateQueries({ queryKey: keys.addonsList(target) });
    return report;
  };

  return (
    <>
      <Button size="sm" icon="upload" onClick={() => setOpen(true)}>{t("addons.shareToServer")}</Button>
      {open ? <ShareDialog title={t("addons.shareToServer")} label={t("addons.shareServer")} targets={targets} initial={servers.find(stopped)?.id ?? ""} onClose={() => setOpen(false)} run={run} /> : null}
    </>
  );
}
