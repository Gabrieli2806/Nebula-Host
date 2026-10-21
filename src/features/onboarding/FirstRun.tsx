/**
 * First run: name the machine, see what it can do, make a server.
 *
 * No sign-up wall, because there is nothing to sign up to. The only thing this
 * asks for is a name for the PC, and the only thing it tells you is whether
 * the machine is ready.
 *
 * It is the only centred screen. It still has the title bar: the window is
 * undecorated, so without it there is no way to move or close the window.
 */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Navigate, useNavigate } from "react-router-dom";

import { TitleBar } from "@/app/TitleBar";
import { keys, useNode, useNodeHealth, useRequirements } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { formatMegabytes } from "@/lib/format";
import { JavaInstallButton } from "@/features/java/JavaInstall";
import { api } from "@/lib/ipc";
import type { InstanceStatus, Requirement } from "@/types/api";
import { isRemote } from "@/lib/connection";
import { IS_MOBILE } from "@/lib/web";
import { PHONE_HOSTING_KEY, PhoneHostingNotice, PhoneWelcome } from "./PhoneWelcome";
import { Button, ErrorNotice, Field, Mark, Skeleton, StatusDot, TextInput } from "@/ui";

const ADVICE: Record<string, string> = {
  java: "firstRun.javaMissing",
  docker: "firstRun.dockerOptional",
  memory: "firstRun.memoryMissing",
  disk: "firstRun.diskMissing",
};

/** Requirement state as a status lamp: ready is the online dot, optional the
 *  hollow offline ring, missing the crashed triangle. Shape and word, never colour alone. */
const LAMP: Record<Requirement["status"], InstanceStatus> = {
  ready: "online",
  optional: "offline",
  missing: "crashed",
};

const FIRST_RUN_KEY = "nebula.firstRunDone";

export function FirstRun() {
  const { t } = useI18n();
  const navigate = useNavigate();
  const client = useQueryClient();
  const { data: node } = useNode();
  const requirements = useRequirements();
  const [name, setName] = useState("");

  const finish = useMutation({
    mutationFn: async () => {
      const chosen = name.trim();
      if (chosen && chosen !== node?.name) {
        const updated = await api.node.rename(chosen);
        client.setQueryData(keys.node, updated);
      }
    },
    onSuccess: () => {
      localStorage.setItem(FIRST_RUN_KEY, "1");
      // ServersScreen opens the create dialog when it sees create=1.
      navigate("/servers?create=1", { replace: true });
    },
  });

  // The phone app opens on "manage your computer" first; hosting here is the
  // second choice. Once it manages another machine, setup is done.
  const [hostHere, setHostHere] = useState(() => {
    try {
      return localStorage.getItem(PHONE_HOSTING_KEY) === "1";
    } catch {
      return false;
    }
  });
  if (IS_MOBILE && isRemote()) return <Navigate to="/servers" replace />;
  if (IS_MOBILE && !hostHere) return <PhoneWelcome onHostHere={() => setHostHere(true)} />;

  const ready =
    requirements.data !== undefined &&
    requirements.data.every((requirement) => requirement.status !== "missing");

  return (
    <div className="flex h-full flex-col">
      <TitleBar />
      <main className="first-run min-h-0 flex-1 overflow-auto">
        <div className="first-run-stage">
          <Mark size={48} />
          <h1 className="display first-run-title">
            {t("firstRun.heading")}
          </h1>
          <p className="first-run-intro">{t("firstRun.intro")}</p>

          <section className="first-run-card" aria-label={t("firstRun.welcome")}>
            {IS_MOBILE ? <PhoneHostingNotice /> : null}
            <Field label={t("firstRun.nameLabel")} help={t("firstRun.nameHelp")}>
              <TextInput
                value={name || (node?.name ?? "")}
                maxLength={48}
                onChange={(event) => setName(event.target.value)}
              />
            </Field>

            <section className="flex flex-col gap-2" aria-live="polite">
              <h2 className="label">{ready ? t("firstRun.readyToHost") : t("firstRun.checking")}</h2>
              <ul className="first-run-requirements">
                {requirements.data
                  ? requirements.data.map((requirement) => (
                      <RequirementRow key={requirement.id} requirement={requirement} />
                    ))
                  : requirements.isPending
                    ? [0, 1, 2, 3].map((index) => (
                        <li key={index} className="first-run-requirement">
                          <Skeleton className="h-3 w-full rounded-[var(--radius-sm)]" />
                        </li>
                      ))
                    : null}
              </ul>
              {requirements.error ? (
                <ErrorNotice error={requirements.error} onRetry={() => void requirements.refetch()} />
              ) : null}
            </section>

            {finish.error ? <ErrorNotice error={finish.error} /> : null}

            <footer className="first-run-footer">
              <Button icon="play" onClick={() => navigate("/launcher")}>Launcher</Button>
              <Button
                variant="quiet"
                onClick={() => {
                  localStorage.setItem(FIRST_RUN_KEY, "1");
                  navigate("/servers", { replace: true });
                }}
              >
                {t("firstRun.skip")}
              </Button>
              <Button variant="primary" disabled={finish.isPending} onClick={() => finish.mutate()}>
                {t("firstRun.finish")}
              </Button>
            </footer>
          </section>
        </div>
      </main>
    </div>
  );
}

function RequirementRow({ requirement }: { requirement: Requirement }) {
  const { t } = useI18n();
  const adviceKey = ADVICE[requirement.id];
  const advice = adviceKey ? t(adviceKey) : null;
  const labelKey = `firstRun.requirement${requirement.id.charAt(0).toUpperCase()}${requirement.id.slice(1)}`;
  const translated = t(labelKey);
  // A check the copy does not know yet shows its id, not a raw i18n key.
  const label = translated === labelKey ? requirement.id : translated;
  const found = useFoundText(requirement);

  return (
    <li className="first-run-requirement" data-state={requirement.status}>
      <div className="flex min-w-0 items-center gap-2.5">
        <StatusDot status={LAMP[requirement.status]} />
        <span className="font-[var(--weight-control)]">{label}</span>
        <span className="first-run-requirement-state">{t(`firstRun.${requirement.status}`)}</span>
        {found ? (
          <span className="first-run-requirement-found mono selectable" title={found}>
            {found}
          </span>
        ) : null}
      </div>
      {requirement.status !== "ready" && advice ? (
        <p className="first-run-requirement-advice">{advice}</p>
      ) : null}
      {requirement.id === "java" && requirement.status === "missing" ? (
        <div className="first-run-requirement-advice">
          <JavaInstallButton major={25} />
        </div>
      ) : null}
    </li>
  );
}

/**
 * What a check found, in the UI's language. The backend's `found` is English
 * ("412 GB free", "32768 MB"), so memory and disk are re-read from the node's
 * health numbers and formatted here; anything else (a Java version) is shown
 * as sent.
 */
function useFoundText(requirement: Requirement): string | null {
  const { t } = useI18n();
  const { data: health } = useNodeHealth();
  if (health && requirement.id === "memory") return formatMegabytes(health.memory_total_mb);
  if (health && requirement.id === "disk") {
    const free = Math.max(0, health.disk_total_gb - health.disk_used_gb);
    return t("firstRun.diskFree", { free: `${free} GB` });
  }
  return requirement.found ?? null;
}
