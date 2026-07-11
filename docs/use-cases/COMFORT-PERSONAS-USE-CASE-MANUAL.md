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
