# Phase 4 Use Case Manual — Comfort & Personas (Manual Test Procedures)

This manual contains human-executable, step-by-step test procedures for every Phase 4
work package (`docs/plans/roadmap/phase-4-comfort-and-personas.md`). Each section
verifies the new behaviour of one WP against the running lab, the same way
`SYSTEM-USE-CASE-MANUAL.md` replays the VTN-side use cases. One section is appended
per WP as it lands.

**VTN UI:** http://Pi4-Server:8221
**VEN UI:** http://Pi4-Server:8214

## Prerequisites

1. VTN UI health chip shows **"VTN: ok"**; VEN UI health chip shows **"ok"** for the
   selected VEN.
2. All containers run the current `main`/phase-4 build (long-lived containers silently
   decouple — when in doubt, rebuild the trio + BFF and restart `ui`).
3. Timestamps: local time with explicit offset (`+01:00` winter / `+02:00` summer),
   RFC 3339 — same convention as `SYSTEM-USE-CASE-MANUAL.md`.

---

## M4.1 — UserRequestMode is accepted and echoed end-to-end (WP4.1-a)

### What this verifies

Every user request (EV charging, heater target, shiftable load) now carries a
`mode` field — one of `ASAP`, `ASAP_FREE`, `BY_DEADLINE`, `BY_DEADLINE_FREE`,
`MAX_COST`, `OPPORTUNISTIC` — stored on both the `UserRequest` and the created
device session. In WP4.1-a the field is **plumbing only**: it must round-trip
through API and UI, default to `BY_DEADLINE`, and change no planning behaviour.

### Steps (VEN UI)

1. Open the **VEN UI** → **Devices** page, select `VEN1`.
2. In the **EV Charging** card, click **Plan Charging**.
3. The dialog now shows a **Mode** dropdown below the "Soft deadline" switch,
   preselected to `BY_DEADLINE`.
4. Set Target SoC ≈ 80 %, a departure a few hours out, and select mode
   `OPPORTUNISTIC`. Confirm.
5. **Expected:** the active-session view shows a small `OPPORTUNISTIC` chip next
   to the target/departure lines. (No chip appears for `BY_DEADLINE` — the default
   is not called out.)
6. Click **Unplan**, then repeat step 2–4 leaving the mode untouched.
   **Expected:** no mode chip (default `BY_DEADLINE`).
7. Repeat the same check on the **Water Heater** card (Set Target dialog) and the
   **Shiftable Loads** card (Add Load dialog): both dialogs show the same Mode
   dropdown.

### Steps (API)

8. From a shell (any machine that reaches the Pi4):

   ```bash
   # Create an EV session with an explicit mode
   curl -s -X POST http://Pi4-Server:8211/ev-session \
     -H 'Content-Type: application/json' \
     -d '{"target_soc":0.8,"departure_time":"2026-07-12T22:00:00+02:00","mode":"ASAP"}'
   ```

   **Expected:** HTTP 201; response JSON contains `"mode":"ASAP"`.
9. `curl -s http://Pi4-Server:8211/ev-session` — **Expected:** `"mode":"ASAP"` echoed.
10. Delete it: `curl -s -X DELETE http://Pi4-Server:8211/ev-session`.
11. Re-create **without** the field (drop `"mode":"ASAP"` from the body).
    **Expected:** HTTP 201 and the response contains `"mode":"BY_DEADLINE"` —
    the backward-compatible default.
12. `curl -s http://Pi4-Server:8211/user-requests` after creating a request from the
    UI — **Expected:** each request object carries a `mode` field, and its embedded
    `session` object carries the same value.

### Non-regression check

13. With a `BY_DEADLINE` (default) EV session active, open the **Controller** page:
    the plan must look exactly as before Phase 4 — WP4.1-a introduces no
    behavioural change, only the field.

---

## M4.2 — ASAP and OPPORTUNISTIC change the plan (WP4.1-b)

### What this verifies

The two mode poles now steer the MILP planner on the EV path:

- **ASAP** — charge at maximum feasible rate from *now*, cost-blind (a lateness
  penalty of `asap_lateness_eur_kwh_h`, default 10 €/kWh·h, dominates any tariff).
- **OPPORTUNISTIC** — no deadline, no charging obligation; charge **only** where
  energy is free (forecast PV surplus, or slots with a non-positive import
  tariff), rewarded at `v_ev_free_charge_eur_kwh` (default 0.10 €/kWh).

### Setup — a price ramp that separates the modes

1. On the **VTN UI**, create a price event on the open program for the next 2 h:
   first hour expensive (e.g. `0.40 €/kWh`), second hour cheap (e.g. `0.05 €/kWh`)
   — same recipe as the dynamic-tariff scenarios in `SYSTEM-USE-CASE-MANUAL.md`.
2. On the **VEN UI** (VEN1), make sure the EV shows as plugged in with SoC well
   below target (Devices page; use `/sim` injection if needed).

### Steps — ASAP front-loads

