/**
 * Launcher accounts: Microsoft sign-in (device code), the signed-in profile
 * with its face, and its skin (preview, upload, classic/slim).
 */

import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { openExternal } from "@/lib/external";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, ErrorNotice, Icon, Panel, PlayerHead, Segmented } from "@/ui";
import type { DeviceLogin, LauncherAccount } from "./types";

const ROOT = ["launcher"];

function Failure({ error }: { error: unknown }) {
  return error ? <ErrorNotice error={toAppError(error)} /> : null;
}

export function Accounts() {
  const { t } = useI18n();
  const client = useQueryClient();
  const accounts = useQuery({ queryKey: [...ROOT, "accounts"], queryFn: launcherApi.accounts, refetchInterval: 60_000 });
  const begin = useMutation({ mutationFn: () => launcherApi.begin() });
  const logout = useMutation({
    mutationFn: launcherApi.logout,
    onSuccess: () => client.invalidateQueries({ queryKey: [...ROOT, "accounts"] }),
  });
  const list = accounts.data ?? [];

  return (
    <div className="launcher-accounts">
      {list.map((account) => (
        <AccountCard key={account.id} account={account} onSignOut={() => logout.mutate(account.id)} busy={logout.isPending} />
      ))}

      <Panel title={list.length ? t("launcher.addAccount") : t("launcher.accounts")} bodyClassName="launcher-panel-body">
        {begin.data ? (
          <DeviceFlow login={begin.data} onClose={() => begin.reset()} />
        ) : (
          <div className="launcher-signin-intro">
            <p className="launcher-muted">{t("launcher.accountsNote")}</p>
            <Button variant="primary" icon="external" disabled={begin.isPending} onClick={() => begin.mutate()}>
              {t("launcher.signIn")}
            </Button>
          </div>
        )}
        <Failure error={begin.error || logout.error || accounts.error} />
      </Panel>
    </div>
  );
}

function AccountCard({ account, onSignOut, busy }: { account: LauncherAccount; onSignOut: () => void; busy: boolean }) {
  const { t } = useI18n();
  return (
    <Panel
      title={account.name}
      bodyClassName="launcher-panel-body"
      actions={<Button size="sm" variant="quiet" onClick={onSignOut} disabled={busy}>{t("launcher.signOut")}</Button>}
    >
      <div className="launcher-profile">
        <PlayerHead name={account.name} uuid={account.id} size={64} />
        <div className="launcher-profile-text">
          <strong>{account.name}</strong>
          <span className="launcher-muted"><Icon name="check" size={12} /> {t("launcher.verified")}</span>
        </div>
      </div>
      <SkinEditor account={account} />
    </Panel>
  );
}

/** Device-code sign-in as three plain steps; the code is copied for you. */
function DeviceFlow({ login, onClose }: { login: DeviceLogin; onClose: () => void }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const [copied, setCopied] = useState(false);
  const [openError, setOpenError] = useState(false);
  // Polled in Rust, so it finishes while the person is in the browser; on
  // Android the app comes back to the front when it does.
  const wait = useQuery({
    queryKey: [...ROOT, "login", login.id],
    queryFn: () => launcherApi.wait(login.id),
    retry: false,
    refetchOnWindowFocus: false,
    staleTime: Infinity,
  });
  const done = wait.data?.status === "complete" ? wait.data.account : null;
  useEffect(() => {
    if (!done) return;
    void client.invalidateQueries({ queryKey: [...ROOT, "accounts"] });
    const timer = window.setTimeout(onClose, 1800);
    return () => window.clearTimeout(timer);
  }, [done, client, onClose]);
  useEffect(() => () => { void launcherApi.cancel(login.id).catch(() => {}); }, [login.id]);

  const copy = () => navigator.clipboard.writeText(login.user_code).then(() => setCopied(true), () => {});
  const open = () => {
    void copy();
    void openExternal(login.verification_uri).then((ok) => setOpenError(!ok));
  };

  if (done) {
    return (
      <div className="launcher-device launcher-device-done" role="status">
        <PlayerHead name={done.name} uuid={done.id} size={48} />
        <p>{t("launcher.signedIn", { name: done.name })}</p>
      </div>
    );
  }

  return (
    <div className="launcher-device" role="status">
      <ol className="launcher-steps">
        <li>{t("launcher.stepCopy")}</li>
        <li>{t("launcher.stepOpen")}</li>
        <li>{t("launcher.stepBack")}</li>
      </ol>
      <div className="launcher-code-box">
        <code className="launcher-code selectable">{login.user_code}</code>
        <Button size="sm" variant="quiet" icon={copied ? "check" : "copy"} onClick={() => void copy()}>
          {copied ? t("common.copied") : t("common.copy")}
        </Button>
      </div>
      <div className="launcher-actions">
        <Button variant="primary" icon="external" onClick={open}>{t("launcher.openMicrosoft")}</Button>
        <Button variant="quiet" onClick={onClose}>{t("launcher.cancel")}</Button>
      </div>
      <p className="launcher-muted launcher-waiting"><span className="launcher-spinner" aria-hidden="true" />{t("launcher.waitingSignIn")}</p>
      {openError ? <p role="alert">{t("launcher.openFailed")}</p> : null}
      <Failure error={wait.error} />
    </div>
  );
}

