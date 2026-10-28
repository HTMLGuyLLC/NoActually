// Wording for app updates: the scorecard banner and the Settings line. Kept pure so it's testable.
import type { UpdateStatus } from "./api";
import { EMPTY, type Html, html } from "./html";

/** The banner over the scorecard, or nothing when no update is waiting (or it was put off). */
export function updateBanner(u: UpdateStatus, dismissed: string | null): Html {
  if (!u.available || u.available === dismissed) return EMPTY;
  if (u.installing) return html`<div class="update-banner" role="status"><span>Updating to ${u.available}…</span></div>`;
  return html`<div class="update-banner" role="status"><span>Version ${u.available} is available.</span>
    <button class="small" data-action="install-update">Restart to update</button><button class="small ghost" data-action="later-update">Later</button></div>`;
}

/** One line under the Settings toggle (plain text). */
export function updateLine(u: UpdateStatus): string {
  if (u.installing) return `Updating to ${u.available}…`;
  if (u.checking) return "Checking for updates…";
  if (u.available) return `Version ${u.available} is available. You have ${u.current}.`;
  if (u.last_error) return `Couldn't check for updates. You have ${u.current}.`;
  if (u.last_check) return `You're up to date (${u.current}).`;
  return `You have ${u.current}.`;
}
