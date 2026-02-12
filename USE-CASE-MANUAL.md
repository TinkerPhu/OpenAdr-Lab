# OpenADR Use Case Manual — Step-by-Step Replay Guide

This manual explains how to replay all 8 use cases from `USE-CASES.md` against the running OpenADR lab. Each use case includes the real-world motivation, a concrete example, and step-by-step UI instructions.

**VTN UI:** http://Pi4-Server:8221
**VEN UI:** http://Pi4-Server:8214

---

## Prerequisites

### Verify the System Is Running

1. Open the **VTN UI** at http://Pi4-Server:8221 — the health chip in the top bar should show **"VTN: ok"** (green)
2. Open the **VEN UI** at http://Pi4-Server:8214 — the health chip should show **"ok"** (green) for the selected VEN

### Understanding the Flow

Every use case follows the same OpenADR 3 pattern:

1. **Create a Program** on the VTN UI (with optional enrollment targets)
2. **Create an Event** under that program (with intervals and payload signals)
3. **VENs poll** the VTN and receive the event (within 30 seconds)
4. **VENs submit Reports** acknowledging the event (via VEN UI)
5. **VTN operator** can view reports on the VTN UI to confirm compliance

---

## UC1 — Emergency Load Shed

### Motivation

A heatwave pushes electricity demand beyond grid capacity. The utility needs to immediately reduce load to prevent a blackout. There is no time for gradual ramp-downs — the signal must be urgent and clear.

### Real-World Example

It's a 42°C afternoon in August. Air conditioning across the city is running at full blast. The grid operator sees frequency dropping and issues an emergency demand response signal: "All enrolled commercial buildings must reduce consumption by 100% of their curtailable load within the next 2 minutes, for 30 minutes."

### Key Characteristics

- **Priority 0** (highest — emergency)
- **Payload type: SIMPLE** (binary on/off signal: 0 = curtail)
- **Single interval** (one clear instruction)
- **Targeted** to specific VENs (only enrolled participants)

### Step-by-Step Replay (Web UI)

