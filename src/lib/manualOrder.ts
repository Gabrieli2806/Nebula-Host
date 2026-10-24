/**
 * An order the user arranges by hand (drag a card onto another, or "Move to
 * start / end" from its menu). It belongs to this device, like the other list
 * preferences, so it lives in this browser's storage as a list of ids.
 * Items the list has never seen go after the arranged ones, in their own order.
 */

import { useState, type DragEvent } from "react";

function read(key: string): string[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(key) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter((id): id is string => typeof id === "string") : [];
  } catch {
    return [];
  }
}

function write(key: string, ids: string[]) {
  try {
    localStorage.setItem(key, JSON.stringify(ids));
  } catch {
    // Storage blocked: the order lasts for this visit.
  }
}

export type DragProps = ReturnType<ReturnType<typeof useManualOrder>["dragProps"]>;

export function useManualOrder(key: string) {
  const [order, setOrder] = useState<string[]>(() => read(key));
  const [dragging, setDragging] = useState<string | null>(null);
  const [over, setOver] = useState<string | null>(null);

  const save = (ids: string[]) => {
    setOrder(ids);
    write(key, ids);
  };

  /** `items` in the arranged order. */
  const arrange = <T>(items: T[], idOf: (item: T) => string): T[] => {
    const position = new Map(order.map((id, index) => [id, index]));
    return items
      .map((item, index) => ({ item, rank: position.get(idOf(item)) ?? order.length + index }))
      .sort((a, b) => a.rank - b.rank)
      .map(({ item }) => item);
  };

  /** Move `id` to where `target` is, within the full arranged list `ids`. */
  const moveOnto = (ids: string[], id: string, target: string) => {
    if (id === target) return;
    const from = ids.indexOf(id);
    const to = ids.indexOf(target);
    if (from < 0 || to < 0) return;
    const next = ids.filter((other) => other !== id);
    next.splice(to, 0, id);
    save(next);
  };

  const moveTo = (ids: string[], id: string, edge: "start" | "end") => {
    const rest = ids.filter((other) => other !== id);
    save(edge === "start" ? [id, ...rest] : [...rest, id]);
  };

  /**
   * Props for one draggable item. `ids` is the full arranged list (not just
   * what a filter shows), so hidden items keep their place. `onArranged`
   * runs before the first manual move, to switch a sorted view to this order.
   */
  const dragProps = (ids: string[], id: string, onArranged?: () => void) => ({
    draggable: true,
    "data-dragging": dragging === id ? "" : undefined,
    "data-drop-target": over === id && dragging !== id ? "" : undefined,
    onDragStart: (event: DragEvent) => {
      // Text fields and the like keep their own dragging.
      if ((event.target as HTMLElement).closest?.("input, textarea, select")) return;
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/plain", id);
      setDragging(id);
    },
    onDragOver: (event: DragEvent) => {
      if (!dragging) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
      if (over !== id) setOver(id);
    },
    onDragLeave: () => {
      if (over === id) setOver(null);
    },
    onDrop: (event: DragEvent) => {
      if (!dragging) return;
      event.preventDefault();
      onArranged?.();
      moveOnto(ids, dragging, id);
      setDragging(null);
      setOver(null);
    },
    onDragEnd: () => {
      setDragging(null);
      setOver(null);
    },
  });

  return { arrange, moveTo, dragProps };
}
