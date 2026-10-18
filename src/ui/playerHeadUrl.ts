/**
 * Where a player's face comes from. See PlayerHead.tsx for the why.
 */

export type PlayerHeadSize = 16 | 24 | 32 | 48 | 64;

/** Valid Java Edition account names; anything else has no face to fetch. */
const NAME = /^[A-Za-z0-9_]{1,16}$/;

/** Version-3 (name-hashed, offline-mode) UUIDs carry `3` as the version nibble. */
export function isOfflineUuid(uuid: string): boolean {
  const hex = uuid.replace(/[^0-9a-fA-F]/g, "");
  return hex.length === 32 && hex[12] === "3";
}

/** The face URL for a player, or null when there is nothing to look up. */
export function playerHeadUrl(name: string, uuid?: string | null, size: PlayerHeadSize = 32): string | null {
  // Twice the box for HiDPI; the service upsamples with nearest-neighbour.
  const px = Math.min(128, Math.max(32, size * 2));
  const hex = uuid?.replace(/-/g, "");
  if (hex && /^[0-9a-fA-F]{32}$/.test(hex) && !isOfflineUuid(hex)) {
    return `https://mc-heads.net/avatar/${hex.toLowerCase()}/${px}`;
  }
  if (NAME.test(name)) return `https://mc-heads.net/avatar/${encodeURIComponent(name)}/${px}`;
  return null;
}
