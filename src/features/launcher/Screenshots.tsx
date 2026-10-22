/**
 * Every screenshot from every instance, newest first, with which instance it
 * came from. Click to view large; right-click to copy, open the folder or
 * delete.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, Dialog, EmptyState, ErrorNotice, Icon, Panel, Select } from "@/ui";
import { useContextMenu, type MenuItem } from "@/ui/ContextMenu";
import { InstanceTile } from "./InstanceSettings";
import type { LauncherProfile } from "./types";

type Shot = { instance_id: string; file: string; size: number; taken: number };

function useShotData(shot: Shot) {
  return useQuery({
    queryKey: ["launcher", "screenshot", shot.instance_id, shot.file],
    queryFn: () => launcherApi.screenshotData(shot.instance_id, shot.file),
    staleTime: Infinity,
    gcTime: 5 * 60_000,
  });
}

async function copyImage(dataUrl: string) {
  const blob = await (await fetch(dataUrl)).blob();
  await navigator.clipboard.write([new ClipboardItem({ [blob.type || "image/png"]: blob })]);
}

export function Screenshots() {
  const { t, language } = useI18n();
  const client = useQueryClient();
  const profiles = useQuery({ queryKey: ["launcher", "profiles"], queryFn: launcherApi.profiles });
  const shots = useQuery({ queryKey: ["launcher", "screenshots"], queryFn: launcherApi.screenshots, refetchInterval: 15_000 });
  const [filter, setFilter] = useState("all");
  const [viewing, setViewing] = useState<Shot | null>(null);
  const [copied, setCopied] = useState(false);
  const { open, menu } = useContextMenu();
  const remove = useMutation({
    mutationFn: (shot: Shot) => launcherApi.screenshotDelete(shot.instance_id, shot.file),
    onSuccess: (_, shot) => {
      client.setQueryData<Shot[]>(["launcher", "screenshots"], (old) => old?.filter((s) => !(s.instance_id === shot.instance_id && s.file === shot.file)));
      if (viewing?.file === shot.file) setViewing(null);
    },
  });
  const byId = new Map((profiles.data ?? []).map((p) => [p.id, p]));
  const visible = (shots.data ?? []).filter((s) => filter === "all" || s.instance_id === filter);
  const date = (secs: number) => new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(new Date(secs * 1000));

  const copy = async (shot: Shot) => {
    const data = await client.fetchQuery({
      queryKey: ["launcher", "screenshot", shot.instance_id, shot.file],
      queryFn: () => launcherApi.screenshotData(shot.instance_id, shot.file),
      staleTime: Infinity,
    });
    await copyImage(data);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };
  const items = (shot: Shot): MenuItem[] => [
    { label: t("launcher.viewShot"), icon: "image", onSelect: () => setViewing(shot) },
    { label: t("launcher.copyImage"), icon: "copy", onSelect: () => void copy(shot) },
    { label: t("launcher.openShotsFolder"), icon: "folder", onSelect: () => void launcherApi.openFolder(shot.instance_id, true) },
    { kind: "separator" },
    { label: t("launcher.deleteShot"), icon: "trash", danger: true, onSelect: () => remove.mutate(shot) },
  ];

  return (
    <div className="flex flex-col gap-3">
      <div className="launcher-content-bar">
        <Select className="launcher-filter" aria-label={t("launcher.filterInstance")} value={filter} onChange={(e) => setFilter(e.target.value)}>
          <option value="all">{t("launcher.allInstances")}</option>
          {(profiles.data ?? []).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </Select>
        <span className="launcher-muted">{copied ? t("common.copied") : t("launcher.shotsCount", { count: visible.length })}</span>
      </div>
      {shots.error || remove.error ? <ErrorNotice error={toAppError(shots.error || remove.error)} compact /> : null}
      {shots.data && visible.length === 0 ? (
        <Panel><EmptyState icon="image" title={t("launcher.noShots")} hint={t("launcher.noShotsHint")} /></Panel>
      ) : null}
      <ul className="launcher-shots">
        {visible.map((shot) => (
          <ShotTile
            key={`${shot.instance_id}/${shot.file}`}
            shot={shot}
            profile={byId.get(shot.instance_id)}
            when={date(shot.taken)}
            onOpen={() => setViewing(shot)}
            onMenu={(event) => open(event, items(shot))}
          />
        ))}
      </ul>
      {menu}
      {viewing ? (
        <ShotViewer
          shot={viewing}
          profile={byId.get(viewing.instance_id)}
          when={date(viewing.taken)}
          onClose={() => setViewing(null)}
          onCopy={() => void copy(viewing)}
          onDelete={() => remove.mutate(viewing)}
          copied={copied}
        />
      ) : null}
    </div>
  );
}

function ShotTile({ shot, profile, when, onOpen, onMenu }: {
  shot: Shot;
  profile?: LauncherProfile;
  when: string;
  onOpen: () => void;
  onMenu: (event: React.MouseEvent) => void;
}) {
  const data = useShotData(shot);
  return (
    <li className="launcher-shot" onContextMenu={onMenu}>
      <button type="button" className="launcher-shot-image" onClick={onOpen} aria-label={shot.file}>
        {data.data ? <img src={data.data} alt="" loading="lazy" draggable={false} /> : <span className="skeleton" />}
      </button>
      <div className="launcher-shot-meta">
        {profile ? <InstanceTile profile={profile} size={20} /> : null}
        <span className="launcher-shot-instance">{profile?.name ?? "—"}</span>
        <span className="launcher-muted">{when}</span>
      </div>
    </li>
  );
}

function ShotViewer({ shot, profile, when, onClose, onCopy, onDelete, copied }: {
  shot: Shot;
  profile?: LauncherProfile;
  when: string;
  onClose: () => void;
  onCopy: () => void;
  onDelete: () => void;
  copied: boolean;
}) {
  const { t } = useI18n();
  const data = useShotData(shot);
  return (
    <Dialog
      open
      onClose={onClose}
      title={profile ? `${profile.name} · ${when}` : when}
      width={1100}
      footer={<>
        <Button variant="quiet" icon="trash" onClick={onDelete}>{t("launcher.deleteShot")}</Button>
        <Button icon="folder" onClick={() => void launcherApi.openFolder(shot.instance_id, true)}>{t("launcher.openShotsFolder")}</Button>
        <Button variant="primary" icon={copied ? "check" : "copy"} onClick={onCopy}>{copied ? t("common.copied") : t("launcher.copyImage")}</Button>
      </>}
    >
      {data.data ? <img className="launcher-shot-full" src={data.data} alt={shot.file} /> : <p role="status" className="launcher-muted"><Icon name="image" size={14} /> {t("launcher.loading")}</p>}
    </Dialog>
  );
}
