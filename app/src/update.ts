// Wording for app updates: the scorecard banner and the Settings line. Kept pure so it's testable.
import type { UpdateStatus } from "./api";
import { EMPTY, type Html, html } from "./html";

/** The banner over the scorecard, or nothing when no update is waiting (or it was put off). */
export function updateBanner(u: UpdateStatus, dismissed: string | null): Html {
  if (!u.available || u.available === dismissed) return EMPTY;
  if (u.installing) return html`<div class="update-banner" role="status"><span>Updating to ${u.available}…</span></div>`;
  // A try that failed says so here, where the person pressed the button, and the button stays for another go.
  const failed = u.last_error ? html`<span class="action-error">The last try failed: ${u.last_error}</span>` : "";
  return html`<div class="update-banner" role="status"><span>Version ${u.available} is available.</span>
    <button class="small" data-action="install-update">${u.last_error ? "Try again" : "Restart to update"}</button><button class="small ghost" data-action="later-update">Later</button>${failed}</div>`;
}

/** One line under the Settings toggle (plain text). */
export function updateLine(u: UpdateStatus): string {
  if (u.installing) return `Updating to ${u.available}…`;
  if (u.checking) return "Checking for updates…";
  if (u.available) return `Version ${u.available} is available. You have ${u.current}.${u.last_error ? ` The last try failed: ${u.last_error}` : ""}`;
  if (u.last_error) return `Couldn't check for updates. You have ${u.current}.`;
  if (u.last_check) return `You're up to date (${u.current}).`;
  return `You have ${u.current}.`;
}

/** Under the update banner, once scores were recounted (`stamp` names the rebuild, so a later one shows again). */
export function rescoredNotice(stamp: string): Html {
  return html`<div class="update-banner" role="status"><span>We recounted your history with updated rules, so your score may have moved.</span>
    <button class="small ghost" data-action="dismiss-rescored" data-id="${stamp}">Got it</button></div>`;
}

/** The first read of a history finished while the person was somewhere else. */
export function readingDoneNotice(): Html {
  return html`<div class="update-banner" role="status"><span>Your history is read.</span><button class="small" data-action="home">See your scorecard</button></div>`;
}
