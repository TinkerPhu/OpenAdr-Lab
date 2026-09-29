# Design

## Context

See `proposal.md` — Why. What matters here is the shape of the code being replaced.

`EvMilpContext` (`controller/milp_planner/asset_port.rs`) carries the EV's whole valuation as
four scalars plus one binary:

| field | today |
|---|---|
| `e_core_kwh` | energy from `soc_init` to `soc_target` |
| `v_core_eur` | **lump** reward for the core, `core_kwh × bid@fill 0.0` |
| `e_extra_max_kwh` | `battery_kwh × (1 − soc_target)` |
| `v_extra_eur_kwh` | per-kWh reward beyond target, `bid@fill 1.0` |
| `z_ev_core` | binary; `MayRun` only |

and two constraint arms (`ev_milp.rs::constraints`):

```rust
MustRun => ev_energy == e_core_kwh + e_ev_extra
MayRun  => ev_energy == e_core_kwh * z_ev_core + e_ev_extra
           e_ev_extra <= e_extra_max_kwh * z_ev_core
```

`ev_energy` is `energy_expr`, already bounded to the deadline step (`t_dead_step`). Only two
points of the curve are ever read — `value_at_fill(rates, 0.0)` and `(rates, 1.0)` in
`assets/ev_comfort.rs::resolve_ev_comfort_reward` — so the shape a user draws between them
cannot influence anything. CO2 bids are already monetized into €/kWh there via `w_ghg_eur_kg`,
so the objective treats them exactly like price.

Constraints on the approach: `entities/`, `controller/` may not import `assets/`; the EV hands
the controller a `Box<dyn AssetMilpContext>`; `VEN/src/` files stay under 500 production lines;
determinism (no wall-clock or RNG in planning inputs).

## Goals / Non-Goals

**Goals:**
- Value EV energy per kWh from a single curve, so partial delivery is expressible.
- Keep the EV's MILP contribution **continuous** — remove a binary rather than trade it for
  several.
- Preserve firm-deadline guarantees exactly, and preserve the free/opportunistic modes'
  behaviour.

**Non-Goals:**
- Per-slot SoC variables for the EV (R-93) — the segments here are energy totals over the
  deadline window, not a SoC trajectory. This change neither fixes nor worsens R-93.
- Multiple deadlines per solve (R-92).
- Changing the controllable-import malus (`c_ctrl_imp_malus`, 0.22 €/kWh). It stays exactly as
  it is; this change only stops it from vetoing a whole request at once.
- Applying the same treatment to the heater or shiftable loads — their all-or-nothing is
  physical and stays.

## Decisions

**1. Segments, not a binary.** Replace `e_core_kwh`/`v_core_eur`/`e_extra_max_kwh`/
`v_extra_eur_kwh`/`z_ev_core` with `Vec<EvEnergySegment { kwh, eur_per_kwh }>` spanning
`soc_init → 1.0`. Each segment declares one continuous variable bounded by its own kWh; their
sum is the delivered energy; each is rewarded at its own bid.

```
ev_energy == Σ e_seg[k]                      (all modes; replaces both arms)
0 <= e_seg[k] <= seg_kwh[k]
objective += −Σ (w_services × bid[k]) × e_seg[k]
```

*Why this over alternatives:* a lump reward with a continuous 0..1 completion fraction would
also remove the binary, but it prices the first kWh the same as the last, which is exactly the
information a comfort curve exists to carry. SOS2/piecewise with binaries would preserve
arbitrary curve shapes but reintroduces binaries into a solve that already brushes its timeout
(R-97).

**2. Concavity comes from validation, not from extra variables.** Because segment bids are
non-increasing (enforced in `services/comfort.rs::validate_curve`), the solver fills the
high-bid segments first on its own — no ordering constraints, no binaries, and the LP relaxation
is exact. A rising curve is refused at the API boundary rather than modelled.

*Why reject rather than clamp:* clamping stores something different from what the user typed,
which is a silent surprise; and a rising willingness-to-pay has no economic reading here. No
shipped curve violates the rule (`VEN/profiles/test.yaml`: 0.50 → 0.05).

**3. `fill` means state of charge for an EV.** Segment k covering SoC `[a, b]` is priced at the
curve's value over that interval. The curve is a property of the battery's fullness, which is
what makes "one curve, now → 100 %" expressible at all.

*Consequence to state plainly in docs:* an existing curve authored as "fraction of this task"
is reinterpreted as "fraction of the battery". For the shipped two-point curves this changes
nothing meaningful; for a hand-drawn multi-point curve it does.

**4. Firm deadlines become an explicit floor.** `MustRun` adds
`Σ e_seg[k] >= (soc_target − soc_init) × battery_kwh`, capped at what the available slots can
physically deliver. The reward terms still apply on top, so a firm request also takes worthwhile
energy beyond its target.

