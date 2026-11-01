// Boot: wire the root once, draw, and listen for the Rust side's events.
import "./fonts";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { bindActions } from "./actions";
import { api, demo } from "./api";
import { bindChartTips } from "./charttip";
import { bindGlobalErrors, report } from "./log";
import { paintStatus, paintUpdate, render, root, scheduler } from "./render";
import { checkingLeft, hasOverlayTitleBar, isGlance, isTab, setReading, setStatus, setSyncing, setTab, setUpdate, state } from "./state";

bindGlobalErrors();
bindActions(root);
bindChartTips(root);
// A field losing focus may be the moment a deferred redraw can land.
root.addEventListener("focusout", () => setTimeout(scheduler.flush, 0));

/** A sync finished. Only the scorecard shows live numbers; other tabs just update the status line. */
function refresh() {
  paintStatus();
  if (isGlance || state.tab === "score") scheduler.refresh();
}

/** New numbers arrived while the window was hidden: drawn when it's shown again. */
let redrawOnShow = false;

/** Like `refresh`, but a hidden window only updates the status line and waits to be shown to redraw. */
function refreshIfShown() {
  if (!document.hidden) return refresh();
  paintStatus();
  redrawOnShow = isGlance || state.tab === "score";
}

/** Whether a pass just ended that changes what the page shows: a rebuild finished, or a first read did. */
function endedSomething(wasRebuilding: boolean): boolean {
  let ended = wasRebuilding && !state.status.rebuilding;
  if (state.reading === "reading" && !state.status.syncing) {
    setReading("done");
    ended = true;
  }
  return ended;
}

/** Redraws whatever is showing, now or when the window is shown again. */
function redrawAnyPage() {
  if (document.hidden) redrawOnShow = true;
  else scheduler.refresh();
}

/** A check ended: once "Checking…" has been up long enough, reads where things stand now. */
const checkDone = (after: () => void) =>
  setTimeout(() => {
    const wasRebuilding = state.status.rebuilding;
    void api
      .syncStatus()
      .then(setStatus)
      .catch((e) => {
        report(e);
        setStatus({ ...state.status, syncing: false });
      })
      .then(() => {
        if (endedSomething(wasRebuilding)) {
          paintStatus();
          redrawAnyPage();
        } else after();
        pollRebuild();
      });
  }, checkingLeft());

/** While scores are rebuilt the app can't say when it ends: ask again every few seconds until it has. */
let polling = false;
function pollRebuild() {
  if (polling || !state.status.rebuilding) return;
  polling = true;
  const tick = () => {
    const was = state.status.rebuilding;
    void api
      .syncStatus()
      .then(setStatus)
      .catch(report)
      .then(() => {
        paintStatus();
        if (state.status.rebuilding) return void setTimeout(tick, 2000);
        polling = false;
        if (endedSomething(was)) redrawAnyPage();
      });
  };
  setTimeout(tick, 2000);
}

const loadUpdate = () =>
  api
    .updateStatus()
    .then((u) => {
      setUpdate(u);
      paintUpdate();
    })
    .catch(report);

if (hasOverlayTitleBar(!demo, isGlance, navigator.userAgent)) document.documentElement.classList.add("mac-bar");
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
  .catch(report)
  .finally(() => {
    void render();
    pollRebuild();
  });
if (!isGlance) void loadUpdate();
if (!demo) {
  if (!isGlance) void listen("update-changed", loadUpdate);
  // The glance's gear opens the scorecard window on Settings.
  if (!isGlance)
    void listen<string>("open-tab", (e) => {
      if (!isTab(e.payload)) return;
      setTab(e.payload);
      void render();
    });
  // A check starting changes only the status line; a finished one may change the numbers.
  void listen("sync-started", () => {
    setSyncing();
    paintStatus();
    // The window may have loaded just before the pass set this, and its numbers are about to change.
    void api
      .syncStatus()
      .then((s) => {
        const started = s.rebuilding && !state.status.rebuilding;
        setStatus(s);
        if (!started) return;
        void render();
        pollRebuild();
      })
      .catch(report);
  });
  void listen("stats-changed", () => checkDone(refreshIfShown));
  // A first read that found nothing ends here too, and its page says it's still reading.
  void listen("sync-idle", () =>
    checkDone(() => {
      paintStatus();
      if (state.reading === "reading") void render();
    }),
  );
  void listen("sync-failed", () => checkDone(paintStatus));
}
document.addEventListener("visibilitychange", () => {
  if (document.hidden || !redrawOnShow) return;
  redrawOnShow = false;
  scheduler.refresh();
});
// Keep "Checked 3 min ago" honest without re-querying stats.
setInterval(() => state.current && paintStatus(), 30_000);
window.addEventListener("hashchange", () => location.reload());
// Dev builds only: lets a test simulate a sync finishing in the background.
if (import.meta.env.DEV) Object.assign(window, { __simulateSync: refresh });
