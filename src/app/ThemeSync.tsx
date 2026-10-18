/**
 * Puts the effective theme on the document: `<html data-theme="dark|light">`.
 *
 * The preference (System / Dark / Light) lives in `./theme.ts`; this only
 * keeps the attribute in step with it and with the OS when on "system".
 */

import { useLayoutEffect } from "react";

import { useEffectiveTheme } from "./theme";

export function ThemeSync() {
  const theme = useEffectiveTheme();

  useLayoutEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  return null;
}
