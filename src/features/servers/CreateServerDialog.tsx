/**
 * Creating a server: game, build, details. A genuine sequence, so the header
 * numbers its steps.
 *
 * There is no game picker, because one game ships: the Game step is shown
 * already done, with the game's name, and the dialog opens on Build. The
 * moment a second manifest exists that step becomes a real choice, but an app
 * that hosts only Minecraft must not ask which game you meant.
 *
 * Nothing here knows what Minecraft *is*, though: the distributions, their
 * descriptions, colours and versions all come from the manifest.
 */

import { useId, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { InstallProgressPanel } from "./InstallProgressPanel";
import { keys, useGames } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, Field, Icon, Select, TextInput, identityProps } from "@/ui";
import { RamSlider, useRamPlan } from "@/ui/RamSlider";

type Step = "build" | "details" | "installing";

/** The header's sequence. Installing is not a step: it is what Create does. */
const SEQUENCE = ["game", "build", "details"] as const;
type SequenceStep = (typeof SEQUENCE)[number];

const STEP_LABEL: Record<SequenceStep, string> = {
  game: "create.stepGame",
  build: "create.stepBuild",
  details: "create.stepDetails",
};

export function CreateServerDialog({
  open,
  onClose,
  onCreated,
}: {
  open: boolean;
  onClose: () => void;
  onCreated: (instanceId: string) => void;
}) {
  const { t } = useI18n();
  const client = useQueryClient();
  const { data: games } = useGames();

  const [step, setStep] = useState<Step>("build");
  // Each choice is "what the user picked, or the sensible default": derived
  // during render rather than synced by an effect, so the dialog never paints
  // one frame with an empty select.
  const [chosenDistribution, setChosenDistribution] = useState<string | null>(null);
  const [chosenVersion, setChosenVersion] = useState<string | null>(null);
  const [loaderVersion, setLoaderVersion] = useState<string>("");
  const [name, setName] = useState("");
  const [chosenMemoryMb, setChosenMemoryMb] = useState<number | null>(null);
  const [instanceId, setInstanceId] = useState<string | null>(null);

  // The only game there is. When there are two, this becomes a first step.
  const game = games?.[0];
  const gameId = game?.id ?? "";

  // Default to the second distribution: for Minecraft that is Paper, which is
  // what most people actually want.
  const distribution = useMemo(() => {
    if (!game) return undefined;
    return (
      game.distributions.find((candidate) => candidate.id === chosenDistribution) ??
      game.distributions[1] ??
      game.distributions[0]
    );
  }, [game, chosenDistribution]);
  const distributionId = distribution?.id ?? "";

  // Memory follows the same rule: the user's amount, or what this kind of
  // server is recommended on this machine (4 GB for Paper, more for mods).
  const ramPlan = useRamPlan(distribution?.addon_project_type);
  const memoryMb = chosenMemoryMb ?? ramPlan.recommendedMb;
  const memoryLabelId = useId();
  const memorySliderId = useId();

  const versions = useQuery({
    queryKey: ["versions", gameId, distributionId],
    queryFn: () => api.games.versions(gameId, distributionId),
    enabled: open && Boolean(gameId && distributionId),
  });

  // Newest version unless the user picked another; a picked version that this
  // distribution does not offer falls back rather than sticking.
  const version =
    versions.data?.find((candidate) => candidate.id === chosenVersion)?.id ??
    versions.data?.[0]?.id ??
    "";

  const loaderVersions = useQuery({
    queryKey: ["loaders", gameId, distributionId, version],
    queryFn: () => api.games.loaderVersions(gameId, distributionId, version),
    enabled: open && Boolean(distribution?.has_loader_version && version),
  });

  const create = useMutation({
    mutationFn: async () => {
      const instance = await api.instances.create({
        game: gameId,
        name,
        build: {
          distribution: distributionId,
          version,
          loader_version: loaderVersion || null,
        },
        memoryMb,
      });
      setInstanceId(instance.id);
      setStep("installing");
      await api.instances.install(instance.id);
      return instance;
    },
    onSuccess: () => client.invalidateQueries({ queryKey: keys.instances }),
  });

  const reset = () => {
    setStep("build");
    setInstanceId(null);
    setName("");
    setChosenVersion(null);
    setLoaderVersion("");
    setChosenMemoryMb(null);
    create.reset();
  };

  const close = () => {
    reset();
    onClose();
  };

  const canCreate = Boolean(name.trim() && version) && !create.isPending;

  const footer =
    step === "build" ? (
      <>
        <Button variant="quiet" onClick={close}>
          {t("actions.cancel")}
        </Button>
        <Button variant="primary" disabled={!version} onClick={() => setStep("details")}>
          {t("actions.next")}
        </Button>
      </>
    ) : step === "details" ? (
      <>
        <Button
          variant="quiet"
          icon="chevron-left"
          className="mr-auto"
          disabled={create.isPending}
          onClick={() => setStep("build")}
        >
          {t("actions.back")}
        </Button>
        <Button variant="quiet" onClick={close}>
          {t("actions.cancel")}
        </Button>
        <Button variant="primary" disabled={!canCreate} onClick={() => create.mutate()}>
          {create.isPending ? t("create.creating") : t("actions.create")}
        </Button>
      </>
    ) : (
      <Button
        variant="primary"
        disabled={create.isPending}
        onClick={() => {
          if (instanceId) onCreated(instanceId);
          reset();
        }}
      >
        {t("actions.done")}
      </Button>
    );

  return (
    <Dialog
      open={open}
      onClose={close}
      title={step === "installing" ? t("install.title", { name }) : t("create.title")}
      width={560}
      header={
        step === "installing" ? undefined : <StepHeader current={step} gameName={game?.name} />
      }
      footer={footer}
    >
      {step === "build" ? (
        <div className="flex flex-col gap-4">
          <Field
            label={t("create.distribution")}
            help={distribution ? t(distribution.summary_key) : undefined}
          >
            {/* The distribution's colour rides along as a small swatch inside
                the field: the same colour its card will carry. */}
            <span className="create-distribution" {...identityProps(distribution?.accent)}>
              <span className="create-distribution-swatch" aria-hidden="true" />
              <Select
                value={distributionId}
                onChange={(event) => {
                  setChosenDistribution(event.target.value);
                  setLoaderVersion("");
                }}
              >
                {game?.distributions.map((candidate) => (
                  <option key={candidate.id} value={candidate.id}>
                    {candidate.name}
                  </option>
                ))}
              </Select>
            </span>
          </Field>

          <div className="grid grid-cols-2 gap-4">
            <Field label={t("create.version")}>
              <Select
                value={version}
                disabled={versions.isLoading}
                onChange={(event) => {
                  setChosenVersion(event.target.value);
                  setLoaderVersion("");
                }}
              >
                {versions.data?.map((candidate) => (
                  <option key={candidate.id} value={candidate.id}>
                    {candidate.id}
                  </option>
                ))}
              </Select>
            </Field>

            {distribution?.has_loader_version ? (
              <Field label={t("create.loaderVersion")}>
                <Select
                  value={loaderVersion}
                  disabled={loaderVersions.isLoading}
                  onChange={(event) => setLoaderVersion(event.target.value)}
                >
                  <option value="">{t("create.latest")}</option>
                  {loaderVersions.data?.map((candidate) => (
                    <option key={candidate.id} value={candidate.id}>
                      {candidate.id}
                    </option>
                  ))}
                </Select>
              </Field>
            ) : null}
          </div>

          {versions.error ? (
            <ErrorNotice error={versions.error} onRetry={() => void versions.refetch()} />
          ) : null}
        </div>
      ) : step === "details" ? (
        <div className="flex flex-col gap-4">
          <Field label={t("create.name")}>
            <TextInput
              value={name}
              placeholder={t("create.namePlaceholder")}
              onChange={(event) => setName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && canCreate) create.mutate();
              }}
              maxLength={64}
              autoFocus
            />
          </Field>

          {/* Not a <Field>: that is one <label>, and this group holds two
              inputs (the slider and the exact amount). */}
          <div className="flex flex-col gap-1.5">
            <label
              id={memoryLabelId}
              htmlFor={memorySliderId}
              className="text-[length:var(--text-sm)] font-[600]"
            >
              {t("create.memory")}
            </label>
            <RamSlider
              id={memorySliderId}
              labelledBy={memoryLabelId}
              value={memoryMb}
              onChange={setChosenMemoryMb}
              projectType={distribution?.addon_project_type}
            />
          </div>

          <BuildSummary
            accent={distribution?.accent}
            distributionName={distribution?.name}
            version={version}
            loaderVersion={
              distribution?.has_loader_version ? loaderVersion || t("create.latest") : null
            }
          />

          {create.error ? <ErrorNotice error={create.error} /> : null}
        </div>
      ) : (
        <InstallProgressPanel instanceId={instanceId} error={create.error} />
      )}
    </Dialog>
  );
}