**VTN UI** (http://Pi4-Server:8221):

1. Navigate to **Programs** page
2. Click **"Create"** button
3. Enter Program Name: `manual-uc1-emergency`
4. Under **Enrolled VENs**, check **ven-1-name** (leave others unchecked)
5. Click **"Create"**
6. Navigate to **Events** page
7. Click **"Create"** button
8. Enter Event Name: `manual-emergency-loadshed`
9. Select Program: `manual-uc1-emergency` from the dropdown
10. Enter Priority: `0`
11. Paste into Intervals (JSON):
    ```json
    [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [0]}]}]
    ```
12. Click **"Create"**

**VEN UI** (http://Pi4-Server:8214):

13. Select **VEN1** in the VEN dropdown (top bar)
14. Navigate to **Events** — verify `manual-emergency-loadshed` appears (wait up to 30s for polling)
15. Select **VEN2** in the VEN dropdown
16. Navigate to **Events** — verify `manual-emergency-loadshed` does **NOT** appear (enrollment targeting)
17. Switch back to **VEN1** in the dropdown
18. Navigate to **Reports** → click **"Submit Report"**
19. Select Event: `manual-emergency-loadshed` from the dropdown
20. Click **"Suggest Example"** → click **"Submit"**

**VTN UI** (http://Pi4-Server:8221):

21. Navigate to **Reports** — verify a report from `ven-1` appears with the matching event ID

### What to Observe

- VEN-1 gets the event, VEN-2 does not (enrollment targeting works)
- Priority 0 marks this as the highest-urgency event
- The SIMPLE payload with value `0` represents "curtail now"
- The report round-trip confirms the VEN acknowledged the instruction

---

## UC2 — Renewable Export Limitation

### Motivation

On a sunny spring day, rooftop solar panels generate more electricity than the local grid can absorb. The grid operator must limit how much power distributed energy resources (DERs) export to the grid to prevent voltage rise and equipment damage.

### Real-World Example

A neighborhood with heavy solar PV penetration is over-generating at noon. The distribution network operator sends a 3-phase signal: first ramp export down to 50%, hold for 20 minutes, then ramp back up to 100%. This prevents sudden voltage spikes while allowing a smooth transition.

### Key Characteristics

- **Payload type: EXPORT_CAPACITY_LIMIT** (how much the VEN may export)
- **3 intervals** representing ramp-down / hold / ramp-up phases
- **Priority 5** (normal operational priority)
- **Targeted** to specific VENs with solar installations

### Step-by-Step Replay (Web UI)

**VTN UI** (http://Pi4-Server:8221):

1. Navigate to **Programs** page
2. Click **"Create"**
3. Enter Program Name: `manual-uc2-export`
4. Under **Enrolled VENs**, check **ven-2** only
5. Click **"Create"**
6. Navigate to **Events** page
7. Click **"Create"**
8. Enter Event Name: `manual-export-limit`
9. Select Program: `manual-uc2-export`
10. Enter Priority: `5`
11. Paste into Intervals (JSON):
    ```json
    [
      {"id": 0, "payloads": [{"type": "EXPORT_CAPACITY_LIMIT", "values": [100.0]}]},
      {"id": 1, "payloads": [{"type": "EXPORT_CAPACITY_LIMIT", "values": [50.0]}]},
      {"id": 2, "payloads": [{"type": "EXPORT_CAPACITY_LIMIT", "values": [100.0]}]}
    ]
    ```
12. Click **"Create"**

**VEN UI** (http://Pi4-Server:8214):

13. Select **VEN2** in the VEN dropdown
14. Navigate to **Events** — verify `manual-export-limit` appears with 3 intervals
15. Select **VEN1** in the VEN dropdown
16. Navigate to **Events** — verify `manual-export-limit` does **NOT** appear

### What to Observe

- The 3 intervals model a real ramp-down/hold/ramp-up sequence
- Values go 100 kW -> 50 kW -> 100 kW (limit drops, then recovers)
- Only VEN-2 (the solar site) receives the instruction

---

## UC3 — Dynamic Price Signal (Time-of-Use)

### Motivation

Instead of directly controlling devices, the grid operator publishes electricity prices that vary throughout the day. Smart devices optimize their behavior locally — running energy-intensive tasks during cheap off-peak hours and reducing consumption during expensive peak hours.

### Real-World Example

A utility publishes tomorrow's hourly electricity prices: $0.06/kWh at 3 AM (off-peak), rising to $0.40/kWh at 5 PM (peak). A smart EV charger sees the prices and schedules charging for 2-5 AM. A commercial building pre-cools overnight when prices are low rather than running AC at peak rates.

### Key Characteristics

- **Payload type: PRICE** (price signal per interval)
- **Multiple intervals** (typically 24 for hourly day-ahead pricing)
- **No targeting** (open program — all VENs see it)
- **No direct control mandate** — VENs decide locally how to respond

### Step-by-Step Replay (Web UI)

**VTN UI** (http://Pi4-Server:8221):

1. Navigate to **Programs** page
2. Click **"Create"**
3. Enter Program Name: `manual-uc3-pricing`
4. Leave **all VEN checkboxes unchecked** (open program — visible to all VENs)
5. Click **"Create"**
6. Navigate to **Events** page
7. Click **"Create"**
8. Enter Event Name: `manual-tou-pricing`
9. Select Program: `manual-uc3-pricing`
10. Enter Priority: `5`
11. Paste into Intervals (JSON):
    ```json
    [
      {"id": 0, "payloads": [{"type": "PRICE", "values": [0.12]}]},
      {"id": 1, "payloads": [{"type": "PRICE", "values": [0.35]}]},
      {"id": 2, "payloads": [{"type": "PRICE", "values": [0.15]}]}
    ]
    ```
12. Click **"Create"**

**VEN UI** (http://Pi4-Server:8214):

13. Select **VEN1** in the VEN dropdown
14. Navigate to **Events** — verify `manual-tou-pricing` appears
15. Select **VEN2** in the VEN dropdown
16. Navigate to **Events** — verify `manual-tou-pricing` also appears here (open program)

### What to Observe

- Both VENs see the event because the program has no enrollment targets
- The PRICE payload carries the price value per interval
- In a real deployment, the seed script creates 24 hourly intervals with realistic pricing curves (see `scripts/seed_vtn.py`)

---

## UC4 — Planned Peak Shaving

### Motivation

Weather forecasts predict high demand tomorrow afternoon. Unlike an emergency, this is planned in advance. The grid operator schedules a moderate curtailment event hours or days ahead, giving participants time to prepare (pre-cool buildings, shift production schedules).

### Real-World Example

The grid operator sees that tomorrow at 2 PM, demand will peak at 95% of grid capacity. They create a 4-hour peak shaving event: "All enrolled commercial sites must limit import to 50 kW between 14:00 and 18:00." Building managers receive the signal the day before and adjust their HVAC schedules accordingly.

### Key Characteristics

- **Payload type: IMPORT_CAPACITY_LIMIT** (maximum power a site may draw)
- **intervalPeriod** with explicit start time and duration (scheduled)
- **Priority 3** (moderate — planned, not emergency)
- **Targeted** to multiple VENs

### Step-by-Step Replay (Web UI)

**VTN UI** (http://Pi4-Server:8221):

1. Navigate to **Programs** page
2. Click **"Create"**
3. Enter Program Name: `manual-uc4-peak`
4. Under **Enrolled VENs**, check both **ven-1-name** and **ven-2**
5. Click **"Create"**
6. Navigate to **Events** page
7. Click **"Create"**
8. Enter Event Name: `manual-peak-shave`
9. Select Program: `manual-uc4-peak`
10. Enter Priority: `3`
11. Enter Start Time: `2026-03-01T14:00:00Z`
12. Enter Duration: `PT4H`
13. Paste into Intervals (JSON):
    ```json
    [{"id": 0, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [50.0]}]}]
    ```
14. Click **"Create"**

**VEN UI** (http://Pi4-Server:8214):

15. Select **VEN1** in the VEN dropdown
16. Navigate to **Events** — verify `manual-peak-shave` appears with an intervalPeriod
17. Select **VEN2** in the VEN dropdown
18. Navigate to **Events** — verify `manual-peak-shave` also appears (both VENs enrolled)

### What to Observe

- The `intervalPeriod` tells VENs exactly when the event starts and how long it lasts
- Both VENs receive the event because both are enrolled
- The IMPORT_CAPACITY_LIMIT of 50.0 means "draw no more than 50 kW"
- This is the pattern for any scheduled, non-emergency curtailment

---

## UC5 — EV Charging Management

### Motivation

During evening peak hours, thousands of electric vehicles plug in to charge simultaneously. The grid cannot handle the combined load. The operator needs to pause or throttle EV charging for specific charging stations.

### Real-World Example

A fleet of 50 EVs returns to the company parking garage at 5 PM and all plug in. The grid operator sends a 2-hour signal: "Pause all charging for 1 hour (0 kW import), then resume at reduced power (7.4 kW max) for the second hour." The fleet still gets enough charge by morning, and the grid avoids a 350 kW spike.

### Key Characteristics

- **Payload type: IMPORT_CAPACITY_LIMIT** (0 = pause charging)
- **Event-level targets** (targets on the event itself, not just the program)
- **Priority 2** (high — grid stability concern)
- **Group-based** — targets specific VENs representing charging infrastructure

### Step-by-Step Replay (Web UI)

**VTN UI** (http://Pi4-Server:8221):

1. Navigate to **Programs** page
2. Click **"Create"**
3. Enter Program Name: `manual-uc5-ev`
4. Under **Enrolled VENs**, check **ven-2** only
5. Click **"Create"**
6. Navigate to **Events** page
7. Click **"Create"**
8. Enter Event Name: `manual-ev-charge-control`
9. Select Program: `manual-uc5-ev`
10. Enter Priority: `2`
11. Paste into Targets (JSON):
    ```json
    [{"type": "VEN_NAME", "values": ["ven-2"]}]
    ```
12. Paste into Intervals (JSON):
    ```json
    [{"id": 0, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [0.0]}]}]
    ```
13. Click **"Create"**

**VEN UI** (http://Pi4-Server:8214):

14. Select **VEN2** in the VEN dropdown
15. Navigate to **Events** — verify `manual-ev-charge-control` appears
16. Select **VEN1** in the VEN dropdown
17. Navigate to **Events** — verify `manual-ev-charge-control` does **NOT** appear

### What to Observe

- The event has its own `targets` array (event-level targeting), in addition to the program's enrollment
- `IMPORT_CAPACITY_LIMIT` of `0.0` means "stop drawing power" (pause charging)
- Only VEN-2 (the EV site) sees the event

---

## UC6 — Battery Dispatch Window

### Motivation

Battery storage systems need directional control: charge when electricity is cheap or solar is abundant, discharge during peak demand. The grid operator sends a multi-phase dispatch schedule telling batteries what to do and when.

### Real-World Example

A 500 kWh community battery receives a 3-phase dispatch:
- **Phase 1 (10:00-12:00):** Charge at 80% of max rate (absorb midday solar surplus)
- **Phase 2 (12:00-13:00):** Idle (0% — hold state of charge)
- **Phase 3 (13:00-15:00):** Discharge at 50% of max rate (feed back during afternoon peak)

### Key Characteristics

- **Payload type: CHARGE_STATE_SETPOINT** (positive = charge, negative = discharge)
- **3 intervals** with different directional commands
- **Priority 3** (planned dispatch)
- **Targeted** to the VEN controlling the battery

### Step-by-Step Replay (Web UI)

**VTN UI** (http://Pi4-Server:8221):

1. Navigate to **Programs** page
2. Click **"Create"**
3. Enter Program Name: `manual-uc6-battery`
4. Under **Enrolled VENs**, check **ven-1-name** only
5. Click **"Create"**
6. Navigate to **Events** page
7. Click **"Create"**
8. Enter Event Name: `manual-battery-dispatch`
9. Select Program: `manual-uc6-battery`
10. Enter Priority: `3`
11. Paste into Intervals (JSON):
    ```json
    [
      {"id": 0, "payloads": [{"type": "CHARGE_STATE_SETPOINT", "values": [80.0]}]},
      {"id": 1, "payloads": [{"type": "CHARGE_STATE_SETPOINT", "values": [0.0]}]},
      {"id": 2, "payloads": [{"type": "CHARGE_STATE_SETPOINT", "values": [-50.0]}]}
    ]
    ```
12. Click **"Create"**

**VEN UI** (http://Pi4-Server:8214):

13. Select **VEN1** in the VEN dropdown
14. Navigate to **Events** — verify `manual-battery-dispatch` appears with 3 intervals
15. Click the event row to inspect details — values should be 80.0 (charge), 0.0 (idle), -50.0 (discharge)
16. Select **VEN2** in the VEN dropdown
17. Navigate to **Events** — verify `manual-battery-dispatch` does **NOT** appear

### What to Observe

- Positive values (80.0) mean "charge at this setpoint"
- Zero (0.0) means "idle / hold"
- Negative values (-50.0) mean "discharge at this setpoint"
- The 3-interval pattern models a real charge/idle/discharge cycle

---

## UC7 — Connectivity Check (Heartbeat)

### Motivation

Grid operators need to know which VENs are online and responsive before sending critical events. A connectivity check is a no-op event that does nothing operationally — it simply verifies the full round-trip: VEN polls the event, processes it, and submits a report back.

### Real-World Example

Before the summer demand response season begins, the utility sends a "heartbeat" event to all enrolled VENs. VENs that respond with a report are confirmed active. VENs that don't respond within 24 hours are flagged for manual follow-up. This is like a roll call before a fire drill.

### Key Characteristics

- **Payload type: SIMPLE** with value `0` (no operational impact)
- **Open program** (all VENs should respond)
- **Priority 5** (low — informational only)
- **Report round-trip** is the key verification

### Step-by-Step Replay (Web UI)

**VTN UI** (http://Pi4-Server:8221):

1. Navigate to **Programs** page
2. Click **"Create"**
3. Enter Program Name: `manual-uc7-connectivity`
4. Leave **all VEN checkboxes unchecked** (open program)
5. Click **"Create"**
6. Navigate to **Events** page
7. Click **"Create"**
8. Enter Event Name: `manual-heartbeat`
9. Select Program: `manual-uc7-connectivity`
10. Enter Priority: `5`
11. Paste into Intervals (JSON):
    ```json
    [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [0]}]}]
    ```
12. Click **"Create"**

**VEN UI** (http://Pi4-Server:8214):

13. Select **VEN1** in the VEN dropdown
14. Navigate to **Events** — verify `manual-heartbeat` appears
15. Navigate to **Reports** → click **"Submit Report"**
16. Select Event: `manual-heartbeat` → click **"Suggest Example"** → click **"Submit"**
17. Select **VEN2** in the VEN dropdown
18. Navigate to **Events** — verify `manual-heartbeat` also appears
19. Navigate to **Reports** → click **"Submit Report"**
20. Select Event: `manual-heartbeat` → click **"Suggest Example"** → click **"Submit"**

**VTN UI** (http://Pi4-Server:8221):

21. Navigate to **Reports** — verify reports from both `ven-1` and `ven-2` appear

### What to Observe

- The event itself has no operational effect (SIMPLE with value 0)
- The value is in the report round-trip: did the VEN respond?
- Both VENs report back, confirming they are online and processing events
- In production, VENs that fail to report would be flagged for investigation

---

## UC8 — Event Cancellation

### Motivation

Grid conditions change. A predicted demand peak might not materialize (e.g., a cold front arrives), or a renewable curtailment is no longer needed because congestion cleared. The operator must cancel an active or upcoming event so VENs return to normal operation.

### Real-World Example

At 1 PM, the grid operator creates a peak shaving event for 4-6 PM. At 3 PM, a large industrial customer unexpectedly shuts down, and the predicted peak is no longer a concern. The operator cancels the event. VENs detect the cancellation on their next poll cycle and resume normal operation — the curtailment never activates.

### Key Characteristics

- **OpenADR 3 cancellation = event deletion** (there is no "cancel" status)
- VENs detect cancellation when the event disappears from their poll results
- The VEN must cleanly roll back any preparation it made for the event

### Step-by-Step Replay (Web UI)

**VTN UI** (http://Pi4-Server:8221):

1. Navigate to **Programs** page
2. Click **"Create"**
3. Enter Program Name: `manual-uc8-cancel`
4. Under **Enrolled VENs**, check **ven-1-name** only
5. Click **"Create"**
6. Navigate to **Events** page
7. Click **"Create"**
8. Enter Event Name: `manual-cancel-test`
9. Select Program: `manual-uc8-cancel`
10. Enter Priority: `5`
11. Paste into Intervals (JSON):
    ```json
    [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [1]}]}]
    ```
12. Click **"Create"**

**VEN UI** (http://Pi4-Server:8214):

13. Select **VEN1** in the VEN dropdown
14. Navigate to **Events** — verify `manual-cancel-test` appears (wait up to 30s)

**VTN UI** (http://Pi4-Server:8221):

15. Navigate to **Events** page
16. Find `manual-cancel-test` in the table
17. Click the **delete icon** (trash can) on that event row
18. In the confirmation dialog, click **"Delete"**

**VEN UI** (http://Pi4-Server:8214):

19. Select **VEN1** in the VEN dropdown (if not already selected)
20. Navigate to **Events** — verify `manual-cancel-test` has **disappeared** (wait up to 30s for next poll)

### What to Observe

- The event appears on VEN-1 after creation (step 14)
- After deletion, the event vanishes from VEN-1's event list (step 20)
- This is how OpenADR 3 handles cancellation — there is no "cancelled" status field
- VENs must detect the absence of a previously-seen event and react accordingly

---

## Automated Replay

### Seed Script (All Use Cases at Once)

The seed script creates programs and events for all 8 use cases with realistic timing:

```bash
python3 scripts/seed_vtn.py --vtn-url http://Pi4-Server:8200
```

To also demo UC8 cancellation (creates event, waits 5s, deletes it):

```bash
python3 scripts/seed_vtn.py --vtn-url http://Pi4-Server:8200 --demo-cancel
```

The seed script is **idempotent** — safe to run multiple times. It updates existing programs and replaces stale events with fresh timings.

### Automated Test Suite

Run the full E2E test suite (all 8 use cases, API + UI):

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab/tests && docker compose -f docker-compose.test.yml run --rm test"
```

This executes 49 scenarios (348 steps) including all use cases via both API calls and browser-driven UI interactions.

---

## CLI Reference (curl commands)

<details>
<summary>Click to expand curl commands for all use cases</summary>

### Prerequisites

```bash
TOKEN=$(curl -s -X POST http://Pi4-Server:8200/auth/token \
  -d "grant_type=client_credentials&client_id=any-business&client_secret=any-business" \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['access_token'])")
```

### UC1 — Emergency Load Shed

```bash
# Create targeted program
curl -s -X POST http://Pi4-Server:8200/programs \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"programName": "manual-uc1-emergency", "targets": [{"type": "VEN_NAME", "values": ["ven-1-name"]}]}' | python3 -m json.tool

# Save PROGRAM_ID from response, then create event
curl -s -X POST http://Pi4-Server:8200/events \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventName": "manual-emergency-loadshed", "priority": 0, "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [0]}]}]}' | python3 -m json.tool

# Verify VEN-1 received it
curl -s http://Pi4-Server:8211/events | python3 -m json.tool

# Verify VEN-2 did NOT receive it
curl -s http://Pi4-Server:8212/events | python3 -m json.tool

# Submit report from VEN-1
EVENT_ID=$(curl -s http://Pi4-Server:8211/events | python3 -c "import sys,json; evts=json.load(sys.stdin); print(next(e['id'] for e in evts if e['eventName']=='manual-emergency-loadshed'))")
curl -s -X POST http://Pi4-Server:8211/reports \
  -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventID": "'$EVENT_ID'", "clientName": "ven-1", "resources": []}' | python3 -m json.tool

# Verify report in VTN
curl -s http://Pi4-Server:8200/reports -H "Authorization: Bearer $TOKEN" | python3 -m json.tool
```

### UC2 — Renewable Export Limitation

```bash
curl -s -X POST http://Pi4-Server:8200/programs \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programName": "manual-uc2-export", "targets": [{"type": "VEN_NAME", "values": ["ven-2"]}]}' | python3 -m json.tool

curl -s -X POST http://Pi4-Server:8200/events \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventName": "manual-export-limit", "priority": 5, "intervals": [{"id": 0, "payloads": [{"type": "EXPORT_CAPACITY_LIMIT", "values": [100.0]}]}, {"id": 1, "payloads": [{"type": "EXPORT_CAPACITY_LIMIT", "values": [50.0]}]}, {"id": 2, "payloads": [{"type": "EXPORT_CAPACITY_LIMIT", "values": [100.0]}]}]}' | python3 -m json.tool
```

### UC3 — Dynamic Price Signal

```bash
curl -s -X POST http://Pi4-Server:8200/programs \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programName": "manual-uc3-pricing", "targets": null}' | python3 -m json.tool

curl -s -X POST http://Pi4-Server:8200/events \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventName": "manual-tou-pricing", "priority": 5, "intervals": [{"id": 0, "payloads": [{"type": "PRICE", "values": [0.12]}]}, {"id": 1, "payloads": [{"type": "PRICE", "values": [0.35]}]}, {"id": 2, "payloads": [{"type": "PRICE", "values": [0.15]}]}]}' | python3 -m json.tool
```

### UC4 — Planned Peak Shaving

```bash
curl -s -X POST http://Pi4-Server:8200/programs \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programName": "manual-uc4-peak", "targets": [{"type": "VEN_NAME", "values": ["ven-1-name"]}, {"type": "VEN_NAME", "values": ["ven-2"]}]}' | python3 -m json.tool

curl -s -X POST http://Pi4-Server:8200/events \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventName": "manual-peak-shave", "priority": 3, "intervalPeriod": {"start": "2026-03-01T14:00:00Z", "duration": "PT4H"}, "intervals": [{"id": 0, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [50.0]}]}]}' | python3 -m json.tool
```

### UC5 — EV Charging Management

```bash
curl -s -X POST http://Pi4-Server:8200/programs \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programName": "manual-uc5-ev", "targets": [{"type": "VEN_NAME", "values": ["ven-2"]}]}' | python3 -m json.tool

curl -s -X POST http://Pi4-Server:8200/events \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventName": "manual-ev-charge-control", "priority": 2, "targets": [{"type": "VEN_NAME", "values": ["ven-2"]}], "intervals": [{"id": 0, "payloads": [{"type": "IMPORT_CAPACITY_LIMIT", "values": [0.0]}]}]}' | python3 -m json.tool
```

### UC6 — Battery Dispatch Window

```bash
curl -s -X POST http://Pi4-Server:8200/programs \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programName": "manual-uc6-battery", "targets": [{"type": "VEN_NAME", "values": ["ven-1-name"]}]}' | python3 -m json.tool

curl -s -X POST http://Pi4-Server:8200/events \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventName": "manual-battery-dispatch", "priority": 3, "intervals": [{"id": 0, "payloads": [{"type": "CHARGE_STATE_SETPOINT", "values": [80.0]}]}, {"id": 1, "payloads": [{"type": "CHARGE_STATE_SETPOINT", "values": [0.0]}]}, {"id": 2, "payloads": [{"type": "CHARGE_STATE_SETPOINT", "values": [-50.0]}]}]}' | python3 -m json.tool
```

### UC7 — Connectivity Check

```bash
curl -s -X POST http://Pi4-Server:8200/programs \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programName": "manual-uc7-connectivity", "targets": null}' | python3 -m json.tool

curl -s -X POST http://Pi4-Server:8200/events \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventName": "manual-heartbeat", "priority": 5, "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [0]}]}]}' | python3 -m json.tool

# Submit reports from VENs
EVENT_ID=$(curl -s http://Pi4-Server:8211/events | python3 -c "import sys,json; evts=json.load(sys.stdin); print(next(e['id'] for e in evts if e['eventName']=='manual-heartbeat'))")

curl -s -X POST http://Pi4-Server:8211/reports -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventID": "'$EVENT_ID'", "clientName": "ven-1", "resources": []}' | python3 -m json.tool

curl -s -X POST http://Pi4-Server:8212/reports -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventID": "'$EVENT_ID'", "clientName": "ven-2", "resources": []}' | python3 -m json.tool
```

### UC8 — Event Cancellation

```bash
curl -s -X POST http://Pi4-Server:8200/programs \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programName": "manual-uc8-cancel", "targets": [{"type": "VEN_NAME", "values": ["ven-1-name"]}]}' | python3 -m json.tool

curl -s -X POST http://Pi4-Server:8200/events \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"programID": "'$PROGRAM_ID'", "eventName": "manual-cancel-test", "priority": 5, "intervals": [{"id": 0, "payloads": [{"type": "SIMPLE", "values": [1]}]}]}' | python3 -m json.tool

# Save EVENT_ID, verify VEN-1 received it, then delete
curl -s -X DELETE http://Pi4-Server:8200/events/$EVENT_ID -H "Authorization: Bearer $TOKEN"

# Verify VEN-1 no longer has the event
curl -s http://Pi4-Server:8211/events | python3 -m json.tool
```

</details>

---

## Quick Reference: Payload Types

| Payload Type | Use Case | Meaning |
|---|---|---|
| `SIMPLE` | UC1, UC7, UC8 | Binary signal (0 = curtail/off, 1 = normal) |
| `EXPORT_CAPACITY_LIMIT` | UC2 | Max power the VEN may export (kW) |
| `PRICE` | UC3 | Electricity price per interval ($/kWh) |
| `IMPORT_CAPACITY_LIMIT` | UC4, UC5 | Max power the VEN may draw (kW). 0 = stop |
| `CHARGE_STATE_SETPOINT` | UC6 | Battery charge (+) or discharge (-) setpoint |

## Quick Reference: Priority Values

| Priority | Meaning | Example |
|---|---|---|
| 0 | Emergency / highest | UC1: Emergency load shed |
| 2 | High | UC5: EV charging pause |
| 3 | Moderate | UC4, UC6: Planned events |
| 5 | Normal / low | UC2, UC3, UC7: Routine operations |
| null | Unspecified | Informational events |

## Quick Reference: Targeting

| Targeting | Visibility | Example |
|---|---|---|
| Check specific VEN checkboxes | Only those VENs | UC1, UC6 |
| Check multiple VEN checkboxes | Multiple VENs | UC4 |
| Leave all checkboxes unchecked | All VENs (open) | UC3, UC7 |
| Event-level Targets (JSON) | Further restrict within program | UC5 |
