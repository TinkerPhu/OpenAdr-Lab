# VEN Runtime Override Capability — Planning

## Context

OpenADR signals are advisory. The VEN (representing the site owner) always retains the right to override grid operator commands. Our reactor already models different compliance levels via YAML profiles, but these are static — configured at deploy time, not adjustable at runtime.

A realistic VEN needs a runtime override mechanism so that a building manager can say: "I know the grid wants me to curtail, but my server room is overheating — override the HVAC restriction."

## Current State

| Profile Setting | Meaning | Current Usage |
|---|---|---|
| `strategy: instant` | Full immediate compliance | — |
| `strategy: ramp` | Ramp to full compliance over `ramp_duration_s` | VEN-1 (300s ramp) |
| `strategy: delayed` | Wait `delay_s`, then ramp to full compliance | VEN-2 (60s delay + 120s ramp) |
| `strategy: partial` | Apply only `compliance` fraction of curtailment | VEN-3 (70% compliance) |
| `strategy: ignore` | Completely ignore all events | Available but unused |

These are **baked into the YAML profile at deploy time**. The end user has no way to override a decision at runtime.

## Proposed Design

### API Endpoints

- `GET /override` — current override state (active overrides, remaining timeout)
- `POST /override` — set overrides (global toggle and/or per-device)
- `DELETE /override` — clear all overrides, return to normal reactor control

### Override Model

```json
{
  "active": true,
  "reason": "Server room cooling emergency",
  "expires_at": "2026-02-15T15:30:00Z",
  "global": false,
  "devices": {
    "heater": { "override": true, "setpoint_kw": 5.0 },
    "ev": { "override": false },
    "pv": { "override": false }
  }
}
```

### Override Levels

1. **Global override** — ignore all events, return all devices to defaults (equivalent to runtime `strategy: ignore`)
2. **Per-device override** — override specific devices while letting the reactor control others (e.g., "keep heater running but let EV and PV be curtailed")

### Behavior

- Reactor checks override state **after** computing setpoints but **before** applying them
- Overridden devices use either their default setpoint or an explicit user-provided setpoint
- Override auto-expires after the specified timeout to prevent forgotten overrides
- Decision trace logs override events with the user-provided reason
- Reports to VTN reflect **actual** behavior (overridden), not the requested curtailment

### VEN UI — Override Panel

- Toggle switch for global override
- Per-device toggle switches (only for devices present in the VEN profile)
- Reason text field (required)
- Timeout selector (15min, 30min, 1h, 2h, custom)
- Visual indicator in the header when any override is active (e.g., warning badge)
- Override history visible in the decision trace

### Implementation Steps

1. Add `OverrideState` struct to VEN app state (with `Arc<RwLock<>>`)
2. Add `GET/POST/DELETE /override` endpoints
3. Modify reactor `evaluate()` to check override state and replace setpoints
4. Add override entries to decision trace
5. Add override timeout expiry check in the simulator tick loop
6. Build VEN UI override panel
7. Add behave test scenarios for override behavior
8. Update USE-CASE-MANUAL with an override walkthrough

### Open Questions

- Should overrides persist across VEN restarts (save to `/data/override_state.json`)?
- Should the VTN be notified of active overrides (via a special report)?
- Should there be an "emergency restore" that clears all overrides if a priority-0 event arrives?
