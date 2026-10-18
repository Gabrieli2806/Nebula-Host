/**
 * One Modrinth project: its versions that run on this server, what the chosen
 * version depends on, and the install (or version switch) itself.
 */

import { useMemo, useState } from "react";

import {
  useAddonDependencies,
  useAddonMutations,
  useAddonVersions,
  useInstalledAddons,
} from "@/hooks/useBackend";
import { formatBytes } from "@/lib/format";
import type { AddonContext, AddonDependency, AddonVersion, InstanceView } from "@/types/api";
import { Button, Dialog, ErrorNotice, Icon, Skeleton } from "@/ui";

import { AddonIcon } from "./AddonIcon";
import {
  compactCount,
  loaderName,
  primaryLoader,
  modrinthUrl,
  openExternal,
  pickBest,
  relativeDate,
  useAddonCopy,
} from "./util";

export interface ProjectTarget {
  projectId: string;
  /** What the opener already knows, shown while the versions load. */
  project: {
    title: string;
    description: string;
    icon_url?: string | null;
    slug: string;
    project_type: string;
    author?: string;
  };
}

export function ProjectDialog({
  instance,
  context,
  target,
  onClose,
}: {
  instance: InstanceView;
  context: AddonContext;
  target: ProjectTarget | null;
  onClose: () => void;
}) {
  const { t } = useAddonCopy();
  return (
    <Dialog
      open={target !== null}
      onClose={onClose}
      title={target?.project.title ?? t("addons.project")}
      width={640}
      footer={null}
    >
      {target ? (
        <ProjectBody key={target.projectId} instance={instance} context={context} target={target} onClose={onClose} />
      ) : null}
    </Dialog>
  );
}

