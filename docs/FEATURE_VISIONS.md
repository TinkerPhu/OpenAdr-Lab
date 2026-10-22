# Feature Visions

Ideas that would be genuinely useful but cannot be implemented today because a
required external input doesn't exist yet — no sensor, no simulated signal, no
data source for the code to observe. This is a different blocking condition
than an item in `docs/BACKLOG.md` or `docs/reference/TECHNICAL_DEBTS.md`: those
are blocked (if at all) on unbuilt *internal* software that's on this project's
own roadmap and buildable anytime someone does the work. An item belongs here
only when it names the specific missing external input and the fix is
meaningless without it — not as a general home for low-priority ideas.

IDs are stable and never reused; gaps in the numbering are items that moved
back out once their blocking input arrived (implement via the normal
BACKLOG.md/TECHNICAL_DEBTS.md flow at that point, then remove from here).

---

### R-58: Verify `PlanTrigger::CapacityChange`/`Alert` cover asset-level faults

**Req:** entities/asset.rs, services/planning/mod.rs, assets/
**Problem:** Unconfirmed whether `PlanTrigger::CapacityChange`/`Alert` are wired to
asset-level faults (thermal derate, BMS fault, breaker trip) rather than only
tariff/VTN-sourced capacity changes.
**Missing input:** Traced 2026-08-23 — no fault/health signal exists anywhere in the
codebase to wire this to. `entities/asset.rs` has no health/fault field;
`design_vocabulary.rs` has no quarantined sketch of one either. All current
`PlanTrigger::CapacityChange`/`Alert` call sites (`tasks/poll_signals.rs`,
`tasks/poll_events/detect.rs`, `controller/trace.rs`) are exclusively tariff/VTN-sourced.
The simulator's `SimInjectState` supports SoC/irradiance/temperature/setpoint/capacity
overrides and two "emergency curtail/absorb" stand-ins, but those are documented
placeholders for a VTN directive, not an asset fault. A third variant,
`PlanTrigger::AssetStateChange` ("device connected/disconnected/failed"), exists but
its only two call sites (`routes/sim.rs`) send it generically from `/sim/inject` or a
manual `/plan/trigger` — never gated on any fault condition.
**What's needed before this is buildable:** a fault/health field on the asset entity
(or `SimInjectState`) representing at least one real fault class (thermal derate is the
most physically grounded — the simulator already has a thermal model), a simulator
injection path for it, and a detection call site that turns it into a `PlanTrigger`.
That's new input plumbing, not a wiring fix — hence parked here rather than in
`TECHNICAL_DEBTS.md`.
**Gain:** Medium — an unwired asset fault could silently fail to trigger a needed
replan, once such a fault can exist at all.
**Complexity:** Small once the input exists; the input itself is Medium.

---

### R-151: Heat pumps and air conditioners as a compressor-asset class

**Req:** assets/ (a new kind beside `heater`), entities/asset.rs, simulator/, controller/milp_planner/ (via `AssetMilpContext`), controller/arbiter/opportunistic.rs
**Problem:** The only thermal asset is the resistive `heater`: power is continuous from 0 to `max_kw`, switching costs nothing, and its thermal energy is electric energy one to one. A heat pump or an air conditioner (AC) is a different class, and fitting it into `heater` would put a second concept in one place:
- **Minimum run and minimum off time.** A compressor must run for a minimum interval once started (typically minutes, to return oil and avoid short-cycling wear) and must stay off for a minimum interval after stopping (pressure equalisation). A planner that treats it like a heater would switch it every slot; the 5-minute slot grid is shorter than some of these intervals, so the constraint spans slots.
- **Staged or modulated power.** Fixed-speed units are on/off at rated power; inverter units modulate between a minimum and a maximum, and below the minimum they must switch off, not run low. This is the `PowerAdjustability` shape of a minimum-power load, not a continuous one.
- **Electric to thermal conversion is not one to one.** The coefficient of performance (COP, thermal kWh per electric kWh) depends on the outdoor temperature and the supply temperature, so the time to reach a target (`Asset::full_effort_time_to_target_s`) and the unserved energy of GB-58 (reported in electric-equivalent kWh) both come from a curve, not from a constant.
- **Defrost and start-up transients** (heat pump) and **a latency before the room responds** (AC) make the opportunistic objective's latest-start rule (`opportunistic-deadline-dispatch`) need a lead time that is the asset's own.
- **An AC is also a thermal load on the building**: its demand follows indoor and outdoor temperature and solar gain, not a stored tank, so its flexibility is a temperature band, not an energy target.
**Missing input:** a heat-pump/AC model with real figures: a COP curve over outdoor and supply temperature, rated and minimum power, and the manufacturer's minimum run and off times, plus a simulator model that enforces them. Nothing in the codebase carries a compressor state (running time, time since stop) or a COP; the heater's latch (`thermostat_delta_c`) is a temperature deadband and not a time constraint.
**What's needed before this is buildable:** (1) the compressor timers as asset state, so the asset alone says whether it may start or stop now (asset-competence); (2) the minimum-interval constraint expressed through the declared asset interface so the planner core stays kind-free; (3) the arbiter treats a held compressor as a forced power for the interval, the way it treats a forced heater; (4) UC-14 (thermal feedback loop) gets a real scenario.
**Gain:** Medium to High — heat pumps are the main flexible thermal load of a modern household, and a naive resistive model would produce plans a real unit cannot follow.
**Complexity:** Large; the compressor timers and a COP model are the first two steps.

---
