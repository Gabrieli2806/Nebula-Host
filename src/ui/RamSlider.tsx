/**
 * Memory for a server, as a slider rather than a list of presets.
 *
 * The range runs from 1 GB up to a safe ceiling derived from this machine's
 * memory (total minus the larger of 2 GB and 20%, rounded down to 512 MB), so
 * a 64 or 128 GB machine gets a long, usable range and an 8 GB laptop a short
 * one. The tick labels pick their spacing from the range, the recommended band
 * comes from the kind of server (vanilla, plugins, mods) and how many addons it
 * carries, and the number field beside the track takes an exact amount.
 *
 * Controlled: `onChange` fires on every movement, `onCommit` once the person
 * lets go (pointer up, a pause after keys, blur or Enter in the number field),
 * so an autosaving caller saves once per gesture, not once per pixel.
 */

import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
} from "react";

import { useInstances, useNodeHealth } from "@/hooks/useBackend";
import { useI18n } from "@/i18n";
import { Icon } from "./Icon";
import "./RamSlider.css";

const MB_PER_GB = 1024;
const MIN_MB = 1024;
const STEP_MB = 256;
const PAGE_MB = 1024;
/** Range used while the machine's total is still unknown. */
const FALLBACK_MAX_MB = 16384;
/** Keys commit after this pause, so holding an arrow saves once. */
const KEY_COMMIT_MS = 600;
/** Never let the number field ask for more than total minus this. */
const HARD_HEADROOM_MB = 1024;
/**
 * Past this share of the machine's memory, counting what the other running
 * servers already hold, the track turns amber and a note says why.
 */
const CAUTION_SHARE = 0.75;

export type RamProfile = "vanilla" | "plugins" | "mods";

/** The profile for a distribution's `addon_project_type`. */
function ramProfileOf(projectType?: string | null): RamProfile {
  if (projectType === "mod") return "mods";
  if (projectType === "plugin") return "plugins";
  return "vanilla";
}

export interface RamPlan {
  /** Machine total, when known. */
  totalMb: number | null;
  /** The largest amount that leaves the OS enough room. */
  safeMaxMb: number;
  /** The most the number field accepts. */
  hardMaxMb: number;
  zone: [number, number];
  recommendedMb: number;
}

/** Total minus the larger of 2 GB and 20%, rounded down to 512 MB. */
function safeMaxFor(totalMb: number): number {
  const headroom = Math.max(2048, totalMb * 0.2);
  return Math.max(MIN_MB, Math.floor((totalMb - headroom) / 512) * 512);
}

/** Recommended band and point, in MB, before clamping to the machine. */
function baseRecommendation(profile: RamProfile, addons: number): { zone: [number, number]; at: number } {
  if (profile === "mods") {
    if (addons > 150) return { zone: [10240, 16384], at: 12288 };
    if (addons > 80) return { zone: [8192, 12288], at: 10240 };
    return { zone: [6144, 10240], at: 6144 };
  }
  if (profile === "plugins") {
    if (addons > 40) return { zone: [6144, 8192], at: 6144 };
    return { zone: [4096, 6144], at: 4096 };
  }
  return { zone: [2048, 4096], at: 3072 };
}

function ramPlan(profile: RamProfile, addonCount: number, totalMb: number | null): RamPlan {
  const safeMaxMb = totalMb ? safeMaxFor(totalMb) : Infinity;
  const hardMaxMb = totalMb
    ? Math.max(MIN_MB, Math.floor((totalMb - HARD_HEADROOM_MB) / STEP_MB) * STEP_MB)
    : 262144;
  const base = baseRecommendation(profile, addonCount);
  const clamp = (mb: number) => Math.max(MIN_MB, Math.min(mb, safeMaxMb));
  const zone: [number, number] = [clamp(base.zone[0]), clamp(base.zone[1])];
  return {
    totalMb,
    safeMaxMb: Number.isFinite(safeMaxMb) ? safeMaxMb : FALLBACK_MAX_MB,
    hardMaxMb,
    zone,
    recommendedMb: clamp(base.at),
  };
}

/**
 * The plan for this machine and this kind of server (`projectType` is the
 * distribution's `addon_project_type`). The create dialog uses it for its
 * default amount.
 */
export function useRamPlan(
  projectType?: string | null,
  addonCount = 0,
  totalOverrideMb?: number,
): RamPlan {
  const { data: health } = useNodeHealth();
  const total = totalOverrideMb ?? (health?.memory_total_mb || null);
  return ramPlan(ramProfileOf(projectType), addonCount, total);
}

// -- helpers ------------------------------------------------------------------

function snap(mb: number): number {
  return Math.round(mb / STEP_MB) * STEP_MB;
}

function useGbFormat() {
  const { language } = useI18n();
  const number = new Intl.NumberFormat(language, { maximumFractionDigits: 2 });
  return {
    gb: (mb: number) => number.format(mb / MB_PER_GB),
    withUnit: (mb: number) => `${number.format(mb / MB_PER_GB)} GB`,
  };
}

