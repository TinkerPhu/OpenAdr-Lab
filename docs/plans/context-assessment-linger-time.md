# Context Assessment: `min_state_linger_s` Scope & Interactions

> Status: pre-implementation  
> Context: Deviation Control — Phase 28 planning

---

## Where `min_state_linger_s` will propagate

### 1. **Profile schema** (primary)
- Each asset in `absorber_assets` declares `min_state_linger_s: u64` (seconds between state changes).
- Test profile: `0` everywhere (fast BDD).
- Production profiles: `0` for battery/EV, `30–60` for heater/boiler.
- **Decision needed**: Should this field also be in the main `assets:` list (for all asset types), or only in `absorber_assets` (for absorber-controlled assets)?

### 2. **Absorber logic** (enforcement)
- Before changing an asset's control state (ON→OFF or OFF→ON), check:
  ```
  time_since_last_state_change >= min_state_linger_s
  ```
- If violated, refuse the state change and move to the next priority asset.
- Track `last_state_change_ts` per asset in the absorber's local state.
- **Question**: Does the absorber operate at the binary (on/off) level, or at the continuous setpoint level?
  - If binary: `min_state_linger_s` prevents relay toggle; straightforward.
  - If continuous: A setpoint change (e.g., 5 kW → 0 kW) is a state change; need to detect crossings.

### 3. **Dispatcher (setpoint application)** (observation point)
- The dispatcher reads the MILP plan and applies setpoints to assets each tick.
- It could optionally observe state changes and update `last_state_change_ts`.
- Alternatively, the absorber handles all state tracking (cleaner, since absorber is new anyway).

### 4. **MILP planner** (soft penalty vs. hard constraint)
- **Current state**: HeaterConfig already has `switching_penalty_eur` (line 135 in profile.rs).
- **MILP model**: The heater's `HeaterMilpContext` carries `lambda_sw_eur` and binary variables `z_heat_mid`, `z_heat_full`.
- **Switching cost**: The MILP penalizes transitions via a `sw` variable (switching cost per step).
- **Decision**: Does `min_state_linger_s` add a **hard constraint** that the MILP must also respect (Option B from doc)?
  - **Option A** (current proposal): MILP ignores linger; absorber enforces at runtime. Plans may become temporarily infeasible.
  - **Option B** (stricter): MILP models state machines with minimum dwell time. Harder to implement; solve times increase.
  - **Recommendation**: Start with Option A. Use `switching_penalty_eur` for soft optimization, `min_state_linger_s` for hard safety at runtime.

### 5. **Reactor (state machine driver)**
- The reactor currently implements FSM states (Idle → Delaying → Ramping → Holding → RampingBack → Idle).
- It doesn't currently track relay state changes explicitly.
- **Question**: Should the reactor's state machine aware of linger time?
  - Current design: reactor models control *intent* (ramp, delay, hold); heater's `step_inner()` quantizes setpoint to a physical tier (0 / mid / full).
  - The actual relay toggle happens in `step_inner()` when quantization crosses a tier boundary.
  - Linger tracking could live in: (a) the reactor's state per asset, (b) the absorber, or (c) a separate "asset control constraint" layer.
  - **Cleanest**: Absorber tracks linger per asset it controls; reactor is unaware (reactor is pre-absorber, higher-level intent).

### 6. **Sim state persistence & restart**
- If the VEN process restarts, does it know the last relay state change time?
- **Current**: Sim state is persisted to `/data/sim_state.json`, but `last_state_change_ts` is not part of `SimState`.
- **Decision**: Add `last_state_change_ts: HashMap<String, DateTime<Utc>>` to `SimState`? Or restart with all assets free to change?
  - **Recommendation**: Persist it. If heater was ON for 20s before restart, the absorber should still respect the remaining 10s linger on reboot (if linger=30s).
  - Alternatively: On restart, assume all relays are "fresh" (last changed at boot). This is simpler but potentially unsafe (could flip a relay immediately after boot if recent history is lost).

### 7. **Opportunistic EV overlay**
- Opportunistic EV computes a persistent overlay based on current MILP setpoint (once per slot).
- Does it respect linger time? Probably not — it's a high-level intent (exploit cheap tariff), not a real-time feedback loop.
- **Question**: Should opportunistic be aware of EV charger's linger time (if EV has mechanical relays)?
  - Most EV chargers are solid-state (no mechanical relays), so `min_state_linger_s = 0`.
  - If a user has an old EV with relay-based contactor, they'd set `min_state_linger_s > 0` for EV.
  - Opportunistic doesn't need special logic; it just sets a setpoint, and the absorber / reactor enforce linger.

