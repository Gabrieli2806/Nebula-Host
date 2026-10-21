import { useId, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useI18n } from "@/i18n";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, LoaderIcon } from "@/ui";
import type { Loader, LauncherProfile } from "./types";
import { LOADERS, loaderInfo } from "./loaderOptions";

export function CreateInstanceDialog({ onClose, onCreated }: { onClose: () => void; onCreated: (profile: LauncherProfile) => void }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const formId = useId();
  const [name, setName] = useState("");
  const [version, setVersion] = useState("");
  const [loader, setLoader] = useState<Loader>("vanilla");
  const [memory, setMemory] = useState(4096);
  const [search, setSearch] = useState("");
  const versions = useQuery({ queryKey: ["launcher", "versions"], queryFn: launcherApi.versions, staleTime: 300_000 });
  const caps = useQuery({ queryKey: ["launcher", "capabilities"], queryFn: launcherApi.capabilities });
  const available = versions.data?.filter(v => caps.data?.can_launch ? v.client_supported : v.type === "release") ?? [];
  const selected = version || available.find(v => v.id === "1.21.4")?.id || available[0]?.id || "";
  const visibleVersions = available.filter(v => v.id.toLowerCase().includes(search.toLowerCase()) || v.id === selected);
  const suggestedName = `${loaderInfo(loader).name} ${selected}`.trim();
  const create = useMutation({ mutationFn: launcherApi.create, onSuccess: async profile => {
    await client.invalidateQueries({ queryKey: ["launcher", "profiles"] });
    onCreated(profile);
  }});
  return <Dialog open onClose={onClose} title={t("launcher.newInstance")} width={700} dismissable={!create.isPending}
    header={<p className="launcher-muted">{t("launcher.createIntro")}</p>}
    footer={<><Button onClick={onClose} disabled={create.isPending}>{t("launcher.cancel")}</Button><Button form={formId} type="submit" variant="primary" icon="plus" disabled={create.isPending || !selected || !available.some(v => v.id === selected)}>{t(create.isPending ? "launcher.saving" : "launcher.createInstance")}</Button></>}>
    <form id={formId} className="launcher launcher-create" onSubmit={e => { e.preventDefault(); create.mutate({ name: name.trim() || suggestedName, game_version: selected, loader, memory_mb: memory }); }}>
      <section className="launcher-create-section">
        <h3><span className="launcher-step">1</span>{t("launcher.chooseVersion")}</h3>
        <div className="launcher-version-fields">
          <label>{t("launcher.searchVersion")}<input type="search" value={search} onChange={e => setSearch(e.target.value)} onKeyDown={e => { if (e.key === "Enter") e.preventDefault(); }} placeholder="1.21…" /></label>
          <label>{t("launcher.version")}<select value={selected} disabled={!available.length || create.isPending} onChange={e => setVersion(e.target.value)}>{visibleVersions.map(v => <option key={v.id}>{v.id}</option>)}</select></label>
        </div>
        {versions.isPending ? <p role="status">{t("launcher.loading")}</p> : null}
        {versions.error ? <><ErrorNotice error={toAppError(versions.error)} /><Button onClick={() => void versions.refetch()}>{t("launcher.retry")}</Button></> : null}
      </section>
      <fieldset className="launcher-create-section" disabled={create.isPending}>
        <legend><span className="launcher-step">2</span>{t("launcher.chooseLoader")}</legend>
        <div className="launcher-loader-options">{LOADERS.map(item => <label key={item.id} className="launcher-loader-option" data-selected={loader === item.id ? "" : undefined}>
          <input type="radio" name={`${formId}-loader`} value={item.id} checked={loader === item.id} onChange={() => setLoader(item.id)} />
          <LoaderIcon glyph={item.glyph} size={25} /><strong>{item.name}</strong><small>{t(item.id === "vanilla" ? "launcher.originalGame" : "launcher.configOnly")}</small>
        </label>)}</div>
        {loader !== "vanilla" ? <p className="launcher-notice" role="note">{t("launcher.loaderPending")}</p> : null}
      </fieldset>
      <section className="launcher-create-section">
        <h3><span className="launcher-step">3</span>{t("launcher.instanceDetails")}</h3>
        <label>{t("launcher.instanceName")}<input maxLength={64} value={name} onChange={e => setName(e.target.value)} placeholder={suggestedName} disabled={create.isPending} /></label>
        <details className="launcher-advanced"><summary>{t("launcher.advancedSettings")}</summary><label>{t("launcher.memory")}<input type="number" required min={512} max={32768} step={256} value={memory} onChange={e => setMemory(e.target.valueAsNumber)} /></label></details>
        <div className="launcher-create-summary"><LoaderIcon glyph={loaderInfo(loader).glyph} size={22} /><div><strong>{name.trim() || suggestedName}</strong><p>{selected} · {loaderInfo(loader).name} · {memory / 1024} GB RAM</p></div></div>
        <p className="launcher-muted">{t("launcher.creationNote")}</p>
      </section>
      {create.error || caps.error ? <ErrorNotice error={toAppError(create.error || caps.error)} /> : null}
    </form>
  </Dialog>;
}
