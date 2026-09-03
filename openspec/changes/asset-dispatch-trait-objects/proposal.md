# Proposal: Asset Dispatch — Closed Enum to Trait Objects

## Why

`AssetConfig` (`VEN/src/assets/mod.rs`) is a closed 5-variant enum
(`Battery, Ev, Heater, Pv, BaseLoad`) that carries every asset type's physics
(`step`, `capability`, `flexibility_floor`, and the rest of the `Asset` trait,
`VEN/src/assets/asset_trait.rs`). Dispatch is done through two macros,
`delegate_asset!` and `delegate_asset_state!`, which expand to an exhaustive
`match` over the enum for every method. Adding a new physics type today means
editing this enum plus every macro-generated match arm.

The MILP layer already solved the identical problem for `AssetMilpContext`
(`VEN/src/controller/asset_milp_port.rs`) as part of R-23, moving from concrete
type imports to `Vec<Box<dyn AssetMilpContext>>` specifically to stop
`milp_planner` importing concrete asset types. `AssetConfig` never received the
equivalent treatment, so the same conceptual "asset" today has two inconsistent
dispatch mechanisms depending on which layer you're in.

This matters now because `docs/plans/asset-max-power-forecast-master-plan.md`
plans four further specs on top of this one — most immediately Spec B, which adds
shiftable loads as a first-class asset type. Doing that against the closed-enum
dispatch mechanism would mean paying the migration cost once to add it to the
enum, then again when the enum is eventually converted. Converting first means
every later spec is written against the target architecture from the start.

A closer look at `impl AssetConfig` (`VEN/src/assets/mod.rs:214-404`) also
surfaced that "`Config`" is the wrong name for what this type does, and that the
mismatch points at a real design gap, not just a cosmetic one: beyond the three
`Asset`-trait methods, `AssetConfig` dispatches fifteen more inherent methods
(`default_setpoint`, `plan_trajectory`, `state_values`, `control_schema`, `reset`,
`update_config`, `forecast`, `resolve_request_target`, `default_comfort_rates`,
`default_completion_policy`, `default_post_deadline_comfort_bid`,
`available_storage_kwh`, `thermostat_setpoint_kw`, `surplus_charge_kw`, and
`build_milp_context`) — almost none of it static configuration, almost all of it
runtime behavior. Auditing each by its actual dispatch mechanism (`design.md`
Decision D4): 9 of the 15 are dispatched uniformly for all 5 variants today
(genuinely universal `Asset`-shaped behavior, however trivial some
implementations may be), and 6 (`plan_trajectory`, `thermostat_setpoint_kw`,
`resolve_request_target`, `available_storage_kwh`, `surplus_charge_kw`,
`build_milp_context`) go through a hand-written match with a real `_ => None`
fallback for asset kinds that structurally can't support the behavior — a
genuine sign that these six are capability-specific, not universal `Asset`
behavior. This change now addresses both problems together: converting
`AssetConfig` to trait-object dispatch is done by splitting its real surface
across the core `Asset` trait (universal behaviors) and new optional capability
traits (partial behaviors), rather than either forcing every asset kind to
implement stubs for behaviors it doesn't have, or leaving a shrunken enum behind
to hold the leftovers. See `design.md` Decision D4.

## What Changes

- `AssetConfig` becomes trait-object-dispatched (`Box<dyn Asset>` or equivalent),
  extending the existing `AssetHandle`/`Asset` trait pattern
  (`VEN/src/assets/asset_trait.rs`) that already proves this shape works, rather
  than introducing a parallel concept.