### 8. **User request overlay**
- Users can POST explicit control requests (turn heater ON, charge EV at X kW).
- Should user requests bypass linger, or respect it?
- **Recommendation**: Respect it. If a user's "turn heater on now" request arrives, queue it but don't apply if linger blocks. This prevents wear damage from user mistakes.
  - Alternatively: Fast-path user requests (override linger). This gives users emergency control but risks relay damage.
  - **Decide later** based on operational feedback.

### 9. **Heater thermostat emergency control**
- The heater has a thermostat that forces ON if `temp <= temp_min_c` (line 97 in heater.rs).
- The emergency override has its own hysteresis (line 96: `EMERGENCY_HYSTERESIS_C = 3°C`).
- **Question**: Should the thermostat emergency respect linger time?
  - If yes: thermal safety might be compromised (user set temp_min_c=15°C for comfort; linger blocks the heater for 30s, temp drops further).
  - If no: emergency can bypass linger. This is acceptable — safety > wear.
  - **Recommendation**: Emergency overrides linger. The thermostat emergency is hardwired safety, not a control decision. If tank gets too cold, the heater must turn on *now*, linger be damned.

### 10. **Heater mode transitions (mid ↔ full power)**
- Heater can operate at three power levels: 0 / mid / full.
- Is a transition from 0→mid a "state change" (counts toward linger)? Or only 0↔full?
- **Interpretation A** (binary state): state = [OFF, ON]. Transition from 0→mid is ON (counts). mid→full is still ON (no count).
- **Interpretation B** (multi-state): state = [0, mid, full]. Each transition counts.
- **Recommendation**: Interpretation B (multi-state). If linger=30s, each transition (0→mid, mid→full, full→mid, mid→0) should be separated by 30s. This is stricter but more conservative for relay life.

---

## Summary: Decisions before coding

| Topic | Status | Decision |
|-------|--------|----------|
| Schema location | **DECIDED** | Add `min_state_linger_s` to main asset profile (applies to all control paths: absorber, dispatcher, thermostat, user requests). |
| Binary vs. multi-state | **DECIDED** | Multi-state (0 / mid / full for heater). Each transition counts as state change with wear. |
| MILP integration | **DECIDED** | Start with Option A (soft penalty, hard absorber enforcement). |
| Sim state persistence | **DECIDED** | Piggyback on existing SimState infrastructure only; no new persistence layer. In-memory tracking per tick sufficient for now. |
| Thermostat emergency | **DECIDED** | Emergency heating bypasses linger (safety > wear). Can flip relay immediately when `temp ≤ temp_min_c`. |
| User requests | **DEFERRED** | Respect linger for now; escalate if users demand bypass. |
| Opportunistic EV | **NO ACTION** | EV charger linger is user-configurable (0 default); opportunistic unaware. |
| Reactor awareness | **NO ACTION** | Reactor is pre-absorber; absorber handles linger tracking. |

---

## Implementation sequence

1. Add `min_state_linger_s: u64` to all asset config structs (EvConfig, HeaterConfig, BatteryConfig, etc.) in `profile.rs`.
2. Create a global `StateChangeTracker: HashMap<String, DateTime<Utc>>` (in-memory per tick; no persistence).
3. Before any control layer (absorber, dispatcher, thermostat, user request) changes a relay state, check:
   ```
   time_since_last_state_change >= min_state_linger_s
   ```
4. On state change, update `StateChangeTracker[asset_id] = now`.
5. For heater: track state as discrete tier (0, mid, full); any transition counts.
6. Thermostat emergency heating bypasses the linger check (always allowed to turn ON if `temp ≤ temp_min_c`).
7. Absorber enforces linger: if blocked, move to next asset in priority order.

---

## Design notes

- **Multi-layer enforcement**: Each control layer (absorber, dispatcher, thermostat, user request) independently checks linger before issuing a state change. This is redundant but safe — no control path can accidentally bypass it.
- **Heater tier transitions**: Treating 0→mid, mid→full, full→mid, mid→0 all as state changes is conservative. If linger=30s, a heater must stay in one tier for 30s before moving to another. This matches real relay switching wear.
- **In-memory tracking**: `StateChangeTracker` is not persisted. After a restart, all relays are "fresh" (last_change_ts at boot time). This is acceptable because (a) restarts are rare, (b) the first 30s after boot, the system is under-constrained but not unsafe.
- **Thermostat emergency**: The thermostat forces heater ON at `temp ≤ temp_min_c` with hysteresis to `temp_min_c + 3°C`. This is safety-critical (prevent tank freeze). Bypassing linger for emergency is correct — the alternative (blocking emergency due to linger) could cause equipment damage or pipe burst.
