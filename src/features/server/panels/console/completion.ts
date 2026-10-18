/**
 * Console autocomplete: pure functions, no React.
 *
 * Given what is typed and the instance's CompletionSet, work out what the
 * caret is on (the command name or one of its arguments) and which values
 * could go there. The backend only knows command names plus a one-line usage
 * per command, so argument completion is best-effort:
 *   - an argument the backend marks as `player_arg`, or whose usage token is
 *     a player placeholder (`<targets>`, `<player>`), offers online players;
 *   - a literal choice in the usage (`(grant|revoke)`, `[on|off]`) offers
 *     those words;
 *   - anything else (`<pos>`, `<item>`) offers nothing and the usage line is
 *     shown as a hint instead.
 *
 * Completion only runs with the caret at the end of the input; editing in
 * the middle of a command is left alone.
 */

import type { CommandSuggestion, CompletionSet } from "@/types/api";

/** How many rows the dropdown shows at most. */
export const MAX_SUGGESTIONS = 8;

export type SuggestionKind = "command" | "player" | "choice";

export interface Suggestion {
  /** The full token that replaces the one being typed. */
  value: string;
  kind: SuggestionKind;
  /** Argument syntax, shown on the right of the row (mono). */
  usage?: string | null;
  /** One-line description, shown when there is no usage (sans). */
  description?: string | null;
  source?: CommandSuggestion["source"];
}

export interface CompletionState {
  /** Text before the token being completed (kept as typed, slash included). */
  head: string;
  /** The token being completed, as typed (may be empty after a space). */
  partial: string;
  suggestions: Suggestion[];
  /** The command the caret is inside, once its name is complete. */
  command: CommandSuggestion | null;
  /** Zero-based argument index the caret is on; -1 while on the name. */
  argIndex: number;
}

const EMPTY: CompletionState = {
  head: "",
  partial: "",
  suggestions: [],
  command: null,
  argIndex: -1,
};

/** Player placeholders the manifest uses; mirrors `player_placeholders`. */
const PLAYER_TOKEN = /^[[<(]*<?(targets?|players?|player_name|name)>?[\])>]*$/i;

export function complete(input: string, set: CompletionSet | undefined): CompletionState {
  if (!set || !input.trim()) return EMPTY;

  // A leading slash is accepted (people type it out of habit) and kept.
  const slash = input.match(/^\s*\/?/)?.[0] ?? "";
  const body = input.slice(slash.length);
  if (/^\s/.test(body)) return EMPTY;

  const firstSpace = body.search(/\s/);
  if (firstSpace < 0) {
    return {
      ...EMPTY,
      head: slash,
      partial: body,
      suggestions: matchCommands(body, set.commands),
    };
  }

  const name = body.slice(0, firstSpace);
  const command = findCommand(name, set.commands);
  if (!command) return EMPTY;

  const rest = body.slice(firstSpace);
  const parts = rest.split(/\s+/).slice(1); // rest starts with whitespace
  const partial = parts[parts.length - 1] ?? "";
  const argIndex = parts.length - 1;
  const head = input.slice(0, input.length - partial.length);

  return {
    head,
    partial,
    command,
    argIndex,
    suggestions: matchArguments(command, argIndex, partial, set.players),
  };
}

/** Name prefix matches; plain names before namespaced ones, short before long. */
function matchCommands(partial: string, commands: CommandSuggestion[]): Suggestion[] {
  const needle = partial.toLowerCase();
  const scored: { command: CommandSuggestion; score: number }[] = [];
  for (const command of commands) {
    const name = command.name.toLowerCase();
    let score: number;
    if (name.startsWith(needle)) {
      score = 0;
    } else {
      // `home` also finds `essentials:home`, ranked after the plain match.
      const local = name.slice(name.indexOf(":") + 1);
      if (local === name || !local.startsWith(needle)) continue;
      score = 2;
    }
    if (name.includes(":")) score += 1;
    scored.push({ command, score });
  }
  scored.sort(
    (a, b) =>
      a.score - b.score ||
      a.command.name.length - b.command.name.length ||
      a.command.name.localeCompare(b.command.name),
  );
  const out = scored.slice(0, MAX_SUGGESTIONS).map(({ command }) => ({
    value: command.name,
    kind: "command" as const,
    usage: command.usage,
    description: command.description,
    source: command.source,
  }));
  // Nothing to add when the only match is exactly what is typed.
  if (out.length === 1 && out[0]?.value.toLowerCase() === needle) return [];
  return out;
}

function findCommand(name: string, commands: CommandSuggestion[]): CommandSuggestion | null {
  const lower = name.toLowerCase();
  return (
    commands.find((command) => command.name.toLowerCase() === lower) ??
    // `minecraft:ban` when only `ban` is listed (the backend drops duplicates).
    commands.find((command) => command.name.toLowerCase() === lower.slice(lower.indexOf(":") + 1)) ??
    null
  );
}

function matchArguments(
  command: CommandSuggestion,
  argIndex: number,
  partial: string,
  players: string[],
): Suggestion[] {
  const token = usageTokens(command.usage)[argIndex];
  const wantsPlayer =
    command.player_arg === argIndex || (token !== undefined && PLAYER_TOKEN.test(token));

  let values: string[] = [];
  let kind: SuggestionKind = "choice";
  if (wantsPlayer) {
    values = players;
    kind = "player";
  } else if (token) {
    values = literalChoices(token);
  }

  const needle = partial.toLowerCase();
  const matches = values.filter((value) => value.toLowerCase().startsWith(needle));
  if (matches.length === 1 && matches[0]?.toLowerCase() === needle) return [];
  return matches.slice(0, MAX_SUGGESTIONS).map((value) => ({ value, kind }));
}

/**
 * Split a usage line into argument tokens, keeping bracketed groups whole:
 * `(grant|revoke) <targets> [<reason>]` → three tokens.
 */
export function usageTokens(usage?: string | null): string[] {
  if (!usage) return [];
  const tokens: string[] = [];
  let depth = 0;
  let current = "";
  for (const char of usage.trim()) {
    if ("([<{".includes(char)) depth += 1;
    if (")]>}".includes(char)) depth = Math.max(0, depth - 1);
    if (/\s/.test(char) && depth === 0) {
      if (current) tokens.push(current);
      current = "";
    } else {
      current += char;
    }
  }
  if (current) tokens.push(current);
  return tokens;
}

/** `(a|b)`, `[a|b]`, `<a|b>` or a bare `literal` → the words; placeholders → []. */
function literalChoices(token: string): string[] {
  const inner = token.replace(/^[[(<]+|[\])>]+$/g, "");
  if (/[<>[\](){}\s]/.test(inner)) return [];
  const bare = token === inner;
  const words = inner.split("|").filter(Boolean);
  // A bare single word in a usage line is a literal keyword (`list uuids`);
  // a single bracketed word is a placeholder (`<message>`).
  if (words.length === 1 && !bare) return [];
  return words.filter((word) => /^[\w:.\-~@^*]+$/.test(word));
}

/** The remainder of `value` after `partial` when it extends it, for ghost text. */
export function ghostFor(partial: string, value: string | undefined): string {
  if (!value || value.length <= partial.length) return "";
  if (!value.toLowerCase().startsWith(partial.toLowerCase())) return "";
  return value.slice(partial.length);
}

/** The input after accepting `suggestion`: the token replaced, then a space. */
export function accept(state: CompletionState, suggestion: Suggestion): string {
  return `${state.head}${suggestion.value} `;
}
