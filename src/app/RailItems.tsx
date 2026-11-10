/**
 * The sidebar's building blocks, shared by the app's navigation, an open
 * server's rail and an open launcher instance's rail.
 */

import { NavLink, useMatch } from "react-router-dom";

import { Icon, type IconName } from "@/ui";

/** A destination in the sidebar. Collapsed, its name shows as the tooltip. */
export function NavItem({ to, icon, label }: { to: string; icon: IconName; label: string }) {
  // Same rule NavLink uses (prefix match), surfaced as an attribute so the
  // active look is a CSS concern.
  const active = useMatch({ path: to, end: false }) !== null;
  return (
    <NavLink
      to={to}
      data-active={active ? "" : undefined}
      data-tip={label}
      className={({ isActive }) => `rail-item ${isActive ? "active" : ""}`}
    >
      <Icon name={icon} />
      <span className="rail-label">{label}</span>
    </NavLink>
  );
}

/** The way back from an open server or instance to its list. */
export function RailBack({ to, label }: { to: string; label: string }) {
  return (
    <NavLink to={to} end className="rail-back" data-tip={label}>
      <Icon name="chevron-left" size={14} />
      <span className="rail-label">{label}</span>
    </NavLink>
  );
}

/** An icon-only key for the collapsed rail; its name shows as the tooltip. */
export function RailKey({
  icon,
  label,
  primary = false,
  disabled,
  onClick,
}: {
  icon: IconName;
  label: string;
  primary?: boolean;
  disabled: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`btn btn-sm rail-key ${primary ? "btn-primary" : ""}`}
      data-tip={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
    >
      <Icon name={icon} />
    </button>
  );
}

/** Collapsed rail: the last action failed. Opens the rail to show why. */
export function RailAlert({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button type="button" className="rail-key rail-key-alert" data-tip={label} aria-label={label} onClick={onClick}>
      <Icon name="warning" />
    </button>
  );
}
