/** Ask the mounted tour to open (Settings > About). */
export const OPEN_TOUR = "nebula:open-tour";

export function openTour() {
  window.dispatchEvent(new Event(OPEN_TOUR));
}
