/**
 * PV irradiance one-shot inject — API integration test
 *
 * User story:
 *   The UI sends a single debounced POST to set pv_irradiance.
 *   The backend applies the offset once and immediately clears the inject field
 *   so the offset decays from that moment forward.
 *
 * Requires a running VEN instance.  Set VITE_VEN_URL to point at it, or let it
 * default to Node1's real LAN address.  The suite is skipped automatically when
 * the VEN is unreachable so CI stays green without a live server.
 *
 * Uses the IP, not the "Node1" hostname: "Node1" is only an SSH client alias
 * (~/.ssh/config), not a real DNS/hosts entry, so Node's getaddrinfo can't
 * reliably resolve it (falls back to unreliable Windows LLMNR/NetBIOS
 * broadcast resolution, which flakes independently on every call).
 */

import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { VenApi } from "../api/client";

const VEN_URL = (import.meta as { env?: Record<string, string> }).env?.VITE_VEN_URL
  ?? "http://192.168.1.103:8211";

const api = new VenApi(VEN_URL);

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

// ── connectivity check ────────────────────────────────────────────────────────

let venReachable = false;

beforeAll(async () => {
  try {
    await api.sim();
    venReachable = true;
  } catch {
    venReachable = false;
  }
});

afterAll(async () => {
  // Restore alpha to default so the running sim is not left in a test state.
  if (venReachable) {
    try {
      await api.postSimInject({ pv_irradiance_alpha: 0.1 } as never);
    } catch { /* best-effort */ }
  }
});

// ── test suite ────────────────────────────────────────────────────────────────

describe("PV irradiance one-shot inject", () => {
  it(
    "inject is consumed after one tick and irradiance decays",
    async () => {
      if (!venReachable) {
        console.warn(`[skip] VEN not reachable at ${VEN_URL}`);
        return;
      }

      // 1. Record natural irradiance before any inject.
      const simBefore = await api.sim();
      const naturalIrradiance: number = (simBefore.assets as Record<string, { irradiance?: number }>).pv?.irradiance ?? 0;

      // 2. Inject an irradiance value with high alpha so decay is fast.
      //    alpha=0.99 → tau_s = -300/ln(0.01) ≈ 65 s → ~12% drop per 8 s.
      //    Irradiance is clamped to [0, 1] server-side, so a fixed offset
      //    (up or down) can run out of headroom depending on time of day
      //    (e.g. natural ≈ 1.0 at solar noon leaves no room above). Inject
      //    toward whichever side has more room instead of always upward.
      const injectingUp = naturalIrradiance <= 0.5;
      const injectIrradiance = injectingUp
        ? Math.min(naturalIrradiance + 0.3, 1.0)
        : Math.max(naturalIrradiance - 0.3, 0.0);
      await api.postSimInject({ pv_irradiance: injectIrradiance, pv_irradiance_alpha: 0.99 } as never);

      // 3. Wait ≥ 1 sim tick (tick period = 1 s) so the backend processes the inject.
      await sleep(1_500);

      // 4. Assert: inject field was consumed (one-shot cleared by backend).
      const injectAfter = await api.getSimInject();
      expect(
        (injectAfter as Record<string, unknown>).pv_irradiance ?? null,
        "pv_irradiance should be null after one tick (one-shot consumed)"
      ).toBeNull();

      // 5. Assert: irradiance was actually applied (diverged from natural in
      //    the injected direction).
      const simAfterInject = await api.sim();
      const irradianceAfterInject: number =
        (simAfterInject.assets as Record<string, { irradiance?: number }>).pv?.irradiance ?? 0;
      const expectedBound = injectingUp ? naturalIrradiance + 0.1 : naturalIrradiance - 0.1;
      expect(
        irradianceAfterInject,
        `irradiance (${irradianceAfterInject.toFixed(3)}) should be ${injectingUp ? "above" : "below"} ` +
          `natural${injectingUp ? "+" : "-"}0.1 (${expectedBound.toFixed(3)})`
      )[injectingUp ? "toBeGreaterThan" : "toBeLessThan"](expectedBound);

      // 6. Wait for decay to be observable.
      //    With alpha=0.99, tau_s≈65 s → exp(-8/65) ≈ 0.884 → ~12 % change after 8 ticks.
      await sleep(8_000);

      // 7. Assert: irradiance is decaying back toward natural — down if
      //    injected up, up if injected down.
      const simAfterDecay = await api.sim();
      const irradianceAfterDecay: number =
        (simAfterDecay.assets as Record<string, { irradiance?: number }>).pv?.irradiance ?? 0;
      expect(
        irradianceAfterDecay,
        `irradiance after decay (${irradianceAfterDecay.toFixed(3)}) should be ` +
          `${injectingUp ? "less" : "greater"} than immediately after inject (${irradianceAfterInject.toFixed(3)})`
      )[injectingUp ? "toBeLessThan" : "toBeGreaterThan"](irradianceAfterInject);
    },
    30_000 // generous timeout for network + sleep
  );
});
