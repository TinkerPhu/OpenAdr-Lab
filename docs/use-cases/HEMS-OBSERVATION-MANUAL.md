# VEN HEMS Use Case Manual — Glass-Box Observation Guide

This manual shows how to observe all 14 HEMS controller use cases from `docs/VEN_Controller/Step5_UseCases.md` using the live lab UI. The Controller page was designed as a "glass box" into the planning engine: every packet, plan slot, rate snapshot, and ledger entry is visible in real time.

**VEN UI:** http://Pi4-Server:8214
**VTN UI:** http://Pi4-Server:8221

---

## UI Pages at a Glance

| Page | What it shows |
|---|---|
| **Controller** | Power chart (history + plan), Rate chart, Packets table (fill%), Ledger, Status bar |
| **Requests** | User requests table with status chips; form to create new requests; inline cancel |
| **Simulation** | Device state cards (EV SoC, Heater temp, PV output), Setpoints chart, Override sliders |
| **Trace** | Per-tick decision log: mode, FSM state, active events, setpoints, constraints |
| **Events** | Raw OpenADR events polled from VTN |
| **Dashboard** | Latest sensor snapshot |

The **Controller** page is the primary observation surface for all HEMS use cases. Open it in one browser tab and keep the **Simulation** page open in another. Use the **Requests** page to submit and cancel energy requests without curl.

---

## VEN Profiles Quick Reference

| VEN | Assets | Best for |
|---|---|---|
| **VEN1** | EV 7.4 kW / 60 kWh (SoC 40%) + PV 8 kW + Battery 10 kWh | UC-01, UC-03, UC-05, UC-06, UC-08, UC-09 |
| **VEN2** | Heater 5 kW + PV 12 kW — no EV, no battery | UC-11 (consumption-like), UC-14 |
| **VEN3** | EV 11 kW / 75 kWh (SoC 30%) + Heater 3 kW + PV 6 kW | UC-06, UC-13 |

---

## Observability Summary

| UC | Observable? | Key trigger | Primary page |
|---|---|---|---|
| UC-01 EV Overnight Charge | ✅ Full | User Requests page → New User Request | Controller: Packets + Power chart |
| UC-02 Washing Machine Batch | ⚠️ Concept only — no washing machine in profiles | User Requests page → New User Request (EV as proxy) | Controller: Packets |
| UC-03 PV Surplus Cascade | ✅ Full | Use VEN1, sunny sim time | Controller: Power chart |
| UC-04 Day-Ahead Price Update | ✅ Full | VTN PRICE event | Controller: Rate chart |
| UC-05 Favorable Far-Horizon | ✅ Full | VTN cheap-window PRICE event | Controller: Rate chart + Packets |
| UC-06 Grid Emergency Alert | ✅ Full | VTN IMPORT_CAPACITY_LIMIT=0 | Simulation setpoints + Trace |
| UC-07 Capacity Reservation | ⚠️ Partial — VEN→VTN reservation not implemented | VTN IMPORT_CAPACITY_LIMIT | Controller: Capacity card |
| UC-08 EV Disconnects Mid-Charge | ✅ Full | Simulation: unplug EV toggle | Controller: Packets (PAUSED) |
| UC-09 Tier Fallback | ✅ Full | User Requests page → New User Request (tight budget) | Controller: Plan card warnings |
| UC-10 Peak Demand Penalty | ⚠️ Partial — no penalty rule UI | Simulation: raise base_load_w | Controller: Power chart (EV steps down) |
| UC-11 Consumption-Only Site | ✅ Full | Use VEN2 or zero PV irradiance | Controller: Power chart (no PV negative) |
| UC-12 VTN Communication Loss | ⚠️ Partial — requires terminal step | `ssh` stop VTN container | Controller: Plan card warnings |
| UC-13 VTN Direct Override | ✅ Full (proxy signal) | VTN IMPORT_CAPACITY_LIMIT event | Simulation + Trace |
| UC-14 Thermal Feedback Loop | ✅ Full | Simulation: ambient_temp_c slider | Simulation HeaterCard + Controller |

> **Two cases not fully observable:**
> - **UC-02**: No washing machine in any VEN profile. You can create a packet for a `"washer"` asset via the User Requests page, but the simulator won't execute it. Use the EV to observe the same planning behavior (deferral to cheap window).
> - **UC-07**: The VEN proactively requesting additional capacity from the VTN is not yet implemented. Only the opposite direction (VTN sending a capacity limit) is observable. The Capacity card in the Controller status bar shows any active limit.

