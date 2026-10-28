// Boot: wire the root once, draw, and listen for the Rust side's events.
import "./fonts";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { bindActions } from "./actions";
import { api, demo } from "./api";
import { paintStatus, paintUpdate, render, root, scheduler } from "./render";
import { isGlance, setStatus, setSyncing, setUpdate, state } from "./state";

bindActions(root);
// A field losing focus may be the moment a deferred redraw can land.
root.addEventListener("focusout", () => setTimeout(scheduler.flush, 0));

/** A sync finished. Only the scorecard shows live numbers; other tabs just update the status line. */
function refresh() {
  paintStatus();
  if (isGlance || state.tab === "score") scheduler.refresh();
}

const loadUpdate = () =>
  api
    .updateStatus()
    .then((u) => {
      setUpdate(u);
      paintUpdate();
    })
    .catch(() => {});

if (isGlance) {
  document.documentElement.classList.add("is-glance");
  if (!demo)
    window.addEventListener("keydown", (e) => {
      if (e.key === "Escape") void getCurrentWindow().hide();
    });
}
// Draw once at launch, after the status is known, so the verdict's scrawl plays exactly once.
api
  .syncStatus()
  .then(setStatus)
  .catch(() => {})
  .finally(() => render());
if (!isGlance) void loadUpdate();
if (!demo) {
  if (!isGlance) void listen("update-changed", loadUpdate);
  // A check starting changes only the status line; a finished one may change the numbers.
  void listen("sync-started", () => {
    setSyncing();
    paintStatus();
  });
  void listen("stats-changed", () => api.syncStatus().then(setStatus).then(refresh));
  void listen("sync-failed", () => api.syncStatus().then(setStatus).then(paintStatus));
}
// Keep "Checked 3 min ago" honest without re-querying stats.
setInterval(() => state.current && paintStatus(), 30_000);
window.addEventListener("hashchange", () => location.reload());
// Dev builds only: lets a test simulate a sync finishing in the background.
if (import.meta.env.DEV) Object.assign(window, { __simulateSync: refresh });
