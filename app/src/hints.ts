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