function ProjectBody({
  instance,
  context,
  target,
  onClose,
}: {
  instance: InstanceView;
  context: AddonContext;
  target: ProjectTarget;
  onClose: () => void;
}) {
  const { t, tc, language } = useAddonCopy();
  const { project, projectId } = target;
  const versions = useAddonVersions(instance.id, projectId);
  const installed = useInstalledAddons(instance.id);
  const { installVersion } = useAddonMutations(instance.id);

  const current = installed.data?.addons.find((entry) => entry.modrinth?.project.project_id === projectId);
  const currentVersionId = current?.modrinth?.installed_version.id;

  const [picked, setPicked] = useState<string | null>(null);
  const best = useMemo(() => (versions.data ? pickBest(versions.data, context) : undefined), [versions.data, context]);
  const selected: AddonVersion | undefined =
    versions.data?.find((version) => version.id === picked) ??
    // An update on offer is the natural pick; otherwise stay on what is installed.
    (current?.modrinth?.update_available
      ? versions.data?.find((version) => version.id === current.modrinth?.latest_version?.id)
      : undefined) ??
    versions.data?.find((version) => version.id === currentVersionId && version.compatible) ??
    best;

  const deps = useAddonDependencies(instance.id, selected?.id ?? null);
  // Choices belong to one version: a new version starts from its defaults
  // (required on, optional off).
  const [choice, setChoice] = useState<{ version?: string; map: Record<string, boolean> }>({ map: {} });
  const chosen = choice.version === selected?.id ? choice.map : {};
  const setChosen = (projectId: string, value: boolean) =>
    setChoice({ version: selected?.id, map: { ...chosen, [projectId]: value } });

  const visibleDeps = (deps.data ?? []).filter((dep) => dep.dependency_type !== "embedded");
  const isChosen = (dep: AddonDependency) =>
    chosen[dep.project_id] ?? (dep.dependency_type === "required" && !dep.installed && Boolean(dep.version));
  const toInstall = visibleDeps.filter(
    (dep) => !dep.installed && dep.version && dep.dependency_type !== "incompatible" && isChosen(dep),
  );
  const conflicts = visibleDeps.filter((dep) => dep.dependency_type === "incompatible" && dep.installed);
  const missingRequired = visibleDeps.filter(
    (dep) => dep.dependency_type === "required" && !dep.installed && !isChosen(dep),
  );

  const [step, setStep] = useState<string | null>(null);
  const [failure, setFailure] = useState<unknown>(null);

  const isCurrent = Boolean(selected && selected.id === currentVersionId);

  const run = async () => {
    if (!selected) return;
    setFailure(null);
    try {
      for (const dep of toInstall) {
        setStep(t("addons.installingName", { name: dep.project?.title ?? dep.project_id }));
        await installVersion.mutateAsync({ projectId: dep.project_id, versionId: dep.version!.id });
      }
      setStep(t("addons.installingName", { name: project.title }));
      await installVersion.mutateAsync({
        projectId,
        versionId: selected.id,
        replace: current?.file_name,
      });
      setStep(null);
      onClose();
    } catch (error) {
      setStep(null);
      setFailure(error);
    }
  };

  const author = project.author;

  return (
    <div className="addons-project">
      <div className="addons-project-head">
        <AddonIcon url={project.icon_url} size={56} />
        <div className="min-w-0 flex-1">
          <p className="addons-project-by">
            {author ? <span>{t("addons.byAuthor", { author })}</span> : null}
            <span>
              {primaryLoader(context)} {context.game_version}
            </span>
          </p>
          <p className="addons-project-desc">{project.description}</p>
        </div>
        <Button
          size="sm"
          variant="quiet"
          icon="external"
          onClick={() => void openExternal(modrinthUrl(project.project_type, project.slug))}
        >
          {t("addons.viewOnModrinth")}
        </Button>
      </div>

      <section className="addons-project-section" aria-labelledby="addons-versions-title">
        <div className="addons-project-section-head">
          <h3 id="addons-versions-title" className="addons-section-title">
            {t("addons.versionTitle")}
          </h3>
          {versions.data && versions.data.length > 0 ? (
            <span className="addons-summary">
              {tc("addons.versionsCompatible", versions.data.length)}
            </span>
          ) : null}
        </div>
        {versions.error ? (
          <ErrorNotice error={versions.error} compact onRetry={() => void versions.refetch()} />
        ) : !versions.data ? (
          <div className="flex flex-col gap-2" aria-hidden="true">
            <Skeleton className="h-11" />
            <Skeleton className="h-11" />
          </div>
        ) : versions.data.length === 0 ? (
          <p className="addons-note">
            <Icon name="warning" />
            <span>
              {t("addons.noCompatible", {
                loader: primaryLoader(context),
                version: context.game_version,
              })}
            </span>
          </p>
        ) : (
          <div className="addons-versions" role="radiogroup" aria-labelledby="addons-versions-title">
            {versions.data.map((version) => (
              <label key={version.id} className="addons-version" data-selected={version.id === selected?.id ? "" : undefined}>
                <input
                  type="radio"
                  name={`addon-version-${projectId}`}
                  className="sr-only"
                  checked={version.id === selected?.id}
                  onChange={() => setPicked(version.id)}
                />
                <span className="addons-version-mark" aria-hidden="true" />
                <span className="addons-version-number mono">{version.version_number}</span>
                {version.version_type !== "release" ? (
                  <span className="tag" data-tone="warn">
                    {t(`addons.channel_${version.version_type}`)}
                  </span>
                ) : null}
                {version.id === currentVersionId ? (
                  <span className="tag" data-tone="accent">
                    {t("addons.currentVersion")}
                  </span>
                ) : version.id === best?.id ? (
                  <span className="tag">{t("addons.recommended")}</span>
                ) : null}
                <span className="addons-version-meta">
                  <span>{version.loaders.map(loaderName).join(", ")}</span>
                  <span className="mono">{formatBytes(version.file.size)}</span>
                  {relativeDate(version.date_published, language) ? (
                    <span>{relativeDate(version.date_published, language)}</span>
                  ) : null}
                  <span className="mono" title={t("addons.downloads", { count: version.downloads })}>
                    {compactCount(version.downloads, language)}
                  </span>
                </span>
              </label>
            ))}
          </div>
        )}
      </section>

      {selected ? (
        <section className="addons-project-section" aria-labelledby="addons-deps-title">
          <div className="addons-project-section-head">
            <h3 id="addons-deps-title" className="addons-section-title">
              {t("addons.dependencies")}
            </h3>
          </div>
          {deps.error ? (
            <ErrorNotice error={deps.error} compact onRetry={() => void deps.refetch()} />
          ) : !deps.data ? (
            <Skeleton className="h-11" />
          ) : visibleDeps.length === 0 ? (
            <p className="addons-summary">{t("addons.noDependencies")}</p>
          ) : (
            <ul className="addons-deps">
              {visibleDeps.map((dep) => (
                <DependencyRow
                  key={dep.project_id}
                  dep={dep}
                  checked={isChosen(dep)}
                  onChange={(value) => setChosen(dep.project_id, value)}
                />
              ))}
            </ul>
          )}
        </section>
      ) : null}

      {conflicts.length > 0 ? (
        <p className="addons-note" role="status">
          <Icon name="warning" />
          <span>
            {t("addons.conflicts", {
              names: conflicts.map((dep) => dep.project?.title ?? dep.project_id).join(", "),
            })}
          </span>
        </p>
      ) : null}

      {failure ? <ErrorNotice error={failure} /> : null}

      <footer className="dialog-footer addons-project-foot">
        <p className="addons-summary min-w-0 flex-1" aria-live="polite">
          {step ??
            (missingRequired.length > 0
              ? tc("addons.missingRequired", missingRequired.length)
              : current && selected && !isCurrent
                ? t("addons.replaces", { file: current.file_name })
                : toInstall.length > 0
                  ? tc("addons.alsoInstalls", toInstall.length)
                  : null)}
        </p>
        <Button variant="quiet" onClick={onClose}>
          {t("actions.cancel")}
        </Button>
        <Button
          variant="primary"
          icon={isCurrent ? "check" : current ? "refresh" : "plus"}
          disabled={!selected || isCurrent || step !== null}
          onClick={() => void run()}
        >
          {isCurrent
            ? t("addons.installedState")
            : step !== null
              ? t("addons.installing")
              : current
                ? t("addons.switchTo", { version: selected?.version_number ?? "" })
                : t("addons.installVersion", { version: selected?.version_number ?? "" })}
        </Button>
      </footer>
    </div>
  );
}

