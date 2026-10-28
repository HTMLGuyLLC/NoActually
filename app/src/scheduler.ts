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
  /** Redraw now (a person asked for it). Includes anything that was deferred. */
  render(): Promise<void>;
  /** Redraw because data changed in the background: now, or once the person is done. */
  refresh(): void;
  /** The person finished with a control: apply a deferred redraw, if any. */
  flush(): void;
  readonly deferred: boolean;
};

export function createScheduler(opts: { isBusy: () => boolean; draw: () => Promise<void> }): Scheduler {
  let deferred = false;
  let rendering = false;
  let again = false;

  async function render(): Promise<void> {
    deferred = false;
    if (rendering) {
      again = true;
      return;
    }
    rendering = true;
    try {
      await opts.draw();
    } finally {
      rendering = false;
      if (again) {
        again = false;
        await render();
      }
    }
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
