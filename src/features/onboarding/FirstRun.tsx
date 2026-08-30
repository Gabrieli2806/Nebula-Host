/**
 * First run: name the machine, see what it can do, make a server.
 *
 * No sign-up wall, because there is nothing to sign up to. The only thing this
 * asks for is a name for the PC, and the only thing it tells you is whether
 * the machine is ready.
 */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router-dom";

import { Mark } from "@/app/TitleBar";
import { keys, useNode, useRequirements } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { api } from "@/lib/ipc";
import type { Requirement } from "@/types/api";
import { Button, ErrorNotice, Field, Icon, TextInput } from "@/ui";

const ADVICE: Record<string, string> = {
  java: "firstRun.javaMissing",
  docker: "firstRun.dockerOptional",
  memory: "firstRun.memoryMissing",
  disk: "firstRun.diskMissing",
};

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
      localStorage.setItem("nebula.firstRunDone", "1");
      navigate("/servers", { replace: true });
    },
  });

  return (
    <div className="flex h-full items-center justify-center p-8" style={{ background: "var(--canvas)" }}>
      <div className="panel flex w-full max-w-[560px] flex-col gap-6 p-8">
        <div className="flex items-center gap-3">
          <Mark size={28} />
          <h1 className="text-[length:var(--text-xl)] font-semibold">{t("firstRun.welcome")}</h1>
        </div>

        <p className="text-[color:var(--text-muted)]">{t("firstRun.intro")}</p>

        <Field label={t("firstRun.nameLabel")} help={t("firstRun.nameHelp")}>
          <TextInput
            value={name || (node?.name ?? "")}
            maxLength={48}
            onChange={(event) => setName(event.target.value)}
          />
        </Field>

        <section className="flex flex-col gap-3">
          <h2 className="label">{t("firstRun.checking")}</h2>
          <ul className="flex flex-col gap-2">
            {requirements.data?.map((requirement) => (
              <RequirementRow key={requirement.id} requirement={requirement} />
            ))}
          </ul>
          {requirements.error ? (
            <ErrorNotice error={requirements.error} onRetry={() => void requirements.refetch()} />
          ) : null}
        </section>

        {finish.error ? <ErrorNotice error={finish.error} /> : null}

        <div className="flex items-center justify-between">
          <Button
            variant="quiet"
            onClick={() => {
              localStorage.setItem("nebula.firstRunDone", "1");
              navigate("/servers", { replace: true });
            }}
          >
            {t("firstRun.skip")}
          </Button>
          <Button variant="primary" disabled={finish.isPending} onClick={() => finish.mutate()}>
            {t("firstRun.finish")}
          </Button>
        </div>
      </div>
    </div>
  );
}

function RequirementRow({ requirement }: { requirement: Requirement }) {
  const { t } = useI18n();

  const colour =
    requirement.status === "ready"
      ? "var(--status-online)"
      : requirement.status === "optional"
        ? "var(--text-faint)"
        : "var(--status-crashed)";

  return (
    <li className="flex items-start gap-3 rounded-[var(--radius)] border border-[color:var(--line)] px-3 py-2.5">
      <span className="mt-0.5 flex-none" style={{ color: colour }}>
        <Icon name={requirement.status === "ready" ? "check" : "warning"} size={14} />
      </span>
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="flex items-center gap-2">
          <span className="font-medium">
            {t(`firstRun.requirement${requirement.id.charAt(0).toUpperCase()}${requirement.id.slice(1)}`)}
          </span>
          <span className="text-[length:var(--text-xs)]" style={{ color: colour }}>
            {t(`firstRun.${requirement.status}`)}
          </span>
        </span>
        <span className="mono text-[length:var(--text-xs)] text-[color:var(--text-faint)]">
          {requirement.found ??
            (requirement.status === "ready" ? "" : t(ADVICE[requirement.id] ?? ""))}
        </span>
        {requirement.status !== "ready" && requirement.found ? (
          <span className="text-[length:var(--text-xs)] text-[color:var(--text-muted)]">
            {t(ADVICE[requirement.id] ?? "")}
          </span>
        ) : null}
      </div>
    </li>
  );
}
