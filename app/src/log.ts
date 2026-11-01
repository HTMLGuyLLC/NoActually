// Where a failure the person can't see goes: the console and the app's log file.
import { api } from "./api";
import { errorText } from "./format";

/** The log keeps a line's worth, not a page: a stack can be long. */
const MAX_LOG_CHARS = 2000;

/** The error and, when it has one, its stack. */
export function errorDetail(e: unknown): string {
  const text = e instanceof Error && e.stack ? `${errorText(e)}\n${e.stack}` : errorText(e);
  return text.length > MAX_LOG_CHARS ? `${text.slice(0, MAX_LOG_CHARS)}…` : text;
}

/** Logs a failure, in the console and in the app's log file (where a failure to write it is ignored). */
export function report(e: unknown): void {
  console.error("Action failed:", e);
  try {
    void Promise.resolve(api.logError(errorDetail(e))).catch(() => {});
  } catch {
    // No way to write the log; the console has it.
  }
}

/** Reports what nothing else caught: an error thrown outside any handler, and a promise nobody awaited. */
export function bindGlobalErrors(target: Pick<Window, "addEventListener"> = window): void {
  target.addEventListener("error", (e) => report(e.error ?? e.message));
  target.addEventListener("unhandledrejection", (e) => report(e.reason));
}
