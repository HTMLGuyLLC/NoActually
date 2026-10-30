// When to redraw. Background syncs never redraw under someone who is using a control; the redraw
// waits ("deferred") until they're done. Overlapping redraws collapse into one more pass.

/** True while the person is mid-action: a popover or dialog is open, a confirm is armed, or a field has focus. */
export function isUserBusy(root: HTMLElement, doc: Document = document): boolean {
  if (doc.querySelector(".picker-pop, dialog[open]")) return true;
  if (root.querySelector('[data-confirm="yes"]')) return true;
  const el = doc.activeElement;
  return !!el && root.contains(el) && el.matches("input, select, textarea");
}

export type Scheduler = {
  /** Redraw now (a person asked for it). Includes anything that was deferred. Resolves once the view is drawn,
   * even if a draw was already running (the call then waits for the pass queued behind it). */
  render(): Promise<void>;
  /** Redraw because data changed in the background: now, or once the person is done. */
  refresh(): void;
  /** The person finished with a control: apply a deferred redraw, if any. */
  flush(): void;
  readonly deferred: boolean;
};

export function createScheduler(opts: { isBusy: () => boolean; draw: () => Promise<void> }): Scheduler {
  let deferred = false;
  /** The draws in flight (the running one and the one pass queued behind it), or null when idle. */
  let running: Promise<void> | null = null;
  let again = false;

  async function passes(): Promise<void> {
    try {
      do {
        again = false;
        await opts.draw();
      } while (again);
    } finally {
      running = null;
    }
  }

  // A call during a draw asks for one more pass and waits for it, so what the caller does next sees the new view.
  function render(): Promise<void> {
    deferred = false;
    if (running) {
      again = true;
      return running;
    }
    running = passes();
    return running;
  }

  return {
    render,
    refresh() {
      if (opts.isBusy()) deferred = true;
      else void render();
    },
    flush() {
      if (deferred && !opts.isBusy()) void render();
    },
    get deferred() {
      return deferred;
    },
  };
}
