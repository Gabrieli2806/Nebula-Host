/**
 * A server's identity tile: its icon when it has one (the game's own
 * server-list picture), otherwise its initial in the distribution's colour.
 *
 * Active (online / starting / stopping) and crashed servers show the full
 * identity colour; idle ones (offline / sleeping) are slightly desaturated,
 * and the initial stays readable. Identity is the only inline style:
 * `--identity`, alongside `data-identity`.
 */

import type { MouseEventHandler } from "react";

import type { InstanceStatus } from "@/types/api";
import { identityProps, initialOf } from "./identity";
import { isIdle } from "./StatusChip";

export function ServerTile({
  name,
  identity,
  status,
  lit,
  size = "md",
  onClick,
  label,
  image,
  className = "",
}: {
  name: string;
  /** The server's icon (`data:` URL); replaces the initial. */
  image?: string | null;
  /** The distribution accent (#rrggbb). Falls back to the app accent. */
  identity?: string | null;
  /** Drives full vs muted colour. Omit for a full-colour tile. */
  status?: InstanceStatus;
  /** Override: true = full colour, false = muted. */
  lit?: boolean;
  /** sm 28px (rail rows), md 40px (cards, rail identity), lg 48px (header). */
  size?: "sm" | "md" | "lg";
  /** Makes it a button. Without it the tile is a plain, aria-hidden span. */
  onClick?: MouseEventHandler<HTMLButtonElement>;
  /** Accessible name for the button form (e.g. "Open Friday night survival"). */
  label?: string;
  className?: string;
}) {
  const idle = lit === false || (lit === undefined && status !== undefined && isIdle(status));
  const shared = {
    ...identityProps(identity),
    "data-size": size,
    "data-idle": idle ? "" : undefined,
    "data-status": status,
    "data-image": image ? "" : undefined,
  };
  // The icon replaces the letter; the tile's frame and status tint stay.
  const face = image ? (
    <img src={image} alt="" className="server-tile-image" draggable={false} />
  ) : (
    initialOf(name)
  );

  if (onClick) {
    return (
      <button
        type="button"
        className={`server-tile ${className}`}
        onClick={onClick}
        aria-label={label ?? name}
        {...shared}
      >
        <span aria-hidden="true">{face}</span>
      </button>
    );
  }

  return (
    <span className={`server-tile ${className}`} aria-hidden="true" {...shared}>
      {face}
    </span>
  );
}
