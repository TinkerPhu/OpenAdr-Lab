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
