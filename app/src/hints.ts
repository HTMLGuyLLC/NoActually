// Tips you have dismissed. Kept in the window's own storage: a tip that comes back is a small
// annoyance, not lost data, so storage that is blocked or empty just shows the tip again.

/** The tips that can be dismissed. */
export type Hint = "report-cards";

const key = (hint: Hint) => `noactually.hint.${hint}`;

export function isDismissed(hint: Hint): boolean {
  try {
    return localStorage.getItem(key(hint)) === "1";
  } catch {
    return false;
  }
}

export function dismiss(hint: Hint): void {
  try {
    localStorage.setItem(key(hint), "1");
  } catch {
    // Storage is blocked: the tip returns next time.
  }
}

const RESCORED_KEY = "noactually.rescored.seen";

/** Whether the rescored line for a rebuild at `stamp` (RFC 3339) is still new: only a later one than the last dismissed shows. */
export function rescoredUnseen(stamp: string | null): boolean {
  if (!stamp) return false;
  try {
    const seen = localStorage.getItem(RESCORED_KEY);
    return !seen || new Date(stamp).getTime() > new Date(seen).getTime();
  } catch {
    return true;
  }
}

export function dismissRescored(stamp: string): void {
  try {
    localStorage.setItem(RESCORED_KEY, stamp);
  } catch {
    // Storage is blocked: the line returns next time.
  }
}
