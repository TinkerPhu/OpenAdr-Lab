import { describe, it, expect, vi } from "vitest";
import { DEFAULT_VENS, mergeVens, fetchDiscoveredVens } from "../api/venRegistry";

describe("mergeVens", () => {
  it("appends discovered names not in the defaults as /api/dyn entries", () => {
    const merged = mergeVens(DEFAULT_VENS, [
      { venName: "fleet-ven-000" },
      { venName: "fleet-ven-001" },
    ]);
    expect(merged).toHaveLength(DEFAULT_VENS.length + 2);
    expect(merged[DEFAULT_VENS.length]).toEqual({
      label: "fleet-ven-000",
      url: "/api/dyn/fleet-ven-000",
      venName: "fleet-ven-000",
    });
    expect(merged[DEFAULT_VENS.length + 1].url).toBe("/api/dyn/fleet-ven-001");
  });

  it("keeps the defaults first and untouched", () => {
    const merged = mergeVens(DEFAULT_VENS, [{ venName: "fleet-ven-000" }]);
    expect(merged.slice(0, DEFAULT_VENS.length)).toEqual(DEFAULT_VENS);
  });

  it("drops discovered names that are already defaults", () => {
    const merged = mergeVens(DEFAULT_VENS, [
      { venName: "ven-1" },
      { venName: "ven-2" },
      { venName: "fleet-ven-000" },
    ]);
    expect(merged).toHaveLength(DEFAULT_VENS.length + 1);
    expect(merged.filter((v) => v.venName === "ven-1")).toHaveLength(1);
  });

  it("dedupes repeated discovered names and sorts extras", () => {
    const merged = mergeVens(DEFAULT_VENS, [
      { venName: "fleet-ven-002" },
      { venName: "fleet-ven-000" },
      { venName: "fleet-ven-002" },
    ]);
    const extras = merged.slice(DEFAULT_VENS.length).map((v) => v.venName);
    expect(extras).toEqual(["fleet-ven-000", "fleet-ven-002"]);
  });

  it("returns just the defaults for an empty discovery list", () => {
    expect(mergeVens(DEFAULT_VENS, [])).toEqual(DEFAULT_VENS);
  });

  it("labels persona-tagged fleet VENs (WP4.5)", () => {
    const merged = mergeVens(DEFAULT_VENS, [
      { venName: "fleet-ven-000", persona: "eco" },
      { venName: "fleet-ven-001" },
    ]);
    expect(merged[DEFAULT_VENS.length].label).toBe("fleet-ven-000 (eco)");
    expect(merged[DEFAULT_VENS.length + 1].label).toBe("fleet-ven-001");
  });

  it("routes a VEN carrying DASHBOARD_URL directly to that origin (BL-41)", () => {
    const merged = mergeVens(DEFAULT_VENS, [
      { venName: "ven-4", dashboardUrl: "http://192.168.1.104:8211" },
    ]);
    expect(merged[DEFAULT_VENS.length]).toEqual({
      label: "ven-4",
      url: "http://192.168.1.104:8211",
      venName: "ven-4",
    });
  });

  it("keeps the /api/dyn route for a VEN without DASHBOARD_URL (BL-41 regression)", () => {
    const merged = mergeVens(DEFAULT_VENS, [{ venName: "ven-4" }]);
    expect(merged[DEFAULT_VENS.length].url).toBe("/api/dyn/ven-4");
  });

  it("sorts extras by natural (numeric) order, not lexicographic string order", () => {
    // Node2 fleet ven-4..ven-13: plain string sort puts "ven-10".."ven-13"
    // before "ven-4".."ven-9" (lexicographic: "1" < "4"). Natural sort must
    // compare the numeric suffix numerically so the dropdown reads 4..13.
    const merged = mergeVens(DEFAULT_VENS, [
      { venName: "ven-13" },
      { venName: "ven-4" },
      { venName: "ven-10" },
      { venName: "ven-9" },
      { venName: "ven-5" },
    ]);
    const extras = merged.slice(DEFAULT_VENS.length).map((v) => v.venName);
    expect(extras).toEqual(["ven-4", "ven-5", "ven-9", "ven-10", "ven-13"]);
  });

  it("combines PERSONA label with a DASHBOARD_URL route (BL-41)", () => {
    const merged = mergeVens(DEFAULT_VENS, [
      { venName: "ven-4", persona: "eco", dashboardUrl: "http://192.168.1.104:8211" },
    ]);
    expect(merged[DEFAULT_VENS.length]).toEqual({
      label: "ven-4 (eco)",
      url: "http://192.168.1.104:8211",
      venName: "ven-4",
    });
  });
});

