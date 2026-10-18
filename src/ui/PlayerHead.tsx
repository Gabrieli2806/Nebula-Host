/**
 * A Minecraft player's face: the head layer plus the hat overlay, rendered
 * with hard pixels.
 *
 * The initial tile is always underneath; the face fades in over it once it
 * has loaded. So an offline machine, a blocked CDN or a name that has no
 * account still reads as a person, and nothing jumps when the image arrives.
 *
 * Source: mc-heads.net (HTTPS; the CSP allows `img-src https:`), which serves
 * head + hat already composited. Looked up by UUID when we have a real
 * account UUID, by name otherwise — offline-mode servers hand out
 * name-derived (version 3) UUIDs that map to no skin.
 */

import { useState } from "react";

import { initialOf } from "./identity";
import { playerHeadUrl, type PlayerHeadSize } from "./playerHeadUrl";
import "./PlayerHead.css";

/** URLs that already failed this session, so lists do not retry each render. */
const failed = new Set<string>();

export function PlayerHead({
  name,
  uuid,
  size = 32,
  label,
  className = "",
}: {
  name: string;
  /** Account UUID (dashed or not). Optional; v3 offline UUIDs are ignored. */
  uuid?: string | null;
  size?: PlayerHeadSize;
  /**
   * Accessible name. Omit when the player's name is printed next to the head
   * (the head is then decorative and hidden from assistive tech); pass it
   * when the head stands alone, e.g. `label={name}`.
   */
  label?: string;
  className?: string;
}) {
  const url = playerHeadUrl(name, uuid, size);
  const [loadedUrl, setLoadedUrl] = useState<string | null>(null);
  const [, setBroken] = useState(0);
  const showImage = url !== null && !failed.has(url);

  const a11y = label
    ? ({ role: "img", "aria-label": label } as const)
    : ({ "aria-hidden": true } as const);

  return (
    <span className={`player-head ${className}`} data-size={size} {...a11y}>
      <span className="player-head-initial">{initialOf(name)}</span>
      {showImage ? (
        <img
          key={url}
          src={url}
          alt=""
          width={size}
          height={size}
          loading="lazy"
          decoding="async"
          draggable={false}
          referrerPolicy="no-referrer"
          data-loaded={loadedUrl === url ? "" : undefined}
          onLoad={() => setLoadedUrl(url)}
          onError={() => {
            failed.add(url);
            setBroken((n) => n + 1);
          }}
        />
      ) : null}
    </span>
  );
}