- `AssetConfig`'s real behavioral surface — not just the three current `Asset`
  methods — is redistributed by capability rather than carried over as-is:
  - Methods every asset kind genuinely implements today (`default_setpoint`,
    `control_schema`, `update_config`, `default_comfort_rates`,
    `default_completion_policy`, `default_post_deadline_comfort_bid`,
    `state_values`, `reset`, `forecast` — 9 methods, verified via each one's
    actual dispatch mechanism, not by which concept it reads as; see `design.md`
    D4's correction note for why the three "default_*" comfort/completion
    methods belong here despite sounding MILP-specific) move onto the `Asset`
    trait itself.
  - Methods only some asset kinds implement move onto new optional capability
    traits, discovered via an `Option<&dyn Trait>`-returning accessor on `Asset`
    (defaulting to `None`, the same pattern `flexibility_floor`'s doc comment
    already establishes — "every asset type must state its own answer
    explicitly"): `MilpParticipant` (`build_milp_context` only — Battery/EV/
    Heater), `RequestResolvable` (`resolve_request_target`, `surplus_charge_kw`,
    `available_storage_kwh` — Battery/EV), `Thermostat`
    (`thermostat_setpoint_kw`, `plan_trajectory` — Heater only today).
  - This is the resolution to the naming question that prompted this change: once
    `AssetConfig`'s methods are redistributed this way, there is no longer an
    umbrella type left that needs a "less static-sounding" name — call sites hold
    `Box<dyn Asset>` (plus an optional capability-trait reference where needed),
    and the naming problem is resolved by elimination rather than a rename.
- The `delegate_asset!` macro (`VEN/src/assets/mod.rs`) is removed once every call
  site it currently expands for is migrated to trait-object dispatch.
- All 5 existing physics types (Battery, EV, Heater, PV, BaseLoad) are migrated
  with **zero behavior change** — this is a dispatch-mechanism refactor, not a
  physics change. The `Asset`/capability-trait surface grows to cover what
  `AssetConfig` already did; no asset kind's actual behavior differs before and
  after.
