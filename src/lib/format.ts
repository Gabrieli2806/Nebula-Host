/** Number and time formatting, in the units a person reads. */

/** A number with one decimal at most, in the interface's language ("2,5" in
 *  Spanish). The language is the page's, which the i18n provider sets. */
export function formatDecimal(value: number, digits = 1): string {
  const lang = (typeof document !== "undefined" && document.documentElement.lang) || undefined;
  try {
    return new Intl.NumberFormat(lang, { maximumFractionDigits: digits, minimumFractionDigits: 0 }).format(value);
  } catch {
    return value.toFixed(digits);
  }
}

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)));
  const value = bytes / 1024 ** index;
  return `${value >= 10 || index === 0 ? formatDecimal(Math.round(value), 0) : formatDecimal(value)} ${units[index]}`;
}

export function formatMegabytes(mb: number): string {
  return mb >= 1024 ? `${formatDecimal(mb / 1024)} GB` : `${Math.round(mb)} MB`;
}

/** Uptime as `4h 12m`, never as `15120 seconds`. */
export function formatDuration(seconds: number | null | undefined): string {
  if (!seconds || seconds < 0) return "—";
  const days = Math.floor(seconds / 86400);
  const hours = Math.floor((seconds % 86400) / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m`;
  return `${Math.floor(seconds)}s`;
}

export function formatDateTime(iso: string | null | undefined, locale: string): string {
  if (!iso) return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "—";
  return date.toLocaleString(locale, {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** "2 h ago" / "hace 2 h": short enough for a one-line caption. */
export function formatRelative(iso: string | null | undefined, locale: string, now = Date.now()): string {
  if (!iso) return "—";
  const time = new Date(iso).getTime();
  if (Number.isNaN(time)) return "—";
  const seconds = Math.round((time - now) / 1000);
  const format = new Intl.RelativeTimeFormat(locale, { numeric: "auto", style: "short" });
  const steps: [Intl.RelativeTimeFormatUnit, number][] = [
    ["day", 86400],
    ["hour", 3600],
    ["minute", 60],
  ];
  for (const [unit, size] of steps) {
    if (Math.abs(seconds) >= size) {
      if (unit === "day" && Math.abs(seconds) >= 30 * 86400) {
        return new Date(time).toLocaleDateString(locale, { day: "numeric", month: "short" });
      }
      return format.format(Math.round(seconds / size), unit);
    }
  }
  return format.format(0, "minute");
}

export function formatTime(iso: string, locale: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString(locale, { hour12: false });
}

export function formatCount(value: number, locale: string): string {
  return new Intl.NumberFormat(locale, { notation: value >= 10000 ? "compact" : "standard" }).format(
    value,
  );
}
