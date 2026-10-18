/**
 * The web panel's sign-in: a browser pairs with the machine that served it
 * using the same 6-digit code as any other device, then keeps its token.
 */

import { useEffect, useState } from "react";

import { TitleBar } from "@/app/TitleBar";
import { useI18n } from "@/i18n";
import { pairWeb, probe, type NodeInfo } from "@/lib/connection";
import { webApiHost } from "@/lib/web";
import { toAppError } from "@/lib/ipc";
import { Button, ErrorNotice, Field, Icon, Mark, TextInput } from "@/ui";

import "./WebPair.css";

/** "Chrome on Android", "Safari on iPhone"… as a default device name. */
function guessDeviceName(fallback: string): string {
  const ua = navigator.userAgent;
  const browser = /Edg\//.test(ua)
    ? "Edge"
    : /Firefox\//.test(ua)
      ? "Firefox"
      : /Chrome\//.test(ua)
        ? "Chrome"
        : /Safari\//.test(ua)
          ? "Safari"
          : fallback;
  const os = /iPhone/.test(ua)
    ? "iPhone"
    : /iPad/.test(ua)
      ? "iPad"
      : /Android/.test(ua)
        ? "Android"
        : /Windows/.test(ua)
          ? "Windows"
          : /Mac OS/.test(ua)
            ? "Mac"
            : /Linux/.test(ua)
              ? "Linux"
              : "";
  return os ? `${browser} · ${os}` : browser;
}

export function WebPair() {
  const { t } = useI18n();
  const [info, setInfo] = useState<NodeInfo | null>(null);
  const [code, setCode] = useState("");
  const [deviceName, setDeviceName] = useState(() => guessDeviceName(t("web.deviceNameDefault")));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    probe(webApiHost()).then(setInfo, setError);
  }, []);

  const ready = code.replace(/\D/g, "").length === 6 && deviceName.trim() !== "";

  const submit = async () => {
    if (!ready) return;
    setBusy(true);
    setError(null);
    try {
      await pairWeb(code, deviceName.trim());
      window.location.reload();
    } catch (failure) {
      setError(toAppError(failure));
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <TitleBar />
      <main className="web-pair">
        <form
          className="web-pair-card panel"
          onSubmit={(event) => {
            event.preventDefault();
            void submit();
          }}
        >
          <Mark size={36} />
          <h1 className="web-pair-title">
            {info ? t("web.pairTitle", { name: info.nodeName }) : t("web.pairTitleGeneric")}
          </h1>
          <p className="web-pair-intro">{t("web.pairIntro")}</p>

          <Field label={t("remote.code")} help={t("remote.codeHelp")}>
            <TextInput
              autoFocus
              inputMode="numeric"
              autoComplete="one-time-code"
              placeholder="123 456"
              value={code}
              maxLength={7}
              onChange={(event) => setCode(event.target.value)}
              className="pair-code"
            />
          </Field>
          <Field label={t("web.deviceName")}>
            <TextInput value={deviceName} maxLength={60} onChange={(event) => setDeviceName(event.target.value)} />
          </Field>

          {error ? <ErrorNotice error={error} compact /> : null}

          <Button type="submit" variant="primary" disabled={busy || !ready}>
            {busy ? t("web.signingIn") : t("web.signIn")}
          </Button>

          <p className="web-pair-note">
            <Icon name="warning" size={12} />
            <span>{t("web.lanOnly")}</span>
          </p>
        </form>
      </main>
    </div>
  );
}
