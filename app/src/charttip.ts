// The app's tooltip for chart marks. CSS draws every other tooltip from `data-tip`, but a
// pseudo-element can't hang off an SVG shape, so bars and dots share this one card. Hover shows
// it; a click pins it (so it works by tap too) until you click elsewhere or press Escape. Keyboard
// focus on a mark shows it too. Marks carry the same text as their aria-label, and screen readers
// also get each chart's text summary. A chart is one tab stop: the arrow keys, Home and End move along
// its marks, and the mark you leave on is the one Tab returns to.

/** Space between the pointer and the card, and between the card and the window's edge. */
const GAP = 12;

/** Shows `data-tip` on SVG marks inside `root` as the app's tooltip card. */
export function bindChartTips(root: HTMLElement): void {
  const tip = document.createElement("div");
  tip.className = "chart-tip";
  tip.setAttribute("aria-hidden", "true");
  tip.hidden = true;
  document.body.append(tip);
  let pinned: Element | null = null;

  const markOf = (t: EventTarget | null) => (t instanceof Element ? t.closest<SVGElement>("svg [data-tip]") : null);
  /** The marks of the chart `mark` is in, in the order they are drawn. */
  const marksOf = (mark: Element) => [...(mark.closest("svg")?.querySelectorAll<SVGElement>("[data-tip][tabindex]") ?? [])];
  const place = (x: number, y: number) => {
    const { width, height } = tip.getBoundingClientRect();
    const left = Math.min(Math.max(GAP, x - width / 2), window.innerWidth - width - GAP);
    const top = y - height - GAP < GAP ? y + GAP * 2 : y - height - GAP;
    tip.style.transform = `translate(${Math.round(left)}px, ${Math.round(top)}px)`;
  };
  const show = (mark: SVGElement, x: number, y: number) => {
    tip.textContent = mark.dataset.tip ?? "";
    tip.hidden = false;
    place(x, y);
  };
  const showAt = (mark: SVGElement) => {
    const r = mark.getBoundingClientRect();
    show(mark, r.left + r.width / 2, r.top);
  };
  const hide = () => {
    pinned?.classList.remove("tip-pinned");
    pinned = null;
    tip.hidden = true;
  };

  root.addEventListener("pointerover", (e) => {
    const mark = markOf(e.target);
    if (mark && !pinned) show(mark, e.clientX, e.clientY);
  });
  root.addEventListener("pointermove", (e) => {
    if (!pinned && !tip.hidden && markOf(e.target)) place(e.clientX, e.clientY);
  });
  root.addEventListener("pointerout", (e) => {
    if (!pinned && !markOf(e.relatedTarget)) tip.hidden = true;
  });
  root.addEventListener("focusin", (e) => {
    const mark = markOf(e.target);
    if (!mark) return;
    // The focused mark becomes the chart's tab stop.
    for (const m of marksOf(mark)) m.setAttribute("tabindex", m === mark ? "0" : "-1");
    if (!pinned) showAt(mark);
  });
  root.addEventListener("keydown", (e) => {
    const mark = markOf(e.target);
    if (!mark || e.altKey || e.ctrlKey || e.metaKey) return;
    const marks = marksOf(mark);
    const at = marks.indexOf(mark);
    const next =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? at + 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? at - 1
          : e.key === "Home"
            ? 0
            : e.key === "End"
              ? marks.length - 1
              : null;
    if (next === null) return;
    e.preventDefault();
    marks[Math.min(marks.length - 1, Math.max(0, next))]?.focus();
  });
  root.addEventListener("focusout", (e) => {
    if (!pinned && !markOf(e.relatedTarget)) tip.hidden = true;
  });
  document.addEventListener("click", (e) => {
    const mark = markOf(e.target);
    if (!mark || mark === pinned) return hide();
    hide();
    pinned = mark;
    mark.classList.add("tip-pinned");
    show(mark, e.clientX, e.clientY);
  });
  // Escape also dismisses the CSS tooltips (`[data-tip]::after`), which can't be closed from
  // script: a class on <html> hides them until the pointer moves or focus lands somewhere new.
  const html = document.documentElement;
  const wake = () => html.classList.remove("tips-off");
  document.addEventListener("keydown", (e) => {
    if (e.key !== "Escape") return;
    html.classList.add("tips-off");
    if (!tip.hidden) hide();
  });
  document.addEventListener("pointermove", wake);
  document.addEventListener("focusin", wake);
  // A redraw can remove the mark under the pointer (or the pinned one) without a pointerout.
  new MutationObserver(() => {
    if (pinned && !pinned.isConnected) hide();
    else if (!pinned && !tip.hidden && !root.querySelector("svg [data-tip]:hover, svg [data-tip]:focus")) tip.hidden = true;
  }).observe(root, { childList: true, subtree: true });
}