*Refined during implementation:* the cap is applied **where the floor is imposed**
(`EvMilpContext::reachable_energy_kwh`, called from `constraints`), and
`clamp_core_to_reachable_energy` plus its `core_unmet_warning` field on `EvScalars`,
`EvMilpContext` and `MilpInputs` are deleted. Shrinking `e_required_kwh` upstream is what made
the shortfall invisible: once the requirement had been lowered to the reachable amount, the plan
met it exactly and no diagnostic could tell that anything was missing (the warning string it
carried was, by then, read by nobody). Keeping `e_required_kwh` at what the user asked for gives
one number for the guarantee, one derived cap for feasibility, and one comparison for the
warning.

**5. Free/opportunistic modes are untouched — corrected during implementation.** The design
first said these modes would take the curve's lowest bid as their flat rate. Reading
`from_state` showed they never used the curve at all: `Opportunistic`/`AsapFree`/
`ByDeadlineFree` reward each kWh at the profile constant `v_ev_free_charge_eur_kwh` and
`MaxCost` at `BUDGET_CHARGE_REWARD_EUR_KWH`, with `ev_milp.rs`'s own comment stating that "the
curve doesn't apply there". Connecting them to it would be a behaviour change this change did
not set out to make, so they keep their flat per-slot rewards and their `e_extra_max_kwh` cap
exactly as they are. Only the `ByDeadline`/`Asap` arm — the one that reads the curve today —
switches to segments.

**6. One requirement, one diagnostic.** `EV_CORE_ENERGY_UNMET` keeps its wire name (it is
persisted in `plan_history.warning_kinds`) but fires **only** for a firm-deadline shortfall,
carrying delivered and required kWh. The soft path stops producing it: "we charged less because
your bid stopped being worth it" is not an unmet obligation. This also ends the double meaning
introduced when `clamp_core_to_reachable_energy` reused the kind.

### Reuse inventory

Existing functions this change reuses or consolidates rather than reinventing
(`one-concept-one-function`):

| concept | lives at | disposition |
|---|---|---|
| curve interpolation | `entities/asset.rs::ComfortRate::interpolate_at_fill`, `value_at_fill`, `co2_value_at_fill` | reused unchanged to price each segment boundary |
| curve → reward scalars | `assets/ev_comfort.rs::resolve_ev_comfort_reward` | **becomes** the segment builder; the four-scalar struct goes |
| CO2 bid monetization | same function (`/1000 × w_ghg_eur_kg`) | reused; folded into each segment's effective bid |
| curve validation | `services/comfort.rs::validate_curve` | extended with the non-increasing rule |
| deadline-bounded energy | `assets/ev_milp.rs::energy_expr` | reused unchanged |
| reachability clamp | `assets/ev_usage_forecast.rs::clamp_core_to_reachable_energy` | **deleted**; its logic becomes `EvMilpContext::reachable_energy_kwh`, applied at the floor (Decision 4) |
| availability mask, free-energy cap | `a_ev`, `inject_grid_slots` | unchanged |
| binary pinning between phases | `solver_phase2.rs:113`, `solver_duals.rs:128` | one entry each removed |

## Risks / Trade-offs

- **[Segment count inflates the model]** → One variable per curve point (typically 2–5), not per
  slot. Net change is negative: one binary removed, a handful of continuous variables added.
  Task 6 measures `solver_ms` before/after on the same profile rather than assuming.
- **[Reinterpreting `fill` silently changes existing curves]** → Only the two shipped test
  profiles carry curves today and both are two-point; the fleet carries none. Stated as
  **BREAKING** in the proposal and written into the user manual, not left to discovery.
- **[A firm request could now charge beyond its target]** → It always could (the `extra` reward
  existed); the floor keeps the guarantee, and the reward above it is bounded by the curve the
  user drew. Pinned by a test that a firm request still reaches at least its target.
- **[`MayRun` disappears as a distinct constraint arm]** → The mode enum still distinguishes
  firm from soft (the floor), so `MilpLoadMode` stays. Existing mode tests
  (`tests/modes.rs`, `tests/basic.rs`) must keep passing with only their all-or-nothing
  expectations rewritten.
- **[GB-41's probes encode the old behaviour]** → Two of the nine assert all-or-nothing by
  design. They are rewritten to assert graceful degradation, which is the change stated as a
  test rather than deleted evidence.

## Migration Plan

No data migration: comfort curves are stored as-is and reinterpreted. Deploy is a normal VEN
rebuild; the fleet runs entirely on `MustRun` today, so no live request changes behaviour on
the day of deploy. Rollback is a revert — no persisted state gains a new shape.

The one externally visible change on deploy is the validation rule: a stored curve with rising
bids would now be refused on its next POST. None exists in the repo; a user-created one would
need editing, and the error names the point.