- **Scope note (narrowed from the master plan's original wording):** only
  `AssetConfig` is converted. `AssetState` (the sibling enum, currently
  `#[serde(tag = "asset_type", rename_all = "snake_case")]`-derived for the wire
  format) stays a plain closed enum. `AssetState`'s own doc comment already
  states it is "State-only... Variants hold only mutable runtime state — no
  config fields" — it carries no `step`/`capability`/`flexibility_floor` behavior,
  so there is no dynamic-dispatch need for it, and converting it would require
  solving trait-object `Deserialize` (e.g. the `typetag` crate) for no behavioral
  benefit. `delegate_asset_state!` (the macro dispatching over `AssetState` for
  the handful of state-only methods like `state_values`/`reset`/`forecast`) is
  therefore **out of scope** for this change and stays as-is. See `design.md`
  Decision D1.

## Non-Goals

- **Not** a runtime/third-party asset-plugin system. The asset catalog stays
  compile-time-fixed — new asset kind = new file + trait impl, no central enum to
  edit — matching `AssetMilpContext`'s own scope, not a plugin ABI.
- **Not** adding shiftable load (or any new asset type) here. That is Spec B in
  the master plan, deliberately sequenced after this change so it lands as the
  first trait-object-only asset, proving the new pattern on real new work rather
  than only on migrated existing types.
- **Not** a change to the *existing* `Asset` trait methods' behavior (`step`,
  `capability`, `flexibility_floor`, `simulate_forward`) — their signatures and
  semantics are untouched. (Correction from this change's original draft: the
  `Asset` trait's *surface* does grow, and new optional capability traits are
  introduced, to cover the fifteen inherent methods `AssetConfig` carries beyond
  the three it mirrors from `Asset` — see the "What Changes" capability-split
  note above and `design.md` Decision D4. What doesn't change is any asset
  kind's actual runtime behavior.)
- **Not** a change to `AssetState`, its serde wire format, or `delegate_asset_state!`
  — see the scope note above.

## Capabilities

No capability is added, modified, or removed. This is an internal dispatch
mechanism change with no user-observable behavior difference — the same category
of non-capability work as `openspec/changes/capacity-envelope-unification/`
(analysis-only, no `specs/` delta), except this change is real, sequenced
implementation work and so gets a `tasks.md`. No `specs/` directory is included
in this change for the same reason: there is no requirement to add, modify, or
remove.

## Impact

- **`VEN/src/assets/mod.rs`** — `AssetConfig` enum removed in favor of
  trait-object storage; `delegate_asset!` macro removed; its 15
  non-trait-mirrored inherent methods redistributed onto `Asset` (9 universal
  methods) and three new optional capability traits (`MilpParticipant` —
  `build_milp_context` only; `RequestResolvable` — 3 methods; `Thermostat` — 2
  methods; 6 partial methods total). `AssetState` and `delegate_asset_state!`
  are unaffected (see scope note).
- **`VEN/src/assets/asset_trait.rs`** — `Asset` trait grows by 9 methods (see
  above); three new capability traits are defined here or in a new sibling file
  (e.g. `VEN/src/assets/capabilities.rs` — file-size budget permitting, per this
  repo's 500-production-line limit on `VEN/src/` files). `AssetHandle` becomes
  the primary (or only) way `Asset` implementors are referenced, rather than a
  side wrapper used mainly in tests.
- **`VEN/src/controller/asset_milp_port.rs`** — no structural change required,
  but worth cross-checking during implementation whether `MilpParticipant`
  (now just `build_milp_context`, after `design.md` D4's correction moved the
  three comfort/completion-policy methods to `Asset` instead) should simply
  *be* the trait that produces an `AssetMilpContext`, rather than a separate
  one-method concept that happens to cover the same three kinds
  (Battery/EV/Heater) as `AssetMilpContext`'s own `AssetKind` — flag as an
  implementation-time judgment call, not decided here.
- **`VEN/src/simulator/mod.rs`** — the primary storage site, not just another
  call site: `SimState.asset_configs: Vec<AssetConfig>` (a `Vec` held parallel
  by index to `assets: Vec<AssetEntry>`), plus `SimState::find_asset`/
  `find_asset_mut`/`iter_assets`, which hand back `(&AssetEntry, &AssetConfig)`/
  `(&mut AssetEntry, &mut AssetConfig)` pairs. Phase 1's "introduce trait-object
  storage" (`design.md`'s Migration Plan) is concretely this field's type
  changing to `Vec<Box<dyn Asset>>` and those three methods' return types
  changing to match. (An earlier draft of this Impact section listed this file
  only as one of many probable "test/construction sites" among 12 others — it
  is in fact the single most structurally important site in the migration; see
  `design.md` D3's correction note.)
- **Call sites matching `AssetConfig::<Variant>`** (surveyed via
  `grep -rln "AssetConfig::" VEN/src --include="*.rs"`, excluding `assets/mod.rs`
  itself, 41 matches across these files):
  - `VEN/src/assets/asset_trait.rs`
  - `VEN/src/controller/capacity_forecast.rs`
  - `VEN/src/controller/milp_planner/tests/mod.rs`
  - `VEN/src/routes/debug.rs`
  - `VEN/src/simulator/base_load_preview.rs`
  - `VEN/src/simulator/forecast.rs`
  - `VEN/src/simulator/persist.rs`
  - `VEN/src/simulator/plan_context.rs`
  - `VEN/src/simulator/pv_preview.rs`
  - `VEN/src/simulator/snapshot.rs`
  - `VEN/src/simulator/tests/peek_pv_kw_tests.rs`
  - `VEN/src/simulator/tests.rs`

  Most of these are expected to be test/construction sites (building an
  `AssetConfig::Battery(...)` fixture) rather than behavior-dispatching matches,
  since `delegate_asset!` is meant to be the sole dispatch point — `tasks.md`'s
  survey task confirms this per-file before migration.
- **A missed call site, found by broadening the survey to bare-type usages**
  (`grep -rlnE "\bAssetConfig\b" VEN/src --include="*.rs"`, per `design.md` D3's
  correction): `VEN/src/routes/hems/sessions.rs` — a real
  `use crate::assets::{AssetConfig as AC, ...}` plus
  `.zip(sim.asset_configs.iter())`, missed by the narrower `AssetConfig::`
  pattern because it references the bare type, not a variant constructor.
  Three more files (`VEN/src/assets/grid.rs`,
  `VEN/src/controller/simulator_port.rs`, `VEN/src/profile/schema.rs`) also
  turned up under the broader search but are doc-comment mentions of the type
  name only, with no real code dependency — a comment update during migration,
  not a functional task.
- **Tests** that construct `AssetConfig` variants directly (the majority of the
  above list) need their construction syntax updated to whatever the trait-object
  registry's construction API becomes; no test *assertions* should need to change,
  since behavior is unchanged.