3. Plan Charging: Target ≈ 30 % above current SoC, departure **+2 h**, mode `ASAP`.
4. Open the **Controller** page and look at the plan timeline.
   **Expected:** EV charging starts in the *first* slots at ~max charger power,
   inside the expensive window. A cost-aware plan would have waited — that's the
   point: ASAP is cost-blind.
5. Unplan.

### Steps — BY_DEADLINE defers (contrast)

6. Same session but mode `BY_DEADLINE` (or untouched default).
   **Expected:** the planned EV charging sits in the *cheap* second hour, not in
   the expensive first hour. Unplan.

### Steps — OPPORTUNISTIC charges only free energy

7. Same session but mode `OPPORTUNISTIC`, departure time irrelevant (it is
   ignored — no deadline constraint).
8. **While the import tariff is positive and there is no PV surplus** (evening,
   or PV irradiance slider at 0): **Expected:** the plan contains *no* EV
   charging at all. The request stays active but nothing is scheduled.
9. Now create PV surplus: on the **Devices/Sim** controls raise PV irradiance so
   forecast PV exceeds the base load (daytime), or publish a price event with a
   **negative** import tariff interval.
   **Expected:** after the next replan (≤ ~30 s), EV charging appears exactly in
   the surplus / negative-price slots, capped at the surplus power — never more.

### API check

10. `curl -s http://Pi4-Server:8211/plan | jq '[.slots[] | {start, ev: (.allocations[]? | select(.asset_id=="ev") | .power_kw)}]'`
    shows the same allocation pattern the UI displays.

---

## M4.3 — Notification feed (WP4.3)

### What this verifies

User-facing notifications (BL-20): a bell with badge in the VEN UI app bar, a
feed panel, `GET /notifications`, an SSE stream, and persistence across VEN
restarts. Producers live at three trigger points so far: grid-emergency alert
events (Alert), VTN reachability edges (Warn on loss / Info on recovery), and
newly-appearing planner warnings on an adopted plan (Warn / Alert).

### Steps — grid emergency produces an Alert

1. Open the **VEN UI** (VEN1). Note the bell icon next to the health chip —
   remember its badge count.
2. On the **VTN UI**, create an alert event (priority 0, `alertType` payload —
   same recipe as UC1 in `SYSTEM-USE-CASE-MANUAL.md`) targeted at VEN1 with a
   window starting now.
3. Within one poll cycle (~30 s) the badge count increments.
   Click the bell: the top entry reads **ALERT — Grid emergency (…)** with the
   event's message text.
4. Delete the alert event. **Expected:** no new notification (deletion is not
   an emergency), and the existing entry stays in the feed (it is history).

### Steps — VTN outage edges

5. Stop the VTN container: `ssh Pi4-Server "docker stop vtn-vtn-1"` (test stack
   only — never production containers without approval; this is the lab VTN).
6. After the next poll (~30 s): **Expected:** one **WARN — VTN unreachable**
   notification. Repeated failed polls must NOT add more entries.
7. Start the VTN again. **Expected:** one **INFO — VTN connection restored**.

### Steps — persistence across restart

8. `curl -s http://Pi4-Server:8211/notifications | jq length` — note the count.
9. Restart VEN1: `ssh Pi4-Server "cd /srv/docker/openadr_lab/VEN && docker compose restart ven-1"`.
10. After it is healthy, repeat step 8. **Expected:** the same entries are
    back (seeded from `history.sqlite`), not an empty list.

### API check — since filter and SSE

11. `curl -s "http://Pi4-Server:8211/notifications?since=2026-07-12T00:00:00Z"`
    returns only entries newer than the timestamp.
12. `curl -N http://Pi4-Server:8211/notifications/events` holds an SSE stream
    open; trigger step 2 again and watch the notification arrive live.

---

## M4.4 — StaleRatePolicy fills slots beyond tariff coverage (WP4.4)

### What this verifies

The planner (BL-07) now prices slots that lie beyond the last known tariff
data per the profile-configured `stale_rate_policy`:

| Policy | Stale-slot import rate |
|---|---|
| `LAST_KNOWN` | last known rate repeated |
| `SAFE_AVERAGE` | `stale_rate_safe_pctl` percentile of the known rates (default 0.8) |
| `DEFER_TO_FLEXIBLE` | maximum known rate — discretionary load defers into covered slots |
| `HEURISTIC_FORECAST` (default) | stub until Phase 5 (BL-14): behaves like `LAST_KNOWN`, says so in the warning |

Stale slots carry `rate_estimated: true` and the plan carries one stable
warning, which the WP4.3 feed surfaces as a single Warn notification.

### Steps

1. On the **VTN UI**, publish a short price event: three intervals over the
   next 2 h, e.g. `0.40 / 0.20 / 0.10 €/kWh` (UC recipe from
   `SYSTEM-USE-CASE-MANUAL.md`). The VEN horizon (48 h) now extends far past
   coverage.
2. After the next replan: `curl -s http://Pi4-Server:8211/plan | jq '[.slots[] | {start, rate: .import_tariff_eur_kwh, est: .rate_estimated}] | .[0:8]'`
   **Expected:** the first ~2 h of slots show the published rates with
   `est: false`; later slots show the fill rate with `est: true`.
   With the default policy the fill equals the last published rate (0.10).
