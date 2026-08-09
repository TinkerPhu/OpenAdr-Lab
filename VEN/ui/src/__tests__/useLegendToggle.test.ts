import { describe, it, expect } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useLegendToggle } from "../components/charts/useLegendToggle";

describe("useLegendToggle", () => {
  it("starts with nothing hidden", () => {
    const { result } = renderHook(() => useLegendToggle());
    expect(result.current.isHidden("power")).toBe(false);
    expect(result.current.isHidden("cost")).toBe(false);
  });

  it("toggling a key hides it, toggling again shows it", () => {
    const { result } = renderHook(() => useLegendToggle());
    act(() => result.current.toggle("power"));
    expect(result.current.isHidden("power")).toBe(true);
    act(() => result.current.toggle("power"));
    expect(result.current.isHidden("power")).toBe(false);
  });

  it("toggles each key independently", () => {
    const { result } = renderHook(() => useLegendToggle());
    act(() => result.current.toggle("power"));
    expect(result.current.isHidden("power")).toBe(true);
    expect(result.current.isHidden("cost")).toBe(false);
  });
});
