// The prompt checker's live result: grades the draft with the Rust rules as you type. The draft
// lives only in memory (state.draft); nothing is saved or logged.
import { api, demo } from "../api";
import { state } from "../state";
import { checkResultView } from "../views/learn";

let timer: ReturnType<typeof setTimeout> | undefined;
/** The last summary announced, so screen readers hear the grade only when it changes. */
let announced = "";
/** Each check gets a number, so a slow answer for an older draft never overwrites a newer one. */
let seq = 0;

/** Grades the current draft into #check-result. */
export async function refreshCheck(root: ParentNode): Promise<void> {
  const out = root.querySelector("#check-result");
  const meta = state.meta;
  if (!out || !meta || demo) return;
  const text = state.draft;
  const mine = ++seq;
  if (!text.trim()) {
    announced = ""; // a fresh draft is announced again, even at the same grade
    out.innerHTML = checkResultView(meta, null, true).value;
    return;
  }
  try {
    const c = await api.checkPrompt(text);
    if (mine !== seq) return;
    out.innerHTML = checkResultView(meta, c, false).value;
    const count = meta.signals.filter((_, i) => (c.signals & (1 << i)) !== 0).length;
    const summary = `${meta.grades.find((g) => g.id === c.grade)?.title ?? c.grade}, carries ${count} of ${meta.signals.length}.`;
    const status = root.querySelector("#check-status");
    if (status && summary !== announced) {
      announced = summary;
      status.textContent = summary;
    }
  } catch {
    if (mine === seq) out.textContent = "Couldn't check that draft.";
  }
}

/** Re-grades shortly after typing stops. */
export function scheduleCheck(root: ParentNode, delay = 150): void {
  clearTimeout(timer);
  timer = setTimeout(() => void refreshCheck(root), delay);
}
