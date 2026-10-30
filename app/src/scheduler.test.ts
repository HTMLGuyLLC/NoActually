// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { must } from "./dom";
import { createScheduler, isUserBusy } from "./scheduler";

describe("createScheduler", () => {
  let busy = false;
  const draw = vi.fn(async () => {});
  const make = () => createScheduler({ isBusy: () => busy, draw });
  beforeEach(() => {
    busy = false;
    draw.mockReset();
    draw.mockImplementation(async () => {});
  });

  it("redraws a background refresh right away when the person is idle", () => {
    make().refresh();
    expect(draw).toHaveBeenCalledTimes(1);
  });

  it("defers a refresh while the person is busy, and flushes once they're done", () => {
    const s = make();
    busy = true;
    s.refresh();
    expect(draw).not.toHaveBeenCalled();
    expect(s.deferred).toBe(true);
    s.flush(); // still busy: keeps waiting
    expect(draw).not.toHaveBeenCalled();
    busy = false;
    s.flush();
    expect(draw).toHaveBeenCalledTimes(1);
    expect(s.deferred).toBe(false);
  });

  it("does nothing on flush when nothing was deferred", () => {
    make().flush();
    expect(draw).not.toHaveBeenCalled();
  });

  it("counts a requested render as applying the deferred one", async () => {
    const s = make();
    busy = true;
    s.refresh();
    busy = false;
    await s.render();
    s.flush();
    expect(draw).toHaveBeenCalledTimes(1);
  });

  it("collapses renders that overlap a slow one into a single extra pass", async () => {
    let finish = () => {};
    draw.mockImplementationOnce(
      () =>
        new Promise<void>((r) => {
          finish = r;
        }),
    );
    const s = make();
    const first = s.render();
    void s.render();
    void s.render();
    expect(draw).toHaveBeenCalledTimes(1);
    finish();
    await first;
    expect(draw).toHaveBeenCalledTimes(2);
  });

  it("resolves an overlapping render only after the queued pass has drawn", async () => {
    let finish = () => {};
    let drawn = 0;
    draw.mockImplementationOnce(
      () =>
        new Promise<void>((r) => {
          finish = r;
        }),
    );
    draw.mockImplementation(async () => {
      drawn++;
    });
    const s = make();
    void s.render();
    const second = s.render().then(() => drawn);
    finish();
    expect(await second).toBe(1);
  });

  it("keeps working after a draw throws", async () => {
    draw.mockRejectedValueOnce(new Error("boom"));
    const s = make();
    await expect(s.render()).rejects.toThrow("boom");
    await s.render();
    expect(draw).toHaveBeenCalledTimes(2);
  });
});

describe("isUserBusy", () => {
  beforeEach(() => {
    document.body.innerHTML = `<div id="app"><input id="field"/><button id="btn">b</button></div>`;
  });
  const root = () => must("#app");

  it("is idle with focus on a button", () => {
    document.getElementById("btn")?.focus();
    expect(isUserBusy(root())).toBe(false);
  });

  it("is busy while a field in the app has focus", () => {
    document.getElementById("field")?.focus();
    expect(isUserBusy(root())).toBe(true);
  });

  it("is busy while the picker or a dialog is open", () => {
    const pop = document.createElement("div");
    pop.className = "picker-pop";
    document.body.append(pop);
    expect(isUserBusy(root())).toBe(true);
    pop.remove();
    const dlg = document.createElement("dialog");
    dlg.setAttribute("open", "");
    document.body.append(dlg);
    expect(isUserBusy(root())).toBe(true);
  });

  it("is busy while a delete confirm is armed", () => {
    document.getElementById("btn")?.setAttribute("data-confirm", "yes");
    expect(isUserBusy(root())).toBe(true);
  });
});
