/**
 * A distribution's glyph, and the tag that pairs it with its name.
 *
 * Which glyph a distribution gets is data: the game manifest's
 * `distributions[].glyph`. The UI never decides it from a distribution id,
 * so a second game brings its own choices without a change here.
 *
 * The glyph is decoration next to a visible name, so it is always
 * `aria-hidden`; the name is what assistive technology reads.
 */

import type { Distribution } from "@/types/api";

import { identityProps } from "./identity";
import { GLYPHS, isLoaderGlyph } from "./loaders/glyphs";

export type { LoaderGlyph } from "./loaders/glyphs";

export interface LoaderIconProps {
  /** The manifest's glyph id. Unknown or missing draws the neutral cube. */
  glyph?: string | null;
  size?: number;
  className?: string;
}

export function LoaderIcon({ glyph, size = 14, className = "" }: LoaderIconProps) {
  const shape = GLYPHS[isLoaderGlyph(glyph) ? glyph : "cube"];
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      className={`loader-icon ${className}`}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.3}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {shape}
    </svg>
  );
}

/**
 * The distribution tag used wherever a server's flavour is named: glyph plus
 * name, in the distribution's identity colour.
 */
export function DistributionTag({
  distribution,
  fallback,
  className = "",
  title,
}: {
  distribution?: Pick<Distribution, "name" | "accent" | "glyph"> | null;
  /** Shown when the manifest does not know the distribution (the raw id). */
  fallback: string;
  className?: string;
  title?: string;
}) {
  return (
    <span className={`tag tag-distribution ${className}`} title={title} {...identityProps(distribution?.accent)}>
      <LoaderIcon glyph={distribution?.glyph} size={12} />
      {distribution?.name ?? fallback}
    </span>
  );
}
