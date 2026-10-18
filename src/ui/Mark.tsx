/**
 * The Nebula mark: a core and two inclined orbits, in the accent.
 *
 * The original orbit logo, redrawn on a 24px grid. Strokes do not scale with
 * the icon (vector-effect), so the orbits stay a crisp hairline in the title
 * bar and do not turn heavy at first-run size. Colours come from CSS
 * (`.mark-*` in global.css), so the mark follows the accent and the theme.
 */

export function Mark({
  size = 18,
  label,
  className = "",
}: {
  size?: number;
  /** Accessible name; omit when the wordmark sits next to it. */
  label?: string;
  className?: string;
}) {
  // Screen pixels: a hairline in the title bar, a little firmer when large.
  const stroke = size <= 20 ? 1.2 : size <= 36 ? 1.4 : 1.75;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      className={`mark ${className}`}
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      focusable="false"
    >
      <ellipse
        className="mark-orbit mark-orbit-outer"
        cx="12"
        cy="12"
        rx="10.4"
        ry="4.6"
        transform="rotate(-22 12 12)"
        strokeWidth={stroke}
        vectorEffect="non-scaling-stroke"
      />
      <ellipse
        className="mark-orbit mark-orbit-inner"
        cx="12"
        cy="12"
        rx="6.8"
        ry="10"
        transform="rotate(14 12 12)"
        strokeWidth={stroke}
        vectorEffect="non-scaling-stroke"
      />
      <circle className="mark-core" cx="12" cy="12" r="3" />
    </svg>
  );
}
