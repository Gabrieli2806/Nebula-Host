/**
 * Per-server identity (the distribution accent) as attributes.
 *
 * Contract (tokens.css): an element carries BOTH `data-identity` and the
 * inline `--identity: #rrggbb`; CSS derives --identity-ink / -dim / -line /
 * -ash on that element. Never concatenate hex alpha onto the colour.
 */

import type { CSSProperties } from "react";

/** First letter of a name, safe for surrogate pairs. */
export function initialOf(name: string): string {
  const first = Array.from(name.trim())[0];
  return first ? first.toLocaleUpperCase() : "?";
}

/**
 * The attributes any identity-carrying element needs (cards, tiles,
 * .tag, rows). Spread onto the element:
 *   <article className="plate relative" {...identityProps(accent)}>
 * `accent` null/undefined falls back to the app accent in CSS.
 */
export function identityProps(accent: string | null | undefined): {
  "data-identity": string;
  style?: CSSProperties;
} {
  return accent
    ? { "data-identity": "", style: { "--identity": accent } as CSSProperties }
    : { "data-identity": "" };
}
