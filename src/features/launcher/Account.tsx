/**
 * Launcher accounts: Microsoft sign-in (device code), the signed-in profile
 * with its face, and its skin (preview, upload, classic/slim).
 */

import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { useI18n } from "@/i18n";
import { openExternal } from "@/lib/external";
import { launcherApi, toAppError } from "@/lib/ipc";
import { Button, Dialog, ErrorNotice, Icon, Panel, PlayerHead, Segmented } from "@/ui";
import { SkinViewer } from "skinview3d";
import type { DeviceLogin, LauncherAccount } from "./types";
import { setPrimaryAccount, usePrimaryAccount } from "./primaryAccount";

const ROOT = ["launcher"];

function Failure({ error }: { error: unknown }) {
  return error ? <ErrorNotice error={toAppError(error)} /> : null;
}

/**
 * Top right of the launcher: the primary account's face and name, and a menu
 * to switch account, add one, or sign out.
 */
export function AccountMenu({ placement = "down" }: { placement?: "down" | "up" }) {
  const { t } = useI18n();
  const client = useQueryClient();
  const { accounts, primary } = usePrimaryAccount();
  const [open, setOpen] = useState(false);
  const [adding, setAdding] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const logout = useMutation({
    mutationFn: launcherApi.logout,
    onSuccess: () => client.invalidateQueries({ queryKey: [...ROOT, "accounts"] }),
  });

  useEffect(() => {
    if (!open) return;
    const close = (event: PointerEvent) => {
      if (!box.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => event.key === "Escape" && setOpen(false);
    document.addEventListener("pointerdown", close);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", close);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  return (
    <div className="launcher-account-menu" data-placement={placement} ref={box}>
      {primary ? (
        <button type="button" className="launcher-account-button" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen(!open)}>
          <PlayerHead name={primary.name} uuid={primary.id} size={24} />
          <span className="launcher-account-name">{primary.name}</span>
          <Icon name="chevron-down" size={12} />
        </button>
      ) : (
        <Button variant="primary" icon="external" onClick={() => setAdding(true)}>{t("launcher.signIn")}</Button>
      )}
      {open ? (
        <div className="launcher-account-popover" role="menu">
          {accounts.map((account) => (
            <div key={account.id} className="launcher-account-item">
              <button
                type="button"
                role="menuitemradio"
                aria-checked={account.id === primary?.id}
                className="launcher-account-pick"
                onClick={() => { setPrimaryAccount(account.id); setOpen(false); }}
              >
                <PlayerHead name={account.name} uuid={account.id} size={24} />
                <span className="launcher-account-name">{account.name}</span>
                {account.id === primary?.id ? <Icon name="check" size={13} /> : null}
              </button>
              <Button size="sm" variant="quiet" iconOnly icon="close" disabled={logout.isPending} onClick={() => logout.mutate(account.id)}>
                {t("launcher.signOutOf", { name: account.name })}
              </Button>
            </div>
          ))}
          <button type="button" role="menuitem" className="launcher-account-pick" onClick={() => { setOpen(false); setAdding(true); }}>
            <span className="launcher-account-add"><Icon name="plus" size={14} /></span>
            <span>{t("launcher.addAccount")}</span>
          </button>
          <Failure error={logout.error} />
        </div>
      ) : null}
      {adding ? <SignInDialog onClose={() => setAdding(false)} /> : null}
    </div>
  );
}

/** Microsoft sign-in in a dialog; the code is requested as it opens. */
function SignInDialog({ onClose }: { onClose: () => void }) {
  const { t } = useI18n();
  const begin = useMutation({ mutationFn: () => launcherApi.begin() });
  const { mutate } = begin;
  useEffect(() => { mutate(); }, [mutate]);
  return (
    <Dialog open onClose={onClose} title={t("launcher.signIn")} width={520}>
      <div className="flex flex-col gap-3">
        <p className="launcher-muted">{t("launcher.accountsNote")}</p>
        {begin.data ? <DeviceFlow login={begin.data} onClose={onClose} /> : begin.isPending ? <p role="status" className="launcher-muted">{t("launcher.loading")}</p> : null}
        <Failure error={begin.error} />
      </div>
    </Dialog>
  );
}

/** The Skin tab: the primary account's skin and cape. */
export function SkinTab() {
  const { t } = useI18n();
  const { primary, loading } = usePrimaryAccount();
  const [adding, setAdding] = useState(false);
  if (loading) return <p role="status" className="launcher-muted">{t("launcher.loading")}</p>;
  if (!primary) {
    return (
      <div className="panel servers-empty"><div className="empty-state">
        <span className="empty-state-icon"><Icon name="users" size={20} /></span>
        <p className="empty-state-title">{t("launcher.skinSignIn")}</p>
        <p className="empty-state-hint">{t("launcher.accountsNote")}</p>
        <div className="mt-1"><Button variant="primary" icon="external" onClick={() => setAdding(true)}>{t("launcher.signIn")}</Button></div>
        {adding ? <SignInDialog onClose={() => setAdding(false)} /> : null}
      </div></div>
    );
  }
  return (
    <Panel bodyClassName="launcher-panel-body" key={primary.id}>
      <div className="launcher-profile">
        <PlayerHead name={primary.name} uuid={primary.id} size={48} />
        <div className="launcher-profile-text">
          <strong>{primary.name}</strong>
          <span className="launcher-muted"><Icon name="check" size={12} /> {t("launcher.verified")}</span>
        </div>
      </div>
      <SkinEditor account={primary} />
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

  const cape = useMutation({
    mutationFn: (capeId: string | null) => launcherApi.setCape(account.id, capeId),
    onSuccess: (data) => client.setQueryData(key, data),
    // Re-read what Minecraft actually has, so the grid never shows a wrong pick.
    onError: () => void client.invalidateQueries({ queryKey: key }),
  });
  const capes = skin.data?.capes ?? [];
  const activeCape = capes.find((c) => c.active) ?? null;
  const [choosing, setChoosing] = useState(false);

  const src = pending?.preview ?? skin.data?.url ?? null;
  return (
    <div className="launcher-skin">
      <SkinPreview src={src} slim={variant} cape={activeCape?.url ?? null} />
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
        <span className="label">{t("launcher.cape")}</span>
        <div className="launcher-cape-current">
          {activeCape ? <CapeFront url={activeCape.url} /> : <span className="launcher-cape-none"><Icon name="close" size={16} /></span>}
          <span>{activeCape?.alias ?? (capes.length ? t("launcher.noCape") : t("launcher.noCapesOwned"))}</span>
          {capes.length ? <Button size="sm" onClick={() => setChoosing(true)}>{t("launcher.changeCape")}</Button> : null}
        </div>
        {choosing ? (
          <CapeDialog
            capes={capes}
            current={activeCape?.id ?? null}
            busy={cape.isPending}
            onClose={() => setChoosing(false)}
            onApply={(id) => cape.mutate(id, { onSuccess: () => setChoosing(false) })}
            error={cape.error}
          />
        ) : null}
        <Failure error={skin.error || upload.error || cape.error} />
      </div>
    </div>
  );
}

/** Pick a cape, then apply: nothing is sent to Minecraft until then. */
function CapeDialog({ capes, current, busy, onClose, onApply, error }: {
  capes: { id: string; alias: string; url: string }[];
  current: string | null;
  busy: boolean;
  onClose: () => void;
  onApply: (id: string | null) => void;
  error: unknown;
}) {
  const { t } = useI18n();
  const [picked, setPicked] = useState<string | null>(current);
  return (
    <Dialog
      open
      onClose={onClose}
      dismissable={!busy}
      title={t("launcher.chooseCape")}
      width={560}
      footer={<>
        <Button variant="quiet" disabled={busy} onClick={onClose}>{t("launcher.cancel")}</Button>
        <Button variant="primary" icon="check" disabled={busy || picked === current} onClick={() => onApply(picked)}>{t("launcher.applySkin")}</Button>
      </>}
    >
      <div className="launcher-capes" role="radiogroup" aria-label={t("launcher.cape")}>
        <button type="button" role="radio" aria-checked={picked === null} className="launcher-cape" onClick={() => setPicked(null)}>
          <span className="launcher-cape-none"><Icon name="close" size={16} /></span>
          <span className="launcher-cape-name">{t("launcher.noCape")}</span>
        </button>
        {capes.map((c) => (
          <button key={c.id} type="button" role="radio" aria-checked={picked === c.id} className="launcher-cape" onClick={() => setPicked(c.id)}>
            <CapeFront url={c.url} />
            <span className="launcher-cape-name">{c.alias}</span>
          </button>
        ))}
      </div>
      <Failure error={error} />
    </Dialog>
  );
}

/** The front of a cape, cut from its 64x32 texture (10x16 at 1,1). */
function CapeFront({ url }: { url: string }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const target = canvas.current?.getContext("2d");
    if (!target) return;
    const image = new Image();
    image.crossOrigin = "anonymous";
    image.onload = () => {
      const scale = image.width / 64;
      target.imageSmoothingEnabled = false;
      target.clearRect(0, 0, 10, 16);
      target.drawImage(image, 1 * scale, 1 * scale, 10 * scale, 16 * scale, 0, 0, 10, 16);
    };
    image.src = url;
  }, [url]);
  return <canvas ref={canvas} className="launcher-cape-art" width={10} height={16} aria-hidden="true" />;
}

/**
 * The skin as a 3D model (skinview3d): it turns slowly and can be dragged
 * around; the cape is worn when one is active.
 */
function SkinPreview({ src, slim, cape }: { src: string | null; slim: boolean; cape: string | null }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const viewer = useRef<SkinViewer | null>(null);

  useEffect(() => {
    if (!canvas.current) return;
    const instance = new SkinViewer({ canvas: canvas.current, width: 180, height: 260 });
    instance.zoom = 0.85;
    instance.autoRotate = true;
    instance.autoRotateSpeed = 0.6;
    instance.controls.enableZoom = false;
    viewer.current = instance;
    return () => {
      instance.dispose();
      viewer.current = null;
    };
  }, []);

  useEffect(() => {
    const instance = viewer.current;
    if (!instance) return;
    if (src) void instance.loadSkin(src, { model: slim ? "slim" : "default" });
    else instance.resetSkin();
  }, [src, slim]);

  useEffect(() => {
    const instance = viewer.current;
    if (!instance) return;
    if (cape) void instance.loadCape(cape);
    else instance.resetCape();
  }, [cape]);

  return <canvas ref={canvas} className="launcher-skin-preview" aria-hidden="true" />;
}
