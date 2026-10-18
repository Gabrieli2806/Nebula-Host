import { useState } from "react";

import { Icon } from "@/ui";

/** The addon's own icon, or a blank tile with a puzzle piece. */
export function AddonIcon({ url, size }: { url?: string | null; size: number }) {
  const [broken, setBroken] = useState(false);
  if (url && !broken) {
    return (
      <img
        src={url}
        alt=""
        width={size}
        height={size}
        loading="lazy"
        className="addon-icon"
        onError={() => setBroken(true)}
      />
    );
  }
  return (
    <span className="addon-icon addon-icon-blank" style={{ width: size, height: size }} aria-hidden="true">
      <Icon name="puzzle" size={size > 36 ? 20 : 16} />
    </span>
  );
}
