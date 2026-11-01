// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({ logError: vi.fn() }));
vi.mock("./api", async (importOriginal) => ({ ...(await importOriginal<typeof import("./api")>()), api, demo: false }));

import { bindGlobalErrors, errorDetail, report } from "./log";

beforeEach(() => {
  api.logError.mockReset();
  api.logError.mockResolvedValue(null);
  vi.spyOn(console, "error").mockImplementation(() => {});
});

describe("errorDetail", () => {
  it("keeps the stack of an Error, and the text of anything else", () => {
    const e = new Error("boom");
    expect(errorDetail(e)).toContain("boom");
    expect(errorDetail(e)).toContain("log.test.ts");
    expect(errorDetail("plain")).toBe("plain");
  });

  it("caps a long stack", () => {
    const e = new Error("x");
    e.stack = "s".repeat(5000);
    expect(errorDetail(e).length).toBeLessThanOrEqual(2001 + 5);
    expect(errorDetail(e).endsWith("…")).toBe(true);
  });
});

describe("report", () => {
  it("writes to the console and the log, with the stack", () => {
    report(new Error("boom"));
    expect(console.error).toHaveBeenCalled();
    expect(api.logError).toHaveBeenCalledWith(expect.stringContaining("boom"));
  });

  it("survives the log failing, in either way", () => {
    api.logError.mockRejectedValueOnce("disk full");
    expect(() => report("a")).not.toThrow();
    api.logError.mockImplementationOnce(() => {
      throw new Error("no command");
    });
    expect(() => report("b")).not.toThrow();
  });
});

describe("bindGlobalErrors", () => {
  it("logs an uncaught error and an unhandled rejection", () => {
    const target = new EventTarget();
    bindGlobalErrors(target as unknown as Window);
    const err = new Error("late");
    target.dispatchEvent(Object.assign(new Event("error"), { error: err, message: "late" }));
    expect(api.logError).toHaveBeenLastCalledWith(expect.stringContaining("late"));
    target.dispatchEvent(Object.assign(new Event("unhandledrejection"), { reason: "nobody caught this" }));
    expect(api.logError).toHaveBeenLastCalledWith("nobody caught this");
  });
});
