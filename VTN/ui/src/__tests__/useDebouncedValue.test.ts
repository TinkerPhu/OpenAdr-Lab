import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDebouncedValue } from "../utils/useDebouncedValue";

describe("useDebouncedValue", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("starts with the value it is given", () => {
    const { result } = renderHook(() => useDebouncedValue("a", 100));
    expect(result.current).toBe("a");
  });

  it("holds the old value until the input has been quiet for the delay", () => {
    const { result, rerender } = renderHook(({ v }) => useDebouncedValue(v, 100), {
      initialProps: { v: "a" },
    });
    rerender({ v: "b" });
    act(() => vi.advanceTimersByTime(99));
    expect(result.current).toBe("a");
    act(() => vi.advanceTimersByTime(1));
    expect(result.current).toBe("b");
  });

  it("restarts the wait on every change, so a stream of changes yields one result", () => {
    const { result, rerender } = renderHook(({ v }) => useDebouncedValue(v, 100), {
      initialProps: { v: 0 },
    });
    for (const v of [1, 2, 3]) {
      rerender({ v });
      act(() => vi.advanceTimersByTime(60));
    }
    expect(result.current).toBe(0);
    act(() => vi.advanceTimersByTime(100));
    expect(result.current).toBe(3);
  });
});