/**
 * Game, Build, Details. Done steps show a check and what was chosen; the
 * current one is filled with the accent. `aria-current="step"` names it for screen readers.
 */
function StepHeader({ current, gameName }: { current: "build" | "details"; gameName?: string }) {
  const { t } = useI18n();
  const currentIndex = SEQUENCE.indexOf(current);

  return (
    <ol
      className="create-steps"
      aria-label={t("create.stepOf", { step: currentIndex + 1, total: SEQUENCE.length })}
    >
      {SEQUENCE.map((id, index) => {
        const state = index < currentIndex ? "done" : index === currentIndex ? "current" : "next";
        return (
          <li
            key={id}
            className="create-step"
            data-state={state}
            aria-current={state === "current" ? "step" : undefined}
          >
            <span className="create-step-mark" aria-hidden="true">
              {state === "done" ? <Icon name="check" size={11} /> : index + 1}
            </span>
            <span className="create-step-label">
              {t(STEP_LABEL[id])}
              {id === "game" && gameName ? (
                <span className="create-step-value">{gameName}</span>
              ) : null}
            </span>
          </li>
        );
      })}
    </ol>
  );
}

/** What Create will build, as the tags its card will show. */
function BuildSummary({
  accent,
  distributionName,
  version,
  loaderVersion,
}: {
  accent?: string | null;
  distributionName?: string;
  version: string;
  loaderVersion: string | null;
}) {
  const { t } = useI18n();
  if (!distributionName) return null;
  return (
    <div className="create-summary">
      <span className="label">{t("create.stepBuild")}</span>
      <span className="flex flex-wrap items-center gap-1.5">
        <span className="tag" {...identityProps(accent)}>
          {distributionName}
        </span>
        {version ? <span className="tag">{version}</span> : null}
        {loaderVersion ? (
          <span className="tag">
            {t("create.loaderVersion")} {loaderVersion}
          </span>
        ) : null}
      </span>
    </div>
  );
}
