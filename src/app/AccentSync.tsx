/**
 * Puts the chosen accent on the document, where the tokens can see it.
 *
 * One attribute — `data-accent` on `<html>` — swings every accent-derived
 * variable at once, because the whole palette is computed from a single base
 * colour in `tokens.css`.
 */

import { useEffect } from "react";

import { useAppSettings } from "@/hooks/useBackend";

export const ACCENTS = ["cyan", "amber", "emerald", "violet", "blue", "rose"] as const;
export type Accent = (typeof ACCENTS)[number];

export function isAccent(value: string): value is Accent {
  return (ACCENTS as readonly string[]).includes(value);
}

export function AccentSync() {
  const { data: settings } = useAppSettings();
  const accent = settings?.accent && isAccent(settings.accent) ? settings.accent : "cyan";

  useEffect(() => {
    document.documentElement.dataset.accent = accent;
  }, [accent]);

  return null;
}
