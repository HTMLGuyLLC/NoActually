// The model picker's popover: a searchable listbox anchored to the headline's model name. It lives
// on <body> so the card can't clip it. Follows the ARIA combobox pattern: focus stays in the search
// field and the highlighted option is announced through aria-activedescendant.
import { html, num } from "../html";
import { catchRate, matchPickItems, type PickItem } from "./picker";

export type PickerOptions = {
  items: PickItem[];
  /** Whether this item is the current scope (gets a check). */
  isSelected: (item: PickItem) => boolean;
  onChoose: (item: PickItem) => void;
  /** The popover closed without a choice. `flush` is false when it closed for a redraw. */
  onClose: (flush: boolean) => void;
};

const LIST_ID = "pick-list";
const optionId = (n: number) => `pick-opt-${n}`;

let active: ((flush: boolean) => void) | null = null;

export const pickerOpen = (): boolean => active !== null;

/** Closes the open picker, if any. `flush: false` skips onClose's redraw (the caller is about to draw). */
export function closePicker(flush = true): void {
  active?.(flush);
}

export function openPicker(trigger: HTMLElement, opts: PickerOptions): HTMLElement {
  closePicker(false);
  const pop = document.createElement("div");
  pop.className = "picker-pop";
  pop.innerHTML = html`<input class="pick-search" type="search" role="combobox" aria-expanded="true" aria-controls="${LIST_ID}"
      aria-autocomplete="list" placeholder="Search models, e.g. opus 5.5" aria-label="Search models" autocomplete="off" spellcheck="false" />
    <ul class="pick-list" id="${LIST_ID}" role="listbox" aria-label="Models"></ul>`.value;
  document.body.appendChild(pop);
  trigger.setAttribute("aria-expanded", "true");

  const input = pop.querySelector("input");
  const list = pop.querySelector("ul");
  if (!input || !list) throw new Error("Picker markup is missing its parts");
  let shown: PickItem[] = opts.items;
  let current = Math.max(
    0,
    opts.items.findIndex((i) => opts.isSelected(i)),
  );

  const highlight = (n: number, scroll: boolean) => {
    list.querySelector(".active")?.classList.remove("active");
    const li = list.querySelector<HTMLElement>(`#${optionId(n)}`);
    current = n;
    if (!li) {
      input.removeAttribute("aria-activedescendant");
      return;
    }
    li.classList.add("active");
    input.setAttribute("aria-activedescendant", li.id);
    if (scroll) li.scrollIntoView?.({ block: "nearest" });
  };

  const draw = () => {
    const searching = input.value.trim().length > 0;
    shown = matchPickItems(opts.items, input.value);
    current = Math.min(current, Math.max(0, shown.length - 1));
    list.innerHTML = (
      shown.length
        ? html`${shown.map(
            (i, n) => html`<li role="option" id="${optionId(n)}" data-n="${n}" class="d${searching ? 0 : i.depth}" aria-selected="${String(opts.isSelected(i))}">
              <span class="pick-name"><b>${i.label}</b>${searching || i.depth === 0 ? html`<span class="pick-path">${i.path}</span>` : ""}</span>
              <span class="pick-count"><b>${num(i.catches)}</b> catches · ${catchRate(i.catches, i.messages)}</span></li>`,
          )}`
        : html`<li class="pick-empty" role="presentation">No model matches “${input.value}”</li>`
    ).value;
    highlight(current, true);
  };

  // Keep the popover under its trigger if the page itself scrolls or resizes.
  const place = () => {
    const r = trigger.getBoundingClientRect();
    pop.style.top = `${Math.round(r.bottom + 8)}px`;
    pop.style.left = `${Math.round(Math.max(12, Math.min(r.left, window.innerWidth - pop.offsetWidth - 12)))}px`;
  };
  const inPop = (t: EventTarget | null) => t instanceof Node && pop.contains(t);
  const onPageScroll = (e: Event) => {
    if (!inPop(e.target)) place();
  };
  const outside = (e: MouseEvent) => {
    if (!inPop(e.target) && !(e.target instanceof Node && trigger.contains(e.target))) close(true);
  };
  // Focus moving anywhere else (tabbing away) closes it, so a redraw waiting on the picker isn't held off forever.
  const onFocusIn = (e: FocusEvent) => {
    if (!inPop(e.target) && e.target !== trigger) close(true);
  };

  function close(flush: boolean) {
    if (active !== close) return;
    active = null;
    pop.remove();
    trigger.setAttribute("aria-expanded", "false");
    document.removeEventListener("mousedown", outside, true);
    document.removeEventListener("focusin", onFocusIn, true);
    window.removeEventListener("scroll", onPageScroll, true);
    window.removeEventListener("resize", place);
    opts.onClose(flush);
  }
  active = close;

  const choose = (i: PickItem | undefined) => {
    if (!i) return;
    trigger.focus(); // the redraw puts focus back on the (new) trigger
    close(false);
    opts.onChoose(i);
  };

  input.addEventListener("input", () => {
    current = 0;
    draw();
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") highlight(Math.min(shown.length - 1, current + 1), true);
    else if (e.key === "ArrowUp") highlight(Math.max(0, current - 1), true);
    else if (e.key === "Home" && !input.value) highlight(0, true);
    else if (e.key === "End" && !input.value) highlight(shown.length - 1, true);
    else if (e.key === "Enter") choose(shown[current]);
    else if (e.key === "Escape") {
      trigger.focus();
      close(true);
    } else return;
    e.preventDefault();
  });
  const optionAt = (t: EventTarget | null) => {
    const n = t instanceof Element ? t.closest<HTMLElement>("[data-n]")?.dataset.n : undefined;
    return n === undefined ? null : Number(n);
  };
  // Hovering only moves the highlight: no rebuild, no scrolling, so the wheel isn't fought.
  list.addEventListener("mousemove", (e) => {
    const n = optionAt(e.target);
    if (n !== null && n !== current) highlight(n, false);
  });
  // Clicking an option must not take focus from the search field (that would close the picker).
  list.addEventListener("mousedown", (e) => e.preventDefault());
  list.addEventListener("click", (e) => {
    const n = optionAt(e.target);
    if (n !== null) choose(shown[n]);
  });
  window.addEventListener("scroll", onPageScroll, true);
  window.addEventListener("resize", place);
  document.addEventListener("mousedown", outside, true);
  place();
  draw();
  input.focus();
  document.addEventListener("focusin", onFocusIn, true); // after focusing the field, which is inside
  return pop;
}
