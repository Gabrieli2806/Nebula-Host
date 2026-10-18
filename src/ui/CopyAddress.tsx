/**
 * An address well you click to copy.
 *
 * On copy the well takes a subtle accent tint and reads "Copied" for 1.2s,
 * then settles back. The timer is cleared on unmount.
 */

import { useEffect, useRef, useState, type ReactNode } from "react";

import { useI18n } from "@/i18n";
import { Icon } from "./Icon";

const COPIED_FOR_MS = 1200;

export function CopyAddress({
  value,
  label,
  hint,
  emptyText,
  size = "sm",
  showHint = true,
  className = "",
  onCopied,
}: {
  /** The address to show and copy. null/empty renders a disabled well. */
  value: string | null | undefined;
  /** Optional label inside the well, above the value (lg size). */
  label?: ReactNode;
  /** Text next to the copy icon at rest. Defaults to "Copy". */
  hint?: string;
  /** Shown in place of the value when there is none. */
  emptyText?: string;
  /** sm: 36px card well, 13px readout. lg: 64px overview well, 20px readout. */
  size?: "sm" | "lg";
  /** Hide the hint word and keep only the icon (tight cards). */
  showHint?: boolean;
  className?: string;
  onCopied?: () => void;
}) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const copy = async () => {
    if (!value) return;
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      // Clipboard refused (permissions, unfocused window): say nothing rather
      // than claim a copy that did not happen.
      return;
    }
    setCopied(true);
    onCopied?.();
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(false), COPIED_FOR_MS);
  };

  const hintText = hint ?? t("common.copy");

  return (
    <button
      type="button"
      className={`copy-address ${className}`}
      data-size={size}
      data-copied={copied ? "" : undefined}
      data-empty={value ? undefined : ""}
      disabled={!value}
      onClick={() => void copy()}
      aria-label={value ? `${hintText}: ${value}` : undefined}
    >
      <span className="copy-address-body">
        {label ? <span className="label copy-address-label">{label}</span> : null}
        <span className="copy-address-value selectable" aria-live="polite">
          {copied ? t("common.copied") : value || emptyText || ""}
        </span>
      </span>
      {value ? (
        <span className="copy-address-action" aria-hidden="true">
          <Icon name={copied ? "check" : "copy"} size={14} />
          {showHint && !copied ? hintText : null}
        </span>
      ) : null}
    </button>
  );
}
