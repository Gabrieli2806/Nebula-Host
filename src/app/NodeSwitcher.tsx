/**
 * The machine picker in the title bar: "PC de Gabriel ▾". Lists this PC and
 * every paired node; "Connect to another machine…" pairs a new one with the
 * code it shows (`nebula-node pair`, or Settings › Remote access on another
 * Nebula Host).
 */

import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { activeRemote, forgetNode, pair, remoteNodes, switchTo, type RemoteNode } from "@/lib/connection";
import { toAppError } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, Field, Icon, TextInput } from "@/ui";

import "./NodeSwitcher.css";

/** This PC's own name — always asked of the local backend, never routed to
 *  the machine being managed (otherwise "This PC" shows the remote's name). */
function useLocalName(): string | undefined {
  const { data } = useQuery({
    queryKey: ["local-node-name"],
    queryFn: async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      return (await invoke<{ name: string }>("node_get")).name;
    },
    staleTime: 60_000,
  });
  return data;
}

export function NodeSwitcher() {
  const { t } = useI18n();
  const localName = useLocalName();
  const [open, setOpen] = useState(false);
  const [pairing, setPairing] = useState(false);
  const active = activeRemote();
  const nodes = remoteNodes();
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => event.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  const label = active ? active.name : (localName ?? t("node.thisMachine"));

  return (
    <div className="node-switcher no-drag" ref={menuRef}>
      <button
        type="button"
        className="node-switcher-button"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        title={active ? `${active.name} — ${active.address}` : undefined}
      >
        {active ? <Icon name="link" size={13} /> : null}
        <span className="titlebar-node">{label}</span>
        <Icon name="chevron-down" size={12} />
      </button>

      {open ? (
        <div className="node-switcher-menu" role="menu">
          <MenuItem
            current={!active}
            title={localName ?? t("node.thisMachine")}
            subtitle={t("remote.thisPc")}
            onSelect={() => (active ? switchTo("local") : setOpen(false))}
          />
          {nodes.map((node) => (
            <MenuItem
              key={node.id}
              current={active?.id === node.id}
              title={node.name}
              subtitle={node.address}
              onSelect={() => (active?.id === node.id ? setOpen(false) : switchTo(node.id))}
              onForget={() => {
                if (window.confirm(t("remote.forgetConfirm", { name: node.name }))) forgetNode(node.id);
                setOpen(false);
              }}
            />
          ))}
          <div className="node-switcher-sep" role="separator" />
          {/* Every machine at once: all their servers in one list, and each
              machine's load. Only worth offering once there is another. */}
          {nodes.length ? (
            <>
              <button
                type="button"
                role="menuitem"
                className="node-switcher-item node-switcher-add"
                onClick={() => {
                  setOpen(false);
                  window.location.hash = "#/machines?view=servers";
                }}
              >
                <Icon name="servers" size={14} />
                <span>{t("fleet.menuAllServers")}</span>
              </button>
              <button
                type="button"
                role="menuitem"
                className="node-switcher-item node-switcher-add"
                onClick={() => {
                  setOpen(false);
                  window.location.hash = "#/machines?view=machines";
                }}
              >
                <Icon name="activity" size={14} />
                <span>{t("fleet.menuMachines")}</span>
              </button>
              <div className="node-switcher-sep" role="separator" />
            </>
          ) : null}
          <button
            type="button"
            role="menuitem"
            className="node-switcher-item node-switcher-add"
            onClick={() => {
              setOpen(false);
              setPairing(true);
            }}
          >
            <Icon name="plus" size={14} />
            <span>{t("remote.connectOther")}</span>
          </button>
        </div>
      ) : null}

      {/* Remounted per opening, so every attempt starts with a clean form. */}
      {pairing ? <PairDialog open onClose={() => setPairing(false)} /> : null}
    </div>
  );
}

function MenuItem({
  current,
  title,
  subtitle,
  onSelect,
  onForget,
}: {
  current: boolean;
  title: string;
  subtitle: string;
  onSelect: () => void;
  onForget?: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="node-switcher-row">
      <button type="button" role="menuitemradio" aria-checked={current} className="node-switcher-item" onClick={onSelect}>
        <span className="node-switcher-check" aria-hidden="true">
          {current ? <Icon name="check" size={13} /> : null}
        </span>
        <span className="node-switcher-text">
          <span className="node-switcher-title">{title}</span>
          <span className="node-switcher-sub">{subtitle}</span>
        </span>
      </button>
      {onForget ? (
        <button type="button" className="node-switcher-forget" aria-label={t("remote.forget", { name: title })} title={t("remote.forget", { name: title })} onClick={onForget}>
          <Icon name="close" size={12} />
        </button>
      ) : null}
    </div>
  );
}

export function PairDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = useI18n();
  const [address, setAddress] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [paired, setPaired] = useState<RemoteNode | null>(null);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const deviceName = (await deviceLabel()) || "Nebula Host";
      setPaired(await pair(address, code, deviceName));
    } catch (failure) {
      setError(toAppError(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={t("remote.pairTitle")}
      footer={
        paired ? (
          <>
            <Button onClick={onClose}>{t("remote.stayHere")}</Button>
            <Button variant="primary" onClick={() => switchTo(paired.id)}>
              {t("remote.openNode", { name: paired.name })}
            </Button>
          </>
        ) : (
          <>
            <Button onClick={onClose}>{t("actions.cancel")}</Button>
            <Button
              variant="primary"
              disabled={busy || !address.trim() || code.replace(/\D/g, "").length !== 6}
              onClick={() => void submit()}
            >
              {busy ? t("remote.pairing") : t("remote.pair")}
            </Button>
          </>
        )
      }
    >
      {paired ? (
        <p className="pair-done">
          <Icon name="check" size={14} /> {t("remote.paired", { name: paired.name })}
        </p>
      ) : (
        <form
          className="flex flex-col gap-4"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
        >
          <p className="pair-intro">{t("remote.pairIntro")}</p>
          <Field label={t("remote.address")} help={t("remote.addressHelp")}>
            <TextInput
              autoFocus
              placeholder="192.168.1.92"
              value={address}
              onChange={(event) => setAddress(event.target.value)}
              spellCheck={false}
            />
          </Field>
          <Field label={t("remote.code")} help={t("remote.codeHelp")}>
            <TextInput
              inputMode="numeric"
              placeholder="123 456"
              value={code}
              maxLength={7}
              onChange={(event) => setCode(event.target.value)}
              className="pair-code"
            />
          </Field>
          {error ? <ErrorNotice error={error} compact /> : null}
          <button type="submit" hidden />
        </form>
      )}
    </Dialog>
  );
}

/** A name for this desktop on the node's device list. */
async function deviceLabel(): Promise<string> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const node = await invoke<{ name: string }>("node_get");
    return node.name;
  } catch {
    return "";
  }
}