/** Tick values in MB: a spacing from 1/2/4/8… GB that gives at most 7 gaps. */
function ticksFor(min: number, max: number): number[] {
  const spanGb = (max - min) / MB_PER_GB;
  const interval = [1, 2, 4, 8, 16, 32, 64].find((gb) => spanGb / gb <= 7) ?? 128;
  const ticks = [min];
  for (let gb = interval; gb * MB_PER_GB < max; gb += interval) {
    const mb = gb * MB_PER_GB;
    if (mb <= min) continue;
    // Leave room for the end label.
    if ((max - mb) / (max - min) < 0.07) continue;
    ticks.push(mb);
  }
  ticks.push(max);
  return ticks;
}

// -- component ----------------------------------------------------------------

export interface RamSliderProps {
  value: number;
  onChange: (mb: number) => void;
  onCommit?: (mb: number) => void;
  /** The distribution's `addon_project_type`: `mod`, `plugin` or none. */
  projectType?: string | null;
  addonCount?: number;
  /** Override the machine total (tests, remote nodes). */
  totalMb?: number;
  /**
   * The server being edited, so its own allocation is not counted twice
   * among "the other running servers". Omit when creating a new one.
   */
  instanceId?: string;
  id?: string;
  /** Id of the visible label for the slider. */
  labelledBy?: string;
  disabled?: boolean;
  className?: string;
}

