import { describe, it, expect } from "vitest";
import { compareVenNames, byVenName } from "../utils/venOrder";

describe("compareVenNames", () => {
  it("counts the way a person does, not the way bytes do", () => {
    const names = ["ven-10", "ven-2", "ven-1", "ven-20", "ven-3", "ven-11"];
    expect([...names].sort(compareVenNames)).toEqual([
      "ven-1",
      "ven-2",
      "ven-3",
      "ven-10",
      "ven-11",
      "ven-20",
    ]);
  });

  it("is the fix for what a plain sort does", () => {
    const names = ["ven-10", "ven-2"];
    // The defect, pinned: without this comparator ven-10 leads.
    expect([...names].sort()).toEqual(["ven-10", "ven-2"]);
    expect([...names].sort(compareVenNames)).toEqual(["ven-2", "ven-10"]);
  });

  it("needs no knowledge of the ven-N shape", () => {
    const names = ["site-12-north", "site-2-north", "site-2-east"];
    expect([...names].sort(compareVenNames)).toEqual([
      "site-2-east",
      "site-2-north",
      "site-12-north",
    ]);
  });

  it("treats a zero-padded name as the same number", () => {
    expect(compareVenNames("ven-007", "ven-7")).toBe(0);
  });

  it("orders a name with no digits by text", () => {
    expect([...["gamma", "alpha", "beta"]].sort(compareVenNames)).toEqual([
      "alpha",
      "beta",
      "gamma",
    ]);
  });

  it("puts a plain name before its suffixed sibling", () => {
    expect([...["ven-1a", "ven-1"]].sort(compareVenNames)).toEqual(["ven-1", "ven-1a"]);
  });
});

describe("byVenName", () => {
  it("sorts a copy, leaving the caller's array untouched", () => {
    const rows = [{ venName: "ven-10" }, { venName: "ven-2" }];
    const sorted = byVenName(rows, (r) => r.venName);
    expect(sorted.map((r) => r.venName)).toEqual(["ven-2", "ven-10"]);
    // React state must never be sorted in place.
    expect(rows.map((r) => r.venName)).toEqual(["ven-10", "ven-2"]);
  });
});
