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