function DependencyRow({
  dep,
  checked,
  onChange,
}: {
  dep: AddonDependency;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  const { t } = useAddonCopy();
  const title = dep.project?.title ?? dep.project_id;
  const incompatible = dep.dependency_type === "incompatible";
  const installable = !dep.installed && Boolean(dep.version) && !incompatible;

  let state;
  if (incompatible) {
    state = dep.installed ? (
      <span className="tag" data-tone="warn">
        {t("addons.conflictInstalled")}
      </span>
    ) : (
      <span className="addons-summary">{t("addons.notInstalled")}</span>
    );
  } else if (dep.installed) {
    state = (
      <span className="addons-installed">
        <Icon name="check" />
        {t("addons.installedState")}
      </span>
    );
  } else if (!dep.version) {
    state = (
      <span className="tag" data-tone="warn">
        {t("addons.noCompatibleShort")}
      </span>
    );
  } else {
    state = (
      <label className="addons-check">
        <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
        <span>{t("addons.installToo", { version: dep.version.version_number })}</span>
      </label>
    );
  }

  return (
    <li className="addons-dep" data-installable={installable ? "" : undefined}>
      <AddonIcon url={dep.project?.icon_url} size={28} />
      <div className="min-w-0 flex-1">
        <p className="addons-name truncate">{title}</p>
        <p className="addons-meta">{t(`addons.dep_${dep.dependency_type}`)}</p>
      </div>
      {state}
    </li>
  );
}
