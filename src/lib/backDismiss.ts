/**
 * The back button (Android's, a phone browser's gesture, a mouse's back key)
 * closes what is open on top first: a dialog, a sheet, a menu. Only with
 * nothing open does it go back a screen, and on the first screen leave.
 *
 * Each open overlay holds one history entry of its own. Back pops it and
 * closes the overlay; closing it any other way (its own button, Escape)
 * takes the entry back off, so history stays as if it had never opened.
 * Overlays stack: back closes the newest first.
 */

import { useEffect, useRef } from "react";

const MARK = "nebulaOverlay";

interface Entry {
  id: number;
  close: () => void;
}

const stack: Entry[] = [];
let nextId = 1;
/** History steps we take ourselves, whose popstate is not a back press. */
let ownPops = 0;
let listening = false;

function onPop() {
  if (ownPops > 0) {
    ownPops -= 1;
    return;
  }
  // A back press: the newest overlay's entry is the one that went.
  const top = stack.pop();
  top?.close();
}

function listen() {
  if (listening || typeof window === "undefined") return;
  listening = true;
  // Capture: before the router hears it and redraws for the same address.
  window.addEventListener("popstate", onPop, true);
}

/**
 * While `open`, the back button calls `onClose` instead of leaving the
 * screen. Pass `enabled = false` for an overlay that must not be dismissed
 * now (something runs that cannot be called off).
 */
export function useBackDismiss(open: boolean, onClose: () => void, enabled = true) {
  const closeRef = useRef(onClose);
  useEffect(() => {
    closeRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    if (!open || !enabled || typeof window === "undefined") return;
    listen();
    const id = nextId++;
    stack.push({ id, close: () => closeRef.current() });
    // The same address, one step deeper: the router sees nothing new.
    window.history.pushState({ ...(window.history.state ?? {}), [MARK]: id }, "");
    return () => {
      const index = stack.findIndex((entry) => entry.id === id);
      if (index < 0) return; // Back already took it, and its entry.
      stack.splice(index, 1);
      // Closed some other way: take our entry back off, if it is on top.
      if (window.history.state?.[MARK] === id) {
        ownPops += 1;
        window.history.back();
      }
    };
  }, [open, enabled]);
}