export function RamSlider({
  value,
  onChange,
  onCommit,
  projectType,
  addonCount = 0,
  totalMb,
  instanceId,
  id,
  labelledBy,
  disabled,
  className = "",
}: RamSliderProps) {
  const { t: copy } = useI18n();
  const format = useGbFormat();
  const plan = useRamPlan(projectType, addonCount, totalMb);
  const profile = ramProfileOf(projectType);
  const ownId = useId();
  const sliderId = id ?? `${ownId}-range`;
  const statusId = `${ownId}-status`;
  const warnId = `${ownId}-warn`;

  // Memory the other live servers already hold (a sleeping or crashed server
  // has no process and holds nothing).
  const { data: instances } = useInstances();
  const otherMb = (instances ?? [])
    .filter(
      (instance) =>
        instance.id !== instanceId &&
        (instance.status === "online" || instance.status === "starting" || instance.status === "stopping"),
    )
    .reduce((sum, instance) => sum + instance.resources.memory_mb, 0);

  // The track always ends at the safe ceiling, so the scale, its ticks and the
  // recommended band never move under the thumb. A value above it (typed, or
  // an older setting) pins the thumb at the end and the warning says why.
  const min = MIN_MB;
  const max = Math.max(plan.safeMaxMb, min + 2048);
  // Where this server plus the others would pass CAUTION_SHARE of the machine.
  const cautionMb =
    plan.totalMb !== null
      ? Math.max(min, Math.floor((plan.totalMb * CAUTION_SHARE - otherMb) / STEP_MB) * STEP_MB)
      : null;
  const cautionOnTrack = cautionMb !== null && cautionMb < max;
  const pct = (mb: number) => `${((Math.min(Math.max(mb, min), max) - min) / (max - min)) * 100}%`;

  // The value a commit sends: set by our own handlers at once, and kept in
  // step with the prop for changes from outside.
  const latest = useRef(value);
  useEffect(() => {
    latest.current = value;
  }, [value]);
  const keyTimer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(keyTimer.current), []);

  const commit = () => {
    window.clearTimeout(keyTimer.current);
    keyTimer.current = undefined;
    onCommit?.(latest.current);
  };
  const commitSoon = () => {
    window.clearTimeout(keyTimer.current);
    keyTimer.current = window.setTimeout(commit, KEY_COMMIT_MS);
  };

  const set = (mb: number) => {
    const next = Math.min(Math.max(snap(mb), min), plan.hardMaxMb);
    latest.current = next;
    onChange(next);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "PageUp" || event.key === "PageDown") {
      event.preventDefault();
      set(Math.min(max, Math.max(min, value + (event.key === "PageUp" ? PAGE_MB : -PAGE_MB))));
      commitSoon();
    }
  };

  // The number field keeps its own text while focused, so typing "1" on the
  // way to "12" does not snap to 1 GB.
  const [text, setText] = useState<string | null>(null);
  const applyText = () => {
    if (text === null) return;
    const gb = Number(text.replace(",", "."));
    setText(null);
    if (!Number.isFinite(gb) || gb <= 0) return;
    set(gb * MB_PER_GB);
    commit();
  };

  const over = plan.totalMb !== null && value > plan.safeMaxMb;
  const caution = !over && cautionMb !== null && value > cautionMb;
  const under = value < plan.zone[0];
  const freeMb = plan.totalMb !== null ? plan.totalMb - value : 0;

  const kind =
    profile === "mods"
      ? addonCount > 0
        ? copy("ram.kindModsCount", { count: addonCount })
        : copy("ram.kindMods")
      : profile === "plugins"
        ? addonCount > 0
          ? copy("ram.kindPluginsCount", { count: addonCount })
          : copy("ram.kindPlugins")
        : copy("ram.kindVanilla");
  const range =
    plan.zone[0] === plan.zone[1]
      ? format.withUnit(plan.zone[0])
      : `${format.gb(plan.zone[0])}–${format.withUnit(plan.zone[1])}`;

  const style = {
    "--ram-fill": pct(value),
    "--ram-zone-start": pct(plan.zone[0]),
    "--ram-zone-end": pct(plan.zone[1]),
    "--ram-rec": pct(plan.recommendedMb),
    "--ram-safe": pct(plan.safeMaxMb),
    "--ram-caution": cautionOnTrack && cautionMb !== null ? pct(cautionMb) : "100%",
  } as CSSProperties;

  const ticks = ticksFor(min, max);

  return (
    <div
      className={`ram-slider ${className}`}
      style={style}
      data-over={over ? "" : undefined}
      data-caution={caution ? "" : undefined}
      data-disabled={disabled ? "" : undefined}
    >
      <div className="ram-main">
        <div className="ram-scale">
          <div className="ram-track">
            <div className="ram-rail" aria-hidden="true">
              <span className="ram-zone" />
              {cautionOnTrack ? <span className="ram-caution" /> : null}
              <span className="ram-fill" />
              <span className="ram-rec" />
            </div>
            <input
              id={sliderId}
              className="ram-input"
              type="range"
              min={min}
              max={max}
              step={STEP_MB}
              value={Math.min(value, max)}
              disabled={disabled}
              aria-labelledby={labelledBy}
              aria-valuetext={format.withUnit(value)}
              aria-describedby={over || caution ? `${statusId} ${warnId}` : statusId}
              onChange={(event) => set(Number(event.target.value))}
              onPointerUp={commit}
              onKeyDown={onKeyDown}
              onKeyUp={(event) => {
                if (event.key !== "Tab" && event.key !== "Shift") commitSoon();
              }}
              onBlur={() => {
                if (keyTimer.current !== undefined) commit();
              }}
            />
          </div>
          <div className="ram-ticks" aria-hidden="true">
            {ticks.map((mb, index) => (
              <span
                key={mb}
                className="ram-tick"
                data-edge={index === 0 ? "start" : index === ticks.length - 1 ? "end" : undefined}
                style={{ left: pct(mb) }}
              >
                {index === ticks.length - 1 ? format.withUnit(mb) : format.gb(mb)}
              </span>
            ))}
          </div>
        </div>

        <label className="ram-amount">
          <input
            className="field ram-number"
            type="number"
            inputMode="decimal"
            min={min / MB_PER_GB}
            max={plan.hardMaxMb / MB_PER_GB}
            step={STEP_MB / MB_PER_GB}
            disabled={disabled}
            aria-label={copy("ram.amount")}
            value={text ?? String(value / MB_PER_GB)}
            onFocus={(event) => setText(event.target.value)}
            onChange={(event) => setText(event.target.value)}
            onBlur={applyText}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                applyText();
              }
              if (event.key === "Escape" && text !== null) {
                event.stopPropagation();
                setText(null);
              }
            }}
          />
          <span className="ram-unit" aria-hidden="true">
            {copy("ram.unit")}
          </span>
        </label>
      </div>

      <div className="ram-status">
        <span id={statusId} className="ram-status-text">
          {copy("ram.recommended", { kind, range })}
          {plan.totalMb !== null ? (
            <span className="ram-machine">
              {copy("ram.machine", { total: format.withUnit(plan.totalMb) })}
              {", "}
              {copy("ram.safeLimit", { value: format.withUnit(plan.safeMaxMb) })}
            </span>
          ) : null}
        </span>
        {value !== plan.recommendedMb ? (
          <button
            type="button"
            className="btn btn-quiet btn-sm ram-use"
            disabled={disabled}
            onClick={() => {
              set(plan.recommendedMb);
              commit();
            }}
          >
            {copy("ram.useRecommended", { value: format.withUnit(plan.recommendedMb) })}
          </button>
        ) : null}
      </div>

      {over ? (
        <p id={warnId} className="ram-warn" role="status">
          <Icon name="warning" size={14} />
          {copy("ram.tooHigh", { free: format.withUnit(Math.max(0, freeMb)) })}
        </p>
      ) : caution && plan.totalMb !== null ? (
        <p id={warnId} className="ram-note" data-tone="caution" role="status">
          <Icon name="warning" size={14} />
          {otherMb > 0
            ? copy("ram.cautionShared", {
                others: format.withUnit(otherMb),
                sum: format.withUnit(value + otherMb),
                total: format.withUnit(plan.totalMb),
              })
            : copy("ram.cautionAlone")}
        </p>
      ) : under ? (
        <p className="ram-note">{copy("ram.belowRecommended")}</p>
      ) : null}
    </div>
  );
}
