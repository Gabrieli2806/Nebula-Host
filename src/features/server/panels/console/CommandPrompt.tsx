/**
 * The console prompt with command autocomplete.
 *
 * Ghost text shows the rest of the highlighted suggestion inside the field;
 * a small list above the prompt shows the others. Keys:
 *   - Tab accepts the highlighted suggestion (and keeps focus here).
 *   - ↑ / ↓ move through the list while it is open; with it closed they walk
 *     the command history exactly as before.
 *   - Enter sends, unless you have moved through the list, then it accepts.
 *   - Esc closes the list until you type again.
 * Recalling a history entry keeps the list closed, so a second ↑ keeps
 * walking history instead of landing in the suggestions.
 */

import { useId, useLayoutEffect, useMemo, useRef, useState } from "react";

import { useI18n } from "@/i18n";
import type { CompletionSet } from "@/types/api";
import { Button, PlayerHead } from "@/ui";

import { accept, complete, ghostFor, usageTokens } from "./completion";

export function CommandPrompt({
  running,
  online,
  completions,
  loadingCompletions,
  uuidOf,
  onSubmit,
}: {
  running: boolean;
  online: boolean;
  completions: CompletionSet | undefined;
  loadingCompletions: boolean;
  /** Account UUID for a player name, for the heads in player suggestions. */
  uuidOf: (name: string) => string | undefined;
  onSubmit: (value: string) => void;
}) {
  const { t } = useI18n();
  const listId = useId();
  const input = useRef<HTMLInputElement>(null);
  const [command, setCommand] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const [active, setActive] = useState(0);
  /** True once ↑/↓ moved the highlight: Enter then accepts instead of sending. */
  const [navigated, setNavigated] = useState(false);
  /** Closed by Esc, a history recall or blur; reopens on the next keystroke. */
  const [dismissed, setDismissed] = useState(false);
  const [focused, setFocused] = useState(false);
  const [caretAtEnd, setCaretAtEnd] = useState(true);
  const [overflowing, setOverflowing] = useState(false);

  const state = useMemo(() => complete(command, completions), [command, completions]);
  const { suggestions } = state;
  const open = running && focused && !dismissed && caretAtEnd && suggestions.length > 0;
  const current = open ? suggestions[Math.min(active, suggestions.length - 1)] : undefined;
  const ghost = current && !overflowing ? ghostFor(state.partial, current.value) : "";

  // While the server is still answering `help`, say so once someone types.
  const waiting =
    running && focused && !dismissed && loadingCompletions && !completions && command.trim() !== "";
  // A usage hint for the argument under the caret when nothing can be offered.
  const hint =
    running && focused && !dismissed && caretAtEnd && !open && state.command?.usage
      ? state.command
      : null;

  // Ghost text is laid over the field; it only lines up while the text fits.
  useLayoutEffect(() => {
    const element = input.current;
    if (element) setOverflowing(element.scrollWidth > element.clientWidth + 1);
  }, [command]);

  const edit = (value: string) => {
    setCommand(value);
    setActive(0);
    setNavigated(false);
    setDismissed(false);
  };

  const recall = (value: string) => {
    setCommand(value);
    setDismissed(true);
    // Put the caret at the end of the recalled command.
    requestAnimationFrame(() => {
      const element = input.current;
      if (element) element.setSelectionRange(value.length, value.length);
    });
  };

  const take = (index: number) => {
    const suggestion = suggestions[index];
    if (!suggestion) return;
    edit(accept(state, suggestion));
    input.current?.focus();
  };

  const submit = () => {
    const value = command.trim();
    if (!value) return;
    onSubmit(value);
    setHistory((previous) => [value, ...previous].slice(0, 50));
    setHistoryIndex(-1);
    edit("");
  };

  const trackCaret = () => {
    const element = input.current;
    if (element) setCaretAtEnd(element.selectionStart === element.value.length);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    if (open) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : -1;
        setActive((index) => (index + step + suggestions.length) % suggestions.length);
        setNavigated(true);
        return;
      }
      if (event.key === "Tab" && !event.shiftKey) {
        event.preventDefault();
        take(active);
        return;
      }
      if (event.key === "Enter" && navigated) {
        event.preventDefault();
        take(active);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        setDismissed(true);
        return;
      }
    }

    if (event.key === "Enter") {
      submit();
    } else if (event.key === "Escape" && hint) {
      setDismissed(true);
    } else if (event.key === "ArrowUp" && history.length) {
      // Command history: the last thing you typed is nearly always the
      // thing you want to type again.
      event.preventDefault();
      const next = Math.min(historyIndex + 1, history.length - 1);
      setHistoryIndex(next);
      recall(history[next] ?? "");
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      const next = historyIndex - 1;
      setHistoryIndex(next);
      recall(next >= 0 ? (history[next] ?? "") : "");
    }
  };

  const source =
    online && completions
      ? completions.from_server
        ? t("console.completeFromServer")
        : t("console.completeBuiltinOnly")
      : null;

  return (
    <div className="console-prompt">
      <div className="console-prompt-field">
        <span className="console-prompt-glyph" aria-hidden="true">
          &gt;
        </span>
        <input
          ref={input}
          className="field mono console-prompt-input"
          value={command}
          disabled={!running}
          placeholder={t("console.placeholder")}
          aria-label={t("console.placeholder")}
          spellCheck={false}
          autoComplete="off"
          role="combobox"
          aria-autocomplete="both"
          aria-expanded={open}
          aria-controls={listId}
          aria-activedescendant={open ? `${listId}-${active}` : undefined}
          onChange={(event) => {
            edit(event.target.value);
            setCaretAtEnd(event.target.selectionStart === event.target.value.length);
          }}
          onSelect={trackCaret}
          onFocus={() => setFocused(true)}
          onBlur={() => {
            setFocused(false);
            setNavigated(false);
          }}
          onKeyDown={onKeyDown}
        />
        {ghost ? (
          <span className="console-ghost mono" aria-hidden="true">
            <span className="console-ghost-typed">{command}</span>
            {ghost}
          </span>
        ) : null}

        {open ? (
          <div className="console-suggest" role="presentation">
            <ul id={listId} role="listbox" aria-label={t("console.suggestions")} className="console-suggest-list">
              {suggestions.map((suggestion, index) => (
                <li
                  key={`${suggestion.kind}-${suggestion.value}`}
                  id={`${listId}-${index}`}
                  role="option"
                  aria-selected={index === active}
                  className="console-suggest-row"
                  // Keep focus in the field; a click picks the row.
                  onMouseDown={(event) => event.preventDefault()}
                  onMouseMove={() => setActive(index)}
                  onClick={() => take(index)}
                >
                  {suggestion.kind === "player" ? <PlayerHead name={suggestion.value} uuid={uuidOf(suggestion.value)} size={16} /> : null}
                  <span className="console-suggest-name mono">
                    <span className="console-suggest-match">{suggestion.value.slice(0, matchLength(state.partial, suggestion.value))}</span>
                    {suggestion.value.slice(matchLength(state.partial, suggestion.value))}
                  </span>
                  {suggestion.kind === "player" ? (
                    <span className="console-suggest-detail">{t("console.suggestPlayer")}</span>
                  ) : suggestion.usage ? (
                    <span className="console-suggest-detail mono">{suggestion.usage}</span>
                  ) : suggestion.description ? (
                    <span className="console-suggest-detail">{suggestion.description}</span>
                  ) : null}
                </li>
              ))}
            </ul>
            <div className="console-suggest-foot">
              <span>{t("console.completeKeys")}</span>
              {source ? <span>{source}</span> : null}
            </div>
          </div>
        ) : hint ? (
          <div className="console-suggest console-usage">
            <span className="mono">
              <span className="console-usage-name">{hint.name}</span>{" "}
              {usageTokens(hint.usage).map((token, index) => (
                <span key={index} className="console-usage-arg" data-current={index === state.argIndex || undefined}>
                  {token}{" "}
                </span>
              ))}
            </span>
          </div>
        ) : waiting ? (
          <div className="console-suggest console-usage" aria-live="polite">
            <span>{t("console.completeLoading")}</span>
          </div>
        ) : null}
      </div>
      <Button icon="send" disabled={!running || !command.trim()} onClick={submit}>
        {t("console.send")}
      </Button>
    </div>
  );
}

/** How much of `value` the typed `partial` covers; 0 for a namespaced match (`home` → `essentials:home`). */
function matchLength(partial: string, value: string): number {
  if (!partial) return 0;
  if (value.toLowerCase().startsWith(partial.toLowerCase())) return partial.length;
  return 0;
}