function SkinEditor({ account }: { account: LauncherAccount }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const key = [...ROOT, "skin", account.id];
  const skin = useQuery({ queryKey: key, queryFn: () => launcherApi.skin(account.id), staleTime: 5 * 60_000 });
  const input = useRef<HTMLInputElement>(null);
  const [pending, setPending] = useState<{ data: string; preview: string } | null>(null);
  const [slim, setSlim] = useState<boolean | null>(null);
  const variant = slim ?? skin.data?.variant === "slim";
  const upload = useMutation({
    mutationFn: () => launcherApi.setSkin(account.id, pending!.data, variant),
    onSuccess: (data) => {
      client.setQueryData(key, data);
      setPending(null);
      setSlim(null);
    },
  });

  const pick = (file: File | undefined) => {
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      const url = String(reader.result);
      setPending({ data: url.slice(url.indexOf(",") + 1), preview: url });
    };
    reader.readAsDataURL(file);
  };

  const src = pending?.preview ?? skin.data?.url ?? null;
  return (
    <div className="launcher-skin">
      <SkinPreview src={src} slim={variant} />
      <div className="launcher-skin-side">
        <span className="label">{t("launcher.skin")}</span>
        <Segmented
          value={variant ? "slim" : "classic"}
          onChange={(value) => setSlim(value === "slim")}
          options={[
            { id: "classic", label: t("launcher.skinClassic") },
            { id: "slim", label: t("launcher.skinSlim") },
          ]}
          label={t("launcher.skinModel")}
        />
        <input ref={input} type="file" accept="image/png" hidden onChange={(e) => { pick(e.target.files?.[0]); e.target.value = ""; }} />
        <div className="launcher-actions">
          <Button icon="upload" onClick={() => input.current?.click()}>{t("launcher.chooseSkin")}</Button>
          {pending || slim !== null ? (
            <Button variant="primary" icon="check" disabled={upload.isPending || (!pending && !skin.data?.url)} onClick={() => upload.mutate()}>
              {t("launcher.applySkin")}
            </Button>
          ) : null}
        </div>
        <p className="launcher-muted">{t("launcher.skinHelp")}</p>
        <Failure error={skin.error || upload.error} />
      </div>
    </div>
  );
}

/**
 * Front view of a skin, drawn from the 64x64 texture: head, body, arms and
 * legs with their overlay layers, in hard pixels.
 */
function SkinPreview({ src, slim }: { src: string | null; slim: boolean }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const target = canvas.current?.getContext("2d");
    if (!target) return;
    target.clearRect(0, 0, 16, 32);
    if (!src) return;
    const image = new Image();
    image.crossOrigin = "anonymous";
    image.onload = () => {
      const legacy = image.height === 32;
      const arm = slim ? 3 : 4;
      const part = (sx: number, sy: number, w: number, h: number, dx: number, dy: number) =>
        target.drawImage(image, sx, sy, w, h, dx, dy, w, h);
      target.imageSmoothingEnabled = false;
      target.clearRect(0, 0, 16, 32);
      // Base layer.
      part(8, 8, 8, 8, 4, 0); // head
      part(20, 20, 8, 12, 4, 8); // body
      part(44, 20, arm, 12, 4 - arm, 8); // right arm
      if (legacy) part(44, 20, arm, 12, 12, 8); else part(36, 52, arm, 12, 12, 8); // left arm
      part(4, 20, 4, 12, 4, 20); // right leg
      if (legacy) part(4, 20, 4, 12, 8, 20); else part(20, 52, 4, 12, 8, 20); // left leg
      // Overlay layer.
      part(40, 8, 8, 8, 4, 0);
      if (!legacy) {
        part(20, 36, 8, 12, 4, 8);
        part(44, 36, arm, 12, 4 - arm, 8);
        part(52, 52, arm, 12, 12, 8);
        part(4, 36, 4, 12, 4, 20);
        part(4, 52, 4, 12, 8, 20);
      }
    };
    image.src = src;
  }, [src, slim]);
  return <canvas ref={canvas} className="launcher-skin-preview" width={16} height={32} aria-hidden="true" />;
}
