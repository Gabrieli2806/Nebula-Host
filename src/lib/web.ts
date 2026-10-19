/**
 * The web panel: this same interface served to a browser on the LAN by the
 * machine's remote API (Settings › Remote access › Web panel).
 *
 * In a browser there is no Tauri backend, so the window always manages the
 * machine that served the page, over its HTTP API, with the token the
 * browser got from pairing. The token lives in this browser's localStorage
 * and travels in a header — never a cookie — so another site cannot use it.
 */

/** True in a browser, false inside the desktop app. */
export const IS_WEB = typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

/** The Android (or iOS) app: the same backend as the desktop, a phone shell
 *  (no window keys, a status bar above the page, touch). */
export const IS_MOBILE =
  typeof navigator !== "undefined" && !IS_WEB && /Android|iPhone|iPad/i.test(navigator.userAgent);

export interface WebSession {
  token: string;
  deviceId: string;
  name: string;
  nodeId?: string;
  platform?: string;
}

const SESSION_KEY = "nebula.webSession";
const PREFS_KEY = "nebula.webPrefs";

function read<T>(key: string): T | null {
  try {
    const raw = window.localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : null;
  } catch {
    return null;
  }
}

function write(key: string, value: unknown) {
  try {
    if (value === null) window.localStorage.removeItem(key);
    else window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage blocked: the session lasts until the tab closes.
  }
}

export function webSession(): WebSession | null {
  return IS_WEB ? read<WebSession>(SESSION_KEY) : null;
}

export function setWebSession(session: WebSession) {
  write(SESSION_KEY, session);
}

/** Sign this browser out (the machine may also have removed it). */
export function signOutWeb() {
  write(SESSION_KEY, null);
  window.location.hash = "";
  window.location.reload();
}

/** How this browser looks (language, accent, density): the browser's own,
 *  like the desktop keeps its own when managing another machine. */
export function webPrefs(): Record<string, unknown> {
  return read<Record<string, unknown>>(PREFS_KEY) ?? {};
}

export function setWebPrefs(prefs: Record<string, unknown>) {
  write(PREFS_KEY, prefs);
}

/** The machine's API: the page's own origin, or in development (`npm run
 *  dev` in a browser) the one named by VITE_NEBULA_HOST. */
export function webApiHost(): string {
  return (import.meta.env.VITE_NEBULA_HOST as string | undefined) || window.location.host;
}
