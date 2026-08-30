/**
 * Creating a server: pick a build, name it.
 *
 * There is no game picker, because one game ships. The moment a second
 * manifest exists this grows a step — but an app that hosts only Minecraft
 * must not ask which game you meant.
 *
 * Nothing here knows what Minecraft *is*, though: the distributions, their
 * descriptions and their versions all come from the manifest.
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { InstallProgressPanel } from "./InstallProgressPanel";
import { keys, useGames } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, Field, Select, TextInput } from "@/ui";

type Step = "choose" | "installing";

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

  const [step, setStep] = useState<Step>("choose");
  // Each choice is "what the user picked, or the sensible default" — derived
  // during render rather than synced by an effect, so the dialog never paints
  // one frame with an empty select.
  const [chosenDistribution, setChosenDistribution] = useState<string | null>(null);
  const [chosenVersion, setChosenVersion] = useState<string | null>(null);
  const [loaderVersion, setLoaderVersion] = useState<string>("");
  const [name, setName] = useState("");
  const [memoryMb, setMemoryMb] = useState(4096);
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
    setStep("choose");
    setInstanceId(null);
    setName("");
    setChosenVersion(null);
    setLoaderVersion("");
    create.reset();
  };

  const close = () => {
    reset();
    onClose();
  };

  return (
    <Dialog
      open={open}
      onClose={close}
      title={step === "installing" ? t("install.title", { name }) : t("create.title")}
      width={560}
      footer={
        step === "choose" ? (
          <>
            <Button variant="quiet" onClick={close}>
              {t("actions.cancel")}
            </Button>
            <Button
              variant="primary"
              disabled={!name.trim() || !version || create.isPending}
              onClick={() => create.mutate()}
            >
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
        )
      }
    >
      {step === "choose" ? (
        <div className="flex flex-col gap-4">
          <Field label={t("create.name")}>
            <TextInput
              value={name}
              placeholder={t("create.namePlaceholder")}
              onChange={(event) => setName(event.target.value)}
              maxLength={64}
            />
          </Field>

          <Field label={t("create.distribution")} help={distribution ? t(distribution.summary_key) : undefined}>
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
            ) : (
              <Field label={t("create.memory")}>
                <Select
                  value={String(memoryMb)}
                  onChange={(event) => setMemoryMb(Number(event.target.value))}
                >
                  {[2048, 4096, 6144, 8192, 12288, 16384].map((value) => (
                    <option key={value} value={value}>
                      {value / 1024} GB
                    </option>
                  ))}
                </Select>
              </Field>
            )}
          </div>

          {versions.error ? <ErrorNotice error={versions.error} onRetry={() => void versions.refetch()} /> : null}
          {create.error ? <ErrorNotice error={create.error} /> : null}
        </div>
      ) : (
        <InstallProgressPanel instanceId={instanceId} error={create.error} />
      )}
    </Dialog>
  );
}
