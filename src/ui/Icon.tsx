/**
 * The icon set.
 *
 * Drawn on one 16px grid with one stroke weight, so they sit together instead
 * of looking borrowed from three different libraries. Adding one means drawing
 * it here, not adding a dependency.
 */

export type IconName =
  | "servers"
  | "play"
  | "stop"
  | "restart"
  | "moon"
  | "folder"
  | "file"
  | "archive"
  | "users"
  | "puzzle"
  | "sliders"
  | "terminal"
  | "globe"
  | "plus"
  | "link"
  | "search"
  | "refresh"
  | "warning"
  | "check"
  | "close"
  | "minimize"
  | "maximize"
  | "chevron-right"
  | "chevron-down"
  | "chevron-left"
  | "trash"
  | "external"
  | "cpu"
  | "clock"
  | "restore"
  | "history"
  | "flame"
  | "copy"
  | "send"
  | "sun"
  | "monitor"
  | "arrow-down"
  | "arrow-up"
  | "upload"
  | "download"
  | "activity"
  | "info"
  | "chat"
  | "power"
  | "image"
  | "coffee";

const PATHS: Record<IconName, string> = {
  servers: "M2.5 3.5h11v4h-11zM2.5 8.5h11v4h-11zM4.6 5.5h.01M4.6 10.5h.01",
  play: "M5 3.4 12.6 8 5 12.6z",
  stop: "M4.5 4.5h7v7h-7z",
  restart: "M13 8a5 5 0 1 1-1.6-3.7M13 2.2V5h-2.8",
  moon: "M13 9.4A5.3 5.3 0 0 1 6.6 3 5.4 5.4 0 1 0 13 9.4z",
  folder: "M2.5 12.5v-9h4l1.4 2h5.6v7z",
  file: "M4 2.5h5l3 3v8H4zM9 2.5v3h3",
  archive: "M2.5 4.2h11v2.6h-11zM3.6 6.8h8.8v6.4H3.6zM6.4 9.2h3.2",
  users: "M6 8.2a2.3 2.3 0 1 0 0-4.6 2.3 2.3 0 0 0 0 4.6zM1.8 13.2c0-2.2 1.9-3.4 4.2-3.4s4.2 1.2 4.2 3.4M11 4a2.2 2.2 0 0 1 0 4.3M12.2 9.9c1.3.4 2 1.4 2 3.3",
  puzzle:
    "M6.2 2.5h3.6v1.7a1.2 1.2 0 1 0 2.4 0v-.2h1.3v3.6h-1.7a1.2 1.2 0 1 0 0 2.4h1.7v3.5H9.8v-1.7a1.2 1.2 0 1 0-2.4 0v1.7H3.9V10h1.7a1.2 1.2 0 1 0 0-2.4H3.9V4h2.3z",
  sliders: "M2.5 5h7M11.5 5h2M2.5 11h2M6.5 11h7M10.5 3.4v3.2M5.5 9.4v3.2",
  terminal: "M3.4 4.4 6.6 8l-3.2 3.6M8 11.6h4.6",
  globe: "M8 2.2a5.8 5.8 0 1 0 0 11.6A5.8 5.8 0 0 0 8 2.2zM2.4 8h11.2M8 2.2c1.5 1.6 2.3 3.6 2.3 5.8S9.5 12.2 8 13.8C6.5 12.2 5.7 10.2 5.7 8S6.5 3.8 8 2.2z",
  plus: "M8 3.5v9M3.5 8h9",
  activity: "M2 8.5h2.6l1.6-4 3 8 1.6-4H14",
  download: "M8 3v7.5M4.5 7.5L8 11l3.5-3.5M3.5 13h9",
  info: "M8 14.5a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM8 7.2v4M8 5h.01",
  chat: "M3 12.5V4.5a1.5 1.5 0 0 1 1.5-1.5h7A1.5 1.5 0 0 1 13 4.5v5a1.5 1.5 0 0 1-1.5 1.5H5.5L3 12.5zM5.8 6.2h4.4M5.8 8.3h2.8",
  link: "M6.8 9.2l2.4-2.4M7.3 4.9l.9-.9a2.6 2.6 0 0 1 3.7 3.7l-.9.9M8.7 11.1l-.9.9a2.6 2.6 0 0 1-3.7-3.7l.9-.9",
  search: "M7.2 11.4a4.2 4.2 0 1 0 0-8.4 4.2 4.2 0 0 0 0 8.4zM10.4 10.4 13.5 13.5",
  refresh: "M13.2 7.2A5.2 5.2 0 0 0 4 4.6M2.8 8.8A5.2 5.2 0 0 0 12 11.4M2.6 3.6v3h3M13.4 12.4v-3h-3",
  warning: "M8 2.8 14 13H2zM8 6.6v3.1M8 11.4h.01",
  check: "M3.2 8.4 6.4 11.6l6.4-7.2",
  close: "M4 4l8 8M12 4l-8 8",
  minimize: "M3.5 8h9",
  maximize: "M3.5 3.5h9v9h-9z",
  "chevron-right": "M6 3.5 10.5 8 6 12.5",
  "chevron-down": "M3.5 6 8 10.5 12.5 6",
  "chevron-left": "M10 3.5 5.5 8 10 12.5",
  trash: "M3 4.4h10M6.4 4.4V3h3.2v1.4M4.4 4.4l.6 8.2h6l.6-8.2M6.6 6.8v3.6M9.4 6.8v3.6",
  external: "M9 3.5h3.5V7M12.5 3.5 7.5 8.5M11.5 9.5v3h-8v-8h3",
  cpu: "M5 5h6v6H5zM6.4 2.6v2.4M9.6 2.6v2.4M6.4 11v2.4M9.6 11v2.4M2.6 6.4H5M2.6 9.6H5M11 6.4h2.4M11 9.6h2.4",
  clock: "M8 2.6a5.4 5.4 0 1 0 0 10.8A5.4 5.4 0 0 0 8 2.6zM8 5.2V8l2.2 1.6",
  /* Window restore: the front pane, and the one behind it. */
  restore: "M3.5 5.5h7v7h-7zM5.5 5.5v-2h7v7h-2",
  // A clock with a counter-clockwise arrow: back to an earlier point.
  history: "M3 8a5 5 0 1 0 1.5-3.6M3 2.4v2.8h2.8M8 5.3V8l1.9 1.2",
  flame:
    "M8 2c2.4 2.4 3.9 4.4 3.9 6.8a3.9 3.9 0 0 1-7.8 0c0-1.6.9-2.9 1.9-4 .2 1.1.7 1.8 1.4 2.1C7.2 5.3 7.5 3.6 8 2z",
  copy: "M5.5 5.5h7v7h-7zM10.5 5.5v-2h-7v7h2",
  send: "M2.6 8.2 13.4 3 9.8 13.4 7.6 9zM7.6 9l5.8-6",
  sun: "M8 5.4a2.6 2.6 0 1 0 0 5.2 2.6 2.6 0 0 0 0-5.2zM8 1.8v1.4M8 12.8v1.4M1.8 8h1.4M12.8 8h1.4M3.6 3.6l1 1M11.4 11.4l1 1M3.6 12.4l1-1M11.4 4.6l1-1",
  monitor: "M2.5 3.5h11v7.5h-11zM6 13.5h4M8 11v2.5",
  "arrow-down": "M8 3v10M4 9l4 4 4-4",
  "arrow-up": "M8 13V3M4 7l4-4 4 4",
  upload: "M8 10.5V2.8M4.8 6 8 2.8 11.2 6M2.8 10.2v2.3h10.4v-2.3",
  power: "M8 2.4v5M4.8 4.4a4.8 4.8 0 1 0 6.4 0",
  image: "M2.5 3.5h11v9h-11zM2.5 10.5l3-3 2.6 2.6 1.9-1.9 3 3M10.4 6h.01",
  coffee: "M3 6.2h8v3.6a3 3 0 0 1-3 3H6a3 3 0 0 1-3-3zM11 7.2h1.2a1.4 1.4 0 0 1 0 2.8H11M5.4 2.6v1.6M8 2.6v1.6",
};

/** Icons that read better as a filled shape than as an outline. */
const FILLED: ReadonlySet<IconName> = new Set<IconName>(["play", "stop"]);

export interface IconProps {
  name: IconName;
  size?: number;
  className?: string;
  /** Give an icon a label only when it is the sole content of a control. */
  label?: string;
}

export function Icon({ name, size = 16, className, label }: IconProps) {
  const filled = FILLED.has(name);
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      className={className}
      fill={filled ? "currentColor" : "none"}
      stroke={filled ? "none" : "currentColor"}
      strokeWidth={1.4}
      strokeLinecap="round"
      strokeLinejoin="round"
      role={label ? "img" : "presentation"}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      focusable="false"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