3. `curl -s http://Pi4-Server:8211/plan | jq .warnings`
   **Expected:** one warning naming `HEURISTIC_FORECAST` and its
   `LAST_KNOWN` fallback.
4. Check the notification bell: **Expected:** one **WARN** entry with the same
   text — and only one, even after several replans (dedup by stable text).
5. Optional — policy comparison: set `stale_rate_policy: SAFE_AVERAGE` (or
   `DEFER_TO_FLEXIBLE`) in the VEN profile YAML, restart the VEN, repeat
   step 2. **Expected:** the stale-slot rate changes to the percentile
   (0.20 at p50) or the maximum (0.40) respectively.

---

## M4.5 — Comfort-curve override (WP4.2)

### What this verifies

A resident can replace an asset's built-in comfort/value curve (BL-19) with
their own bid curve via UI or API; the override persists across VEN restarts
(`user_settings` table) and is preferred over `default_comfort_rates()`
wherever the curve is consulted; DELETE restores the default. Invalid curves
(non-monotonic fill, out-of-range bids) are rejected.

### Steps (VEN UI)

1. **VEN UI → Devices** (VEN1). The new **Comfort Curve** card shows the
   selected asset's effective curve with a `default` chip.
2. Change the first point's bid (e.g. 0.30 → 0.50 €/kWh) and click **Save**.
   **Expected:** the chip flips to `override`.
3. Click **Reset to default**. **Expected:** chip back to `default`, table
   shows the built-in points again.

### Steps (API + persistence)

4. Install an override:

   ```bash
   curl -s -X POST http://Pi4-Server:8211/assets/ev/comfort_curve \
     -H 'Content-Type: application/json' \
     -d '[{"fill":0.5,"max_marginal_price":0.40,"max_marginal_co2":0},
          {"fill":1.0,"max_marginal_price":0.10,"max_marginal_co2":0}]'
   ```

   **Expected:** HTTP 201 with `"source":"override"`.
5. `curl -s http://Pi4-Server:8211/assets/ev/comfort_curve` echoes the
   override.
6. Restart VEN1 (`docker compose restart ven-1`), wait for healthy, repeat
   step 5. **Expected:** the override is still there (persisted).
7. Validation: POST a non-monotonic curve
   (`[{"fill":0.9,...},{"fill":0.5,...}]`). **Expected:** HTTP 422 with a
   reason. POST to `/assets/toaster/comfort_curve` → HTTP 404.
8. `curl -s -X DELETE http://Pi4-Server:8211/assets/ev/comfort_curve` →
   HTTP 204; GET reports `"source":"default"` again.

> Note: today the curve feeds the user-request build path
> (`AssetRequestSlice`); a deeper coupling of the curve into MILP tier
> constraints is future work (see BL-19 resolution note in `docs/BACKLOG.md`).

---

## M4.6 — MAX_COST and *_FREE request modes (WP4.1-c)

### What this verifies

The remaining request modes (BL-28):

- **MAX_COST** — charge toward the target whenever it is cheapest, but total
  charging cost never exceeds the session budget. An unaffordable target does
  NOT fail the plan: charging stops at the budget and one Warn notification
  ("budget") appears in the feed.
- **ASAP_FREE** — only free energy (PV surplus / non-positive tariff), taken
  as early as it appears.
- **BY_DEADLINE_FREE** — only free energy, and only inside the deadline window.

### Steps — MAX_COST budget shortfall

1. **VEN UI → Devices → Plan Charging** (VEN1): target 90 %, mode `MAX_COST`.
   A **Budget (€)** field appears — set it absurdly low, e.g. `0.05`. Confirm.
2. Within ~1–2 replans: **Expected:** the notification bell gains one **WARN**
   entry saying the budget is too low to reach the target; the Controller page
   shows partial EV charging only (what €0.05 buys).
3. Unplan, repeat with budget `5.00`. **Expected:** full charging schedule in
   the cheapest slots, no budget notification.

### Steps — free-energy modes

4. Pin the PV forecast to zero so no energy is free:
   `curl -s -X POST http://Pi4-Server:8211/sim/inject -H 'Content-Type: application/json' -d '{"pv_plan_kw": 0.0}'`
5. Plan Charging with mode `ASAP_FREE` (no other change).
   **Expected:** after the next replan the plan contains **no** EV charging at
   all — the request stays active but nothing is scheduled (flat positive
   tariff, no surplus → no free energy).
6. Restore PV (`curl -s -X POST http://Pi4-Server:8211/sim/inject/reset`) at
   midday, or publish a price event with a negative-price interval.
   **Expected:** charging appears exactly in the surplus / negative-price
   slots, front-loaded (earliest free slots first for `ASAP_FREE`).
7. Repeat with `BY_DEADLINE_FREE` and a departure before the free window:
   **Expected:** still no charging — free energy outside the deadline does
   not count.

---
