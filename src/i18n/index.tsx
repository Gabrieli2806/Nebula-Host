/**
 * Translation, deliberately small.
 *
 * Keys are dotted paths into the copy object, values interpolate `{name}`
 * placeholders. Errors get their own helper because they are the one place
 * where a missing translation would leak backend English into the UI.
 */

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

import { IS_MOBILE } from "@/lib/web";

import { en } from "./en";
import { es } from "./es";
import type { AppError } from "@/lib/ipc";

export type Language = "en" | "es";

const CATALOGUES = { en, es } as const;

type Params = Record<string, string | number>;

/**
 * Walk a dotted key. A level may also hold a literal dotted property (the
 * game manifests use `"seed.help"` next to `seed`), so the rest of the key is
 * tried whole before descending another level.
 */
function resolve(catalogue: unknown, key: string): unknown {
  const parts = key.split(".");
  let node = catalogue;
  for (const [i, part] of parts.entries()) {
    if (!node || typeof node !== "object") return undefined;
    const level = node as Record<string, unknown>;
    if (i < parts.length - 1) {
      const rest = parts.slice(i).join(".");
      if (rest in level) return level[rest];
    }
    if (!(part in level)) return undefined;
    node = level[part];
  }
  return node;
}

function interpolate(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

export interface Translator {
  language: Language;
  setLanguage: (language: Language | "system") => void;
  /** Translate a dotted key. Falls back to English, then to the key itself. */
  t: (key: string, params?: Params) => string;
  /** Title, cause and suggested action for a backend error. */
  describeError: (error: AppError) => { title: string; cause: string; action: string; detail?: string };
}

const TranslatorContext = createContext<Translator | null>(null);

function systemLanguage(): Language {
  return navigator.language.toLowerCase().startsWith("es") ? "es" : "en";
}

/** Keys whose wording names the device: swapped for the phone's wording. */
const PHONE_WORDING: Record<string, string> = {
  "remote.thisPc": "phone.thisPhone",
  "transfer.thisPc": "phone.thisPhone",
  "node.thisMachine": "phone.thisPhone",
};

export function I18nProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<Language>(() => {
    // Development only: `?lang=es|en` pins the language for previews.
    if (import.meta.env.DEV) {
      const flag = new URLSearchParams(window.location.search).get("lang");
      if (flag === "es" || flag === "en") return flag;
    }
    const stored = readStoredLanguage();
    return stored === "system" || stored === null ? systemLanguage() : stored;
  });

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  const setLanguage = useCallback((next: Language | "system") => {
    try {
      localStorage.setItem("nebula.language", next);
    } catch {
      // A browser with storage blocked still gets a working app for this run.
    }
    setLanguageState(next === "system" ? systemLanguage() : next);
  }, []);

  const t = useCallback(
    (key: string, params?: Params) => {
      // On the phone app, "This PC" reads "This phone".
      if (IS_MOBILE) key = PHONE_WORDING[key] ?? key;
      const value = resolve(CATALOGUES[language], key) ?? resolve(en, key);
      return typeof value === "string" ? interpolate(value, params) : key;
    },
    [language],
  );

  const describeError = useCallback(
    (error: AppError) => {
      // A `reason` param can name a more precise entry (`io_failed_in_use`).
      const reason = error.params?.reason;
      const precise = reason ? `errors.${error.code}_${reason}` : null;
      const base = `errors.${error.code}`;
      const known =
        (precise ? resolve(CATALOGUES[language], precise) ?? resolve(en, precise) : undefined) ??
        resolve(CATALOGUES[language], base) ??
        resolve(en, base);
      const source = (known ?? resolve(en, "errors.unexpected")) as {
        title: string;
        cause: string;
        action: string;
      };

      return {
        title: interpolate(source.title, error.params),
        cause: interpolate(source.cause, error.params),
        action: interpolate(source.action, error.params),
        detail: error.detail,
      };
    },
    [language],
  );

  const value = useMemo<Translator>(
    () => ({ language, setLanguage, t, describeError }),
    [language, setLanguage, t, describeError],
  );

  return <TranslatorContext.Provider value={value}>{children}</TranslatorContext.Provider>;
}

function readStoredLanguage(): Language | "system" | null {
  try {
    const value = localStorage.getItem("nebula.language");
    return value === "en" || value === "es" || value === "system" ? value : null;
  } catch {
    return null;
  }
}

export function useI18n(): Translator {
  const value = useContext(TranslatorContext);
  if (!value) throw new Error("useI18n must be used inside <I18nProvider>");
  return value;
}