describe("fetchDiscoveredVens", () => {
  function fetchStub(registry: unknown, healthyNames: string[], healthyUrls: string[] = []) {
    return vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "/api/vens-registry") {
        return { ok: true, json: async () => registry } as Response;
      }
      const m = url.match(/^\/api\/dyn\/([^/]+)\/health$/);
      if (m) {
        return { ok: healthyNames.includes(m[1]) } as Response;
      }
      if (url.endsWith("/health")) {
        return { ok: healthyUrls.includes(url.replace(/\/health$/, "")) } as Response;
      }
      throw new Error(`unexpected fetch: ${url}`);
    });
  }

  it("returns only registered, non-default, health-responding names", async () => {
    const registry = [
      { venName: "ven-1" }, // default — never probed or returned
      { venName: "fleet-ven-000" }, // healthy
      { venName: "fleet-ven-001" }, // registered but down (purged fleet)
    ];
    const fetchFn = fetchStub(registry, ["fleet-ven-000"]);

    const names = await fetchDiscoveredVens(fetchFn as unknown as typeof fetch);

    expect(names).toEqual([{ venName: "fleet-ven-000", persona: undefined }]);
    const probed = fetchFn.mock.calls.map((c) => String(c[0]));
    expect(probed).not.toContain("/api/dyn/ven-1/health");
  });

  it("treats a probe that throws (connection refused) as unhealthy", async () => {
    const fetchFn = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url === "/api/vens-registry") {
        return {
          ok: true,
          json: async () => [{ venName: "fleet-ven-000" }],
        } as Response;
      }
      throw new Error("connection refused");
    });

    const names = await fetchDiscoveredVens(fetchFn as unknown as typeof fetch);
    expect(names).toEqual([]);
  });

  it("reads the WP4.5 PERSONA attribute into the persona field", async () => {
    const fetchFn = fetchStub(
      [{ venName: "fleet-ven-000", attributes: [{ type: "PERSONA", values: ["eco"] }] }],
      ["fleet-ven-000"],
    );
    const names = await fetchDiscoveredVens(fetchFn as unknown as typeof fetch);
    expect(names).toEqual([{ venName: "fleet-ven-000", persona: "eco" }]);
  });

  it("rejects when the registry endpoint itself fails", async () => {
    const fetchFn = vi.fn(async () => ({ ok: false, status: 502 }) as Response);
    await expect(
      fetchDiscoveredVens(fetchFn as unknown as typeof fetch),
    ).rejects.toThrow(/502/);
  });

  it("ignores registry rows without a venName", async () => {
    const fetchFn = fetchStub([{ id: "x" }, { venName: "fleet-ven-000" }], [
      "fleet-ven-000",
    ]);
    const names = await fetchDiscoveredVens(fetchFn as unknown as typeof fetch);
    expect(names).toEqual([{ venName: "fleet-ven-000", persona: undefined }]);
  });

  it("reads the DASHBOARD_URL attribute and probes health there directly (BL-41)", async () => {
    const fetchFn = fetchStub(
      [
        {
          venName: "ven-4",
          attributes: [{ type: "DASHBOARD_URL", values: ["http://192.168.1.104:8211"] }],
        },
      ],
      [],
      ["http://192.168.1.104:8211"],
    );
    const names = await fetchDiscoveredVens(fetchFn as unknown as typeof fetch);
    expect(names).toEqual([
      { venName: "ven-4", persona: undefined, dashboardUrl: "http://192.168.1.104:8211" },
    ]);
    const probed = fetchFn.mock.calls.map((c) => String(c[0]));
    expect(probed).toContain("http://192.168.1.104:8211/health");
    expect(probed).not.toContain("/api/dyn/ven-4/health");
  });

  it("keeps PERSONA and DASHBOARD_URL working together (BL-41)", async () => {
    const fetchFn = fetchStub(
      [
        {
          venName: "ven-4",
          attributes: [
            { type: "PERSONA", values: ["eco"] },
            { type: "DASHBOARD_URL", values: ["http://192.168.1.104:8211"] },
          ],
        },
      ],
      [],
      ["http://192.168.1.104:8211"],
    );
    const names = await fetchDiscoveredVens(fetchFn as unknown as typeof fetch);
    expect(names).toEqual([
      { venName: "ven-4", persona: "eco", dashboardUrl: "http://192.168.1.104:8211" },
    ]);
  });

  it("falls back to the /api/dyn probe when DASHBOARD_URL is absent (BL-41 regression)", async () => {
    const fetchFn = fetchStub([{ venName: "fleet-ven-000" }], ["fleet-ven-000"]);
    const names = await fetchDiscoveredVens(fetchFn as unknown as typeof fetch);
    expect(names).toEqual([
      { venName: "fleet-ven-000", persona: undefined, dashboardUrl: undefined },
    ]);
  });
});