---

## Prerequisites

1. Open **VTN UI** (http://Pi4-Server:8221) — health chip shows **"VTN: ok"**
2. Open **VEN UI** (http://Pi4-Server:8214) — health chip shows **"ok"**
3. Navigate to the **Controller** page — you should see seeded packets in the Packets table and a Power chart with trace lines
4. Open the **Simulation** page in a second tab

For use cases that create energy requests, use the **User Requests** page (nav button in the VEN UI). No curl required:
- Click **New User Request** — a monospace JSON textarea opens, prefilled with a working EV example (deadline = tomorrow 07:00)
- Select-all, paste the JSON for the use case, click **Submit**
- Use **Reset to EV example** if you want to start from the prefill again
- Cancel an active request with the trash icon → confirm dialog

If you prefer curl (e.g. for scripting or automation), the CLI equivalents are listed in the **CLI Reference** section at the end of this document. Set up shortcuts:
```bash
VEN1=http://Pi4-Server:8211
VEN2=http://Pi4-Server:8212
VEN3=http://Pi4-Server:8213
```

---

## UC-01: EV Overnight Charge

**Scenario:** User plugs in EV at 18:00, wants 80% SoC by 07:00, budget €3. EV skips expensive peak and charges off-peak.

**What the controller should do:** Peak slots are too expensive (EffectiveCost > ComfortBid at low fill). EV defers to off-peak. FlexibilityEnvelope shows 25 kWh available 20:00–07:00. When off-peak slots enter the near-horizon (become FIRM), the EV starts charging.

**Suggested VEN:** VEN1 (EV SoC 40%, target 80% = 24 kWh needed)

### Setup

1. Switch to **VEN1** in the VEN dropdown

2. Navigate to the **User Requests** page → **New User Request**. Paste this JSON (the dialog is prefilled with a similar example — select-all and replace):

```json
{
  "asset_id": "ev",
  "target_soc": 0.80,
  "target_energy_kwh": null,
  "desired_power_kw": 7.0,
  "completion_policy": "CONTINUE",
  "deadlines": [
    {
      "latest_end": "TOMORROW_07:00:00Z",
      "max_total_cost_eur": 3.00,
      "max_marginal_rate_eur_kwh": 0.30,
      "min_completion": 0.8
    }
  ],
  "comfort_rates": null
}
```
Replace `TOMORROW_07:00:00Z` with tomorrow's 07:00 in ISO 8601 (e.g. `2026-03-13T07:00:00+01:00`). The prefilled example already has tomorrow 07:00 — you can also just edit `max_marginal_rate_eur_kwh` to `0.30` in the existing prefill instead of replacing the whole block.

Click **Submit**. The new row appears in the User Requests table with status **ACTIVE**.

### What to observe

**Controller → Packets table:**
- New row appears with `asset_id = ev`, status `PENDING` or `SCHEDULED`
- Fill% bar at 0% initially
- Deadline shows tomorrow 07:00
- Estimated cost shows ~€3.00 (budget-limited)

**Controller → Power chart:**
- **Solid lines** (past): `trace_ev` flat at 0 or at current idle level
- **Dashed lines** (future): `plan_ev` shows 0 kW during peak slots, then a step up to ~7 kW at the off-peak boundary (20:00)
- The NOW line divides what has happened from what is planned

**Controller → Rate chart:**
- If a PRICE event is active: you can see the price step at the peak→off-peak boundary
- Without a PRICE event: flat rate is used (no visible step)

**Controller → Status bar (Plan card):**
- Trigger: `USER_REQUEST`
- Firm cost shows 0 (EV not yet in FIRM zone)

**Controller → Ledger:**
- `ev` row starts accumulating import kWh and cost once charging begins

### Observability note
The VEN processes the new packet within one planning cycle (~20 seconds). The off-peak allocation won't appear in FIRM slots until the near-horizon sliding window reaches 20:00. Run this in the late afternoon or early evening for best timing.

---

## UC-02: Washing Machine Batch Run

**Scenario:** Batch consumer deferred to cheap PV surplus window. The system waits for 12:00 (stronger PV) rather than starting immediately at 10:00 (higher import cost).

> **No washing machine in VEN profiles.** Use the **EV as a proxy** for this planning behavior — the Planner defers any flexible packet to the cheapest window using the same logic.

**Substitute observation:** Create a short EV request with a 2-hour tight window that spans a PV surplus period (midday). The Planner will prefer PV surplus slots.

### Setup

1. Switch to **VEN1**
2. On the **Simulation** page, enable **Manual Irradiance** at 80% to simulate strong PV production
3. Navigate to the **User Requests** page → **New User Request**. Paste this JSON:

```json
{
  "asset_id": "ev",
  "target_soc": null,
  "target_energy_kwh": 2.0,
  "desired_power_kw": 7.0,
  "completion_policy": "STOP",
  "deadlines": [
    {
      "latest_end": "TODAY_14:00:00+01:00",
      "max_total_cost_eur": 2.00,
      "max_marginal_rate_eur_kwh": null,
      "min_completion": null
    }
  ],
  "comfort_rates": null
}
```
Replace `TODAY_14:00:00+01:00` with today's 14:00 in ISO 8601 (e.g. `2026-03-12T14:00:00+01:00`). Click **Submit**.

### What to observe

**Controller → Power chart:**
- Plan dashed lines show EV deferred (0 kW in expensive slots, positive kW in cheap/surplus slots)
- `import_cap` and `export_cap` step lines show site capacity limits

**Controller → Packets table:**
- Status `SCHEDULED` — Planner assigned a future start within the cheap window
- Estimated cost reflects PV self-consumption (cheaper than pure grid import)

**Conceptual gap:** Without a real washing machine in the simulator, you won't see a 2 kW fixed ON/OFF asset — only the planning deferral logic, which is identical.

---

## UC-03: PV Surplus Cascade

**Scenario:** Sunny afternoon. PV generates more than site consumes. Cascade: self-consume → charge battery → export.

**What the controller should do:** Planner allocates battery charging at the PV surplus rate (ExportPrice opportunity cost ≈ €0.08). After battery is full, residual surplus is exported. The priority chain is emergent from EffectiveCost, not hardcoded.

**Suggested VEN:** VEN1 (has PV 8 kW + Battery 10 kWh)

### Setup

1. Switch to **VEN1**
2. Navigate to **Simulation** page
3. Enable **Manual Irradiance** and set to **80–100%** (midday equivalent)
4. Wait ~10 seconds for the simulator to tick

### What to observe

**Simulation → PV card:**
- `Output` shows 6–8 kW (positive)
- `Irradiance` at 80%+

**Simulation → Power & Energy card:**
- Net power approaches 0 or goes negative (export) once battery absorbs the surplus
- Export kWh accumulating

**Controller → Power chart (solid lines, past):**
- `trace_pv` shows the export limit (0 = no limit, PV running freely)
- `trace_net` dips toward or below 0 (site importing less / exporting)

**Controller → Ledger:**
- `pv` row: `Export kWh` increasing
- Battery row (if visible by asset_id): import kWh accumulating during charge phase

**Controller → Plan card:**
- Trigger `PERIODIC` — normal replan cycle
- Firm cost shows low values (surplus power has low opportunity cost)

### What you should NOT see
- The system does not first fill the battery, then redirect to export, then to self-consume in separate "modes". It all happens through one allocation pass per planning cycle. The chart shows all three effects simultaneously when the cascade is in progress.

---

## UC-04: Day-Ahead Price Update from VTN

**Scenario:** At 16:00, VTN publishes new prices for tomorrow. Planner replans immediately on RATE_CHANGE trigger, shifting packet allocation to the cheapest new slots.

**What the controller should do:** Rate chart updates. FlexibilityEnvelopes for pending packets are recalculated. Estimated costs on packets change to reflect new rates.

### Setup

**VTN UI** (http://Pi4-Server:8221):

1. Navigate to **Programs** → **Create**
2. Program Name: `obs-uc04-price` — leave VENs unchecked (open program)
3. Navigate to **Events** → **Create**
4. Event Name: `obs-price-update`, Program: `obs-uc04-price`, Priority: `5`
5. Paste Intervals (use your current time, intervals starting now):
```json
[
  {"id": 0, "intervalPeriod": {"start": "NOW+00:00", "duration": "PT2H"}, "payloads": [{"type": "PRICE", "values": [0.35]}]},
  {"id": 1, "intervalPeriod": {"start": "NOW+02:00", "duration": "PT4H"}, "payloads": [{"type": "PRICE", "values": [0.08]}]},
  {"id": 2, "intervalPeriod": {"start": "NOW+06:00", "duration": "PT2H"}, "payloads": [{"type": "PRICE", "values": [0.22]}]}
]
```
*Example (local time 16:00 CET):*
```json
[
  {"id": 0, "intervalPeriod": {"start": "2026-03-12T16:00:00+01:00", "duration": "PT2H"}, "payloads": [{"type": "PRICE", "values": [0.35]}]},
  {"id": 1, "intervalPeriod": {"start": "2026-03-12T18:00:00+01:00", "duration": "PT4H"}, "payloads": [{"type": "PRICE", "values": [0.08]}]},
  {"id": 2, "intervalPeriod": {"start": "2026-03-12T22:00:00+01:00", "duration": "PT2H"}, "payloads": [{"type": "PRICE", "values": [0.22]}]}
]
```
6. Click **Create**

**VEN UI** — wait up to 30 seconds for poll.

### What to observe

**Controller → Rate chart:**
- Three price steps appear: 0.35 → 0.08 → 0.22 €/kWh
- The NOW line shows which step is currently active
- Import price area (cyan) shows the staircase clearly

**Controller → Plan card:**
- Trigger changes to `RATE_CHANGE` on next replan
- Estimated costs on existing packets update

**Controller → Packets table:**
- If any packet was previously estimated at peak-rate cost, its `Est. Cost €` drops when cheap window appears

**VEN UI → Events:**
- Event `obs-price-update` appears with 3 intervals visible

---

## UC-05: VTN Sends Favorable Far-Horizon Pricing

**Scenario:** VTN offers a cheap incentive window (€0.03/kWh) far in the future. The system holds flexibility and resolves to that window when it enters the near-horizon.

**What the controller should do:** Rate chart shows the cheap window. Packets estimated cost drops sharply. Flexible allocation shifts to cheap window rather than committing early to standard off-peak.

### Setup

This builds on UC-04. If you did UC-04, add one more cheap window further in the future:

**VTN UI** → **Events** → **Create**:
- Event Name: `obs-incentive-window`, Program: `obs-uc04-price` (reuse), Priority: `5`
```json
[
  {"id": 0, "intervalPeriod": {"start": "TONIGHT+02:00", "duration": "PT2H"}, "payloads": [{"type": "PRICE", "values": [0.03]}]}
]
```
*Example — 2 AM tonight:*
```json
[
  {"id": 0, "intervalPeriod": {"start": "2026-03-13T02:00:00+01:00", "duration": "PT2H"}, "payloads": [{"type": "PRICE", "values": [0.03]}]}
]
```

### What to observe

**Controller → Rate chart:**
- Cheap window at €0.03 visible as a deep dip in the import price area
- The step is clearly distinct from the surrounding €0.08–0.22 rates

**Controller → Packets table:**
- `Est. Cost €` on EV packet drops significantly (e.g. €3.00 → ~€1.74)
- This is the key behavioral signal: the system recognized the cheap window and updated its estimate

**Controller → Power chart (dashed future lines):**
- `plan_ev` should show 0 kW in the hours before the cheap window, then a spike to ~7 kW during it

### Key insight to verify
The power chart shows the EV packet is NOT pre-committed to the 18:00–20:00 off-peak window anymore. It is waiting for the 02:00 window. This is the FlexibilityEnvelope mechanism at work.

---

## UC-06: Grid Emergency Alert

**Scenario:** VTN signals emergency — EV pauses, heater reduces, battery (if present) discharges. Site net import drops sharply.

**What the controller should do:** Import capacity limit 0 is an emergency signal. Reactor switches to IMPORT_CAP or SIMPLE mode. EV charge drops to 0. Any packet in progress transitions to PAUSED.

**Suggested VEN:** VEN1 or VEN3 (both have EV)

### Setup

**VTN UI** → **Programs** → **Create**:
- Name: `obs-uc06-emergency` — check **VEN1** only (or VEN3)

**VTN UI** → **Events** → **Create**:
- Name: `obs-emergency`, Program: `obs-uc06-emergency`, Priority: `0`
- Intervals:
```json
[{"id": 0, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [0.0]}]}]
```
*(No intervalPeriod = active indefinitely until edited or deleted)*

**VEN UI** — wait up to 30 seconds.

### What to observe

**Simulation page:**
- **EV card**: Current charging drops to 0 kW
- **Power & Energy card**: Net import drops (EV + possibly heater reduced)

**Trace page:**
- Latest entries show mode `IMPORT_CAP` or `SIMPLE`
- `constraints` array shows the active capacity limit
- `setpoints.ev_charge_kw = 0.0`

**Controller → Power chart (solid trace lines):**
- `trace_ev` drops to 0 at the moment the event was received
- `trace_net` drops correspondingly

**Controller → Packets table:**
- EV packet status changes to `PAUSED`
- Fill% bar freezes (no energy being delivered)

### Ending the emergency

1. **VTN UI** → Events → find `obs-emergency` → click edit (pencil)
2. Add Start Time (when it began) and Duration (e.g. `PT5M`)
3. Save — VEN picks up the update within 30s and resumes charging

---

## UC-07: VTN Capacity Reservation Request

> **Partial observability.** The VEN proactively requesting additional capacity from the VTN (sending an `OadrCapacityRequest`) is **not implemented** in the current lab. Only the reverse direction — the VTN informing the VEN of a capacity limit — is observable.

**What IS observable:** The VTN can send an `IMPORT_CAPACITY_LIMIT` event. The Controller Capacity card reflects it immediately. The Planner respects it when allocating future slots (import_cap step line in Power chart).

### Setup

**VTN UI** → **Programs** → **Create**:
- Name: `obs-uc07-capacity` — check **VEN1**

**VTN UI** → **Events** → **Create**:
- Name: `obs-capacity-limit`, Priority: `3`
- Intervals:
```json
[{"id": 0, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [5.0]}]}]
```

### What to observe

**Controller → Status bar, Capacity card:**
- `Import limit: 5.0 kW` appears within 30 seconds

**Controller → Power chart (future dashed lines):**
- `import_cap` step line appears at 5 kW across all future plan slots
- `plan_net` will not exceed 5 kW in the dashed future allocation

**Controller → Plan card:**
- Trigger `CAPACITY_CHANGE` on next replan
- If EV would have charged at 7 kW but site baseline is 0.5 kW: effective limit for EV = 5 − 0.5 = 4.5 kW. The plan allocates accordingly.

### What is NOT observable
The scenario in Step5 where the VEN computes that reservation would pay off (€0.80 fee for 5 kW reservation) and sends a capacity request to the VTN. That economic evaluation and outgoing API call are not yet implemented.

---

## UC-08: EV Disconnects Mid-Charge

**Scenario:** EV is charging. User unplugs. Packet goes to PAUSED. When EV is reconnected, packet resumes and completes normally.

**What the controller should do:** `ASSET_STATE_CHANGE` trigger → Planner sees EV not available → clears all EV allocations → packet PAUSED. On reconnect: replans and resumes.

### Setup

1. Select **VEN1** (EV active and charging, or create a charge request from UC-01)
2. Open **Simulation** page
3. Find the **EV Charger** card — toggle **Plugged in** to **off**
4. Wait ~5 seconds (one simulator tick + reactor cycle)

### What to observe

**Simulation → EV card:**
- `Plugged` chip changes from green "Plugged" to grey "Unplugged"
- `Charging: 0 kW`

**Trace page:**
- New entry: `fsm_state = Idle`, constraints empty for EV

**Controller → Packets table:**
- EV packet status: `ACTIVE` → `PAUSED`
- Fill% bar freezes at current value
- Row is still visible (not removed)

**Controller → Power chart:**
- `trace_ev` drops to 0 at disconnect timestamp

Now **reconnect** the EV: toggle **Plugged in** back to **on**.

**Controller → Packets table:**
- Status transitions: `PAUSED` → `SCHEDULED` → `ACTIVE` (within one plan cycle, ~20s)
- Fill% bar resumes incrementing

**Controller → Power chart:**
- `trace_ev` resumes, `plan_ev` shows remaining allocation

### What to verify
The `accumulated_cost_eur` in the Ledger carries over across the disconnect — budget is not reset.

---

## UC-09: Tier Fallback on Time Constraint

**Scenario:** User has two deadline tiers — tonight (urgent, high budget) and Friday (relaxed, very low budget). Neither tier is feasible given current rates. System falls back to CONTINUE mode with lowest priority.

**What the controller should do:** Plan warns that Tier 0 can't be met (insufficient time). Tier 1 is also infeasible (€0.10 max rate — everything is more expensive). Packet stalls.

### Setup

Navigate to the **User Requests** page → **New User Request**. Paste this JSON:

```json
{
  "asset_id": "ev",
  "target_soc": 0.80,
  "target_energy_kwh": null,
  "desired_power_kw": 7.0,
  "completion_policy": "CONTINUE",
  "deadlines": [
    {
      "latest_end": "TODAY_22:00:00+01:00",
      "max_total_cost_eur": 5.00,
      "max_marginal_rate_eur_kwh": 0.50,
      "min_completion": null
    },
    {
      "latest_end": "FRIDAY_18:00:00+01:00",
      "max_total_cost_eur": 1.00,
      "max_marginal_rate_eur_kwh": 0.10,
      "min_completion": null
    }
  ],
  "comfort_rates": null
}
```
Replace `TODAY_22:00:00+01:00` with tonight's 22:00 and `FRIDAY_18:00:00+01:00` with this Friday's 18:00 in ISO 8601. Click **Submit**.

For Tier 0 to fail by time: run this in the afternoon when fewer than ~4.3h of off-peak remain before 22:00.
For Tier 1 to fail by budget: the €0.10 max marginal rate is below all available rates (off-peak is €0.12+), so it will always fail.

### What to observe

**Controller → Packets table:**
- Packet appears with status `PENDING`
- Estimated cost may be shown as €0 or minimal (no feasible allocation found)
- Deadline shows the first tier's deadline

**Controller → Plan card:**
- `Warnings` chip shows 1 or more warnings (yellow chip)

**Controller → Status bar, Plan card:**
- Warning count > 0

To inspect warning details: `GET http://Pi4-Server:8211/plan` — the `warnings` array in the JSON contains the message text (`"EV can only reach ~X% of target by tonight"` etc.). The **User Requests** page also shows the stalled request with status `ACTIVE` and estimated cost at or near €0.

### Repairing the situation

On the **User Requests** page, find the stalled user request and click the **delete (trash) icon** → confirm. Then open **New User Request** and paste the same JSON as above, with `max_total_cost_eur` on Tier 1 changed from `1.00` to `4.00`.

---

## UC-10: Peak Demand Penalty Avoidance

**Scenario:** An unmodeled device (dishwasher) causes a site load spike. Planner reduces EV power to prevent a rolling-average threshold breach.

> **Partial observability.** Penalty rules are configured in the profile YAML but not exposed in the UI. You can observe the *effect* — EV power reduction when base load spikes — but not the penalty rule logic directly.

**Suggested VEN:** VEN1 (has EV to reduce)

### Setup

1. Select **VEN1**
2. On **Simulation** page, note the current base load (~500 W)
3. Confirm EV is charging on **Controller → Packets table** (status ACTIVE)
4. On **Simulation** page, drag **Base load** slider from 500 W to **3000 W** (simulating a 2.5 kW unmodeled appliance turning on)

### What to observe

**Simulation → Power & Energy card:**
- Net import jumps (EV 7 kW + base load 3 kW = ~10 kW)

**Controller → Power chart (solid trace lines):**
- `trace_net` shows the load spike at the moment you changed the slider

**Planner reaction (within ~20 seconds):**
- If the total load approaches a capacity threshold, the Planner may reduce EV allocation
- `plan_ev` in the dashed future may show a reduced value (e.g. 5 kW instead of 7 kW)
- `trace_ev` in subsequent ticks reflects the actual commanded power

**Trace page:**
- New entries show updated constraints or reduced setpoints

### What you cannot see
The PenaltyRule entity (threshold €100, measurement window PT15M, rolling average) is not exposed in the UI. To inspect it directly: `GET http://Pi4-Server:8211/plan` and look at the plan JSON for `phase_6_diagnostics` or check the Rust logs on Pi4.

---

## UC-11: Consumption-Only Site (No PV, No Battery)

**Scenario:** Site has only controllable loads — EV and heater. No generation. Planning value is load shifting (off-peak only), no PV self-consumption cascade.

**What the controller should do:** Power chart never goes negative (no export). No battery allocation. FlexibilityEnvelopes still work — EV shifts to off-peak. Rate chart shows the available price windows.

### Option A: Use VEN2 (no EV, no battery)

VEN2 has only a heater and PV. To make it consumption-only:

1. Select **VEN2** in the VEN dropdown
2. On **Simulation** page, enable **Manual Irradiance** and set to **0%**
3. Now the site has only the heater drawing power from the grid

**Controller → Power chart:**
- `trace_pv` = 0 (no PV output)
- `trace_net` is always positive (only importing, never exporting)
- No battery lines visible

### Option B: Force VEN1 to consumption-only

1. Select **VEN1**
2. On **Simulation** page, enable **Manual Irradiance** at **0%**
3. This removes PV surplus — battery has nothing to charge from

**Controller → Power chart:**
- `trace_pv` = 0
- `trace_net` = EV + heater + base load (all positive, grid import only)

### What to observe

**Controller → Power chart:**
- Solid lines always above 0 (importing, never exporting)
- Plan dashed lines show EV deferred to cheapest available time window (no PV surplus to exploit)

**Controller → Plan card:**
- Firm cost is purely grid import cost (no zero-cost surplus allocation)

**Controller → Ledger:**
- `Export kWh` column stays at 0.000
- `Cost €` accumulates for all energy (no free/cheap surplus)

---

## UC-12: VTN Communication Loss

**Scenario:** VTN network drops. System operates on cached rates. After rates expire, Planner uses stale-rate policy. On reconnection, replans with fresh data.

> **Partial observability.** Requires stopping the VTN container via terminal. After that, all effects are observable in the Controller UI.

### Setup

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose -f VTN/docker-compose.yml stop vtn"
```

Wait 60–90 seconds (VEN poll interval).

### What to observe

**VEN UI → Health chip:**
- May still show "ok" (VEN is healthy, VTN is unreachable)

**Controller → Plan card:**
- Warnings chip appears with 1+ warnings
- Warning message includes text about VTN offline / stale rates

**Controller → Rate chart:**
- Rate data stops updating (snapshots don't refresh)

**Controller → Events page:**
- Event list stops updating (no new events from VTN)

Wait until cached rates would normally expire (if day-ahead rates were loaded, this takes hours — for a quick test, the plan warning about "operating on cached rates" appears within minutes).

### Restore

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && docker compose -f VTN/docker-compose.yml start vtn"
```

Wait 30 seconds for VEN to reconnect. Controller Plan card should show `RATE_CHANGE` trigger as the fresh rates arrive.

---

## UC-13: VTN Direct Override (DISPATCH_SETPOINT)

**Scenario:** VTN sends a direct power setpoint to the heat pump (or EV). System bypasses normal planning and dispatches immediately.

> **Proxy signal.** `DISPATCH_SETPOINT` is the Step5 OpenADR payload type for direct control. In the lab, use `IMPORT_CAPACITY_LIMIT` to achieve the same observable effect: the reactor constrains the controlled asset and the Trace/Simulation pages show the override in action.

**Suggested VEN:** VEN3 (has both EV and heater)

### Setup

**VTN UI** → **Programs** → **Create**:
- Name: `obs-uc13-override` — check **VEN3**

**VTN UI** → **Events** → **Create**:
- Name: `obs-dispatch-override`, Priority: `2`
- Intervals (active for 5 minutes from now):
```json
[
  {
    "id": 0,
    "intervalPeriod": {"start": "NOW+00:00", "duration": "PT5M"},
    "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [3.0]}]
  }
]
```

### What to observe

**Simulation page (VEN3):**
- **EV card**: Charging power reduces to stay within the 3 kW site import limit
- If heater is also running: one or both assets reduce

**Trace page:**
- Mode shows `IMPORT_CAP`
- `setpoints.ev_charge_kw` shows reduced value
- `constraints` array lists the active event

**Controller → Power chart:**
- `trace_net` drops to ≤ 3 kW after event received
- `trace_ev` shows the reduced setpoint in solid lines

**After 5 minutes:**
- Event expires. Trace page shows mode returning to `IDLE` or `PRICE`.
- EV charging resumes at normal rate.

### Conceptual difference from Step5
Step5's `DISPATCH_SETPOINT` targets a specific asset by `resource_name`. The lab's `IMPORT_CAPACITY_LIMIT` targets the whole site. The observable behavior (asset reduction, mode change in Trace, setpoint drop in Simulation) is the same. The VEN3 `partial` strategy means compliance is at 70% — the override is partially followed, which you can verify in the Trace constraints.

---

## UC-14: Thermal Feedback Loop (Heat Pump Temperature Drop)

**Scenario:** Outdoor temperature drops from 5°C to -2°C. Heat pump must increase power to maintain indoor temperature. Planner adjusts future allocation upward.

**What the controller should do:** Heater's required continuous power increases as temperature drops. Planner re-allocates more kW to heater in future slots. Power chart shows planned heater power stepping up.

**Suggested VEN:** VEN2 or VEN3 (both have a heater)

### Setup

1. Select **VEN2** (heater 5 kW, target range 19–23°C)
2. Navigate to **Simulation** page
3. Note the current **Ambient temperature** slider value (default ~10°C)
4. Drag **Ambient temperature** down to **-5°C**
5. Wait 5–10 seconds for simulator ticks

### What to observe

**Simulation → Heater card:**
- `Temperature` reading: starts to drop slightly (thermal mass is losing heat faster)
- `Heating: X kW / 5 kW max`: power increases as the thermostat demands more

**Simulation → Power & Energy card:**
- Net import rises (more heater power being drawn)

**Controller → Power chart (solid trace lines):**
- `trace_heater` increases over time as the thermal model demands more power

**Controller → Plan card:**
- Trigger `PERIODIC` (routine replans)
- Firm cost increases (more kWh of heating required)

**Controller → Packets table:**
- If a heater packet exists: `Est. Cost €` increases due to higher energy target

### Temperature recovery
Drag ambient temp back to **10°C** — heater power should decrease as the thermal model requires less energy to maintain target temperature.

### Key insight to verify
The feedback loop is: ambient drops → heat loss rate increases → thermal model computes higher TargetEnergy → Planner allocates more heater kW → power chart shows higher `trace_heater`. This is all automatic with no user action beyond the slider.

---

## CLI Reference: POST /user-requests

The **User Requests** page (http://Pi4-Server:8214/user-requests) is the primary way to submit and cancel user requests. The curl commands below are provided as alternatives for scripting, automation, or quick access without opening a browser.

### EV charge to target SoC

```bash
# VEN1: charge EV to 80% by tomorrow 07:00, budget €3
curl -s -X POST http://Pi4-Server:8211/user-requests \
  -H "Content-Type: application/json" \
  -d '{
    "asset_id": "ev",
    "target_soc": 0.80,
    "desired_power_kw": 7.0,
    "deadlines": [{"latest_end": "2026-03-13T07:00:00+01:00", "max_total_cost_eur": 3.00}],
    "completion_policy": "CONTINUE"
  }' | python3 -m json.tool
```

### Multi-tier deadline (UC-09)

```bash
curl -s -X POST http://Pi4-Server:8211/user-requests \
  -H "Content-Type: application/json" \
  -d '{
    "asset_id": "ev",
    "target_soc": 0.80,
    "desired_power_kw": 7.0,
    "deadlines": [
      {"latest_end": "2026-03-12T22:00:00+01:00", "max_total_cost_eur": 5.00, "max_marginal_rate_eur_kwh": 0.50},
      {"latest_end": "2026-03-14T18:00:00+01:00", "max_total_cost_eur": 1.00, "max_marginal_rate_eur_kwh": 0.10}
    ],
    "completion_policy": "CONTINUE"
  }' | python3 -m json.tool
```

### Cancel a request

```bash
curl -s -X DELETE http://Pi4-Server:8211/user-requests/<REQUEST_ID>
```

### List all requests

```bash
curl -s http://Pi4-Server:8211/user-requests | python3 -m json.tool
```

### View active plan (full JSON including warnings)

```bash
curl -s http://Pi4-Server:8211/plan | python3 -m json.tool
```

### View flexibility envelopes

```bash
curl -s http://Pi4-Server:8211/flexibility | python3 -m json.tool
```

---

## Quick Reference: What Each UI Section Shows

| Section | UC-relevance |
|---|---|
| **User Requests page → New User Request form** | UC-01 (EV overnight), UC-02 (batch proxy), UC-09 (tier fallback) — submit user requests |
| **User Requests page → status chips** | All user request UCs — see ACTIVE / COMPLETED / CANCELLED / FAILED at a glance |
| **User Requests page → delete button** | UC-09 (cancel stalled user request and re-submit with corrected budgets) |
| **Status bar → Capacity card** | UC-07 (capacity limit from VTN), UC-06 (emergency) |
| **Status bar → Plan card** | UC-04/05 (trigger: RATE_CHANGE), UC-09 (warning count), UC-12 (stale rate warning) |
| **Status bar → Packets card** | All UCs with packets — counts by status |
| **Power chart → solid lines** | UC-03 (PV negative), UC-06 (EV drops to 0), UC-08 (EV drops at disconnect), UC-14 (heater increases) |
| **Power chart → dashed lines** | UC-01/05 (EV deferred to cheap window), UC-03 (battery charge allocation) |
| **Power chart → import_cap step** | UC-07, UC-06 (site limit) |
| **Rate chart** | UC-04 (price steps appear), UC-05 (cheap dip visible) |
| **Packets table → fill bar** | UC-01 (fills overnight), UC-08 (freezes on disconnect), UC-09 (stalls at 0%) |
| **Ledger** | UC-03 (export kWh growing), UC-01 (cost accumulating), UC-11 (export = 0.000) |
