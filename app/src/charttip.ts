// The app's tooltip for chart marks. CSS draws every other tooltip from `data-tip`, but a
// pseudo-element can't hang off an SVG shape, so bars and dots share this one card. Hover shows
// it; a click pins it (so it works by tap too) until you click elsewhere or press Escape. Screen
// readers get each chart's text summary instead.

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
  document.addEventListener("click", (e) => {
    const mark = markOf(e.target);
    if (!mark || mark === pinned) return hide();
    hide();
    pinned = mark;
    mark.classList.add("tip-pinned");
    show(mark, e.clientX, e.clientY);
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !tip.hidden) hide();
  });
  // A redraw can remove the mark under the pointer (or the pinned one) without a pointerout.
  new MutationObserver(() => {
    if (pinned && !pinned.isConnected) hide();
    else if (!pinned && !tip.hidden && !root.querySelector("svg [data-tip]:hover")) tip.hidden = true;
  }).observe(root, { childList: true, subtree: true });
}
