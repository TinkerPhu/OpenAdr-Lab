# Design: Asset Dispatch — Closed Enum to Trait Objects

## Context

`VEN/src/assets/mod.rs` defines `AssetConfig` as a closed enum:

```rust
pub enum AssetConfig {
    Battery(Battery),
    Ev(EvCharger),
    Heater(Heater),
    Pv(PvInverter),
    BaseLoad(BaseLoad),
}
```

Every method the `Asset` trait requires (`step`, `capability`,
`flexibility_floor`, plus several `AssetConfig`-only methods like
`default_setpoint`, `control_schema`, `update_config`) is dispatched through the
`delegate_asset!` macro, which expands to one `match self { AssetConfig::X(cfg)
=> cfg.method(...), ... }` per method. Every new physics type requires editing
this enum and confirming every macro expansion still makes sense for it.

The MILP layer (`VEN/src/controller/asset_milp_port.rs`) faced the identical
problem for `AssetMilpContext` and was already converted (R-23) to
`Vec<Box<dyn AssetMilpContext>>` — genuinely open trait-object dispatch, no
central enum, no macro. The `Asset` trait already has a working proof of this
pattern in the same layer: `AssetHandle<'a>` (`VEN/src/assets/asset_trait.rs`)
implements `Asset` by borrowing individual fields and delegating to
`self.config: &AssetConfig`. `AssetHandle` shows the target shape already
compiles and passes tests — this change removes the closed-enum layer
underneath it, it doesn't invent a new pattern.

`AssetConfig`'s name was also raised as questionable during this change's
drafting ("Config" undersells a type that dispatches physics, MILP-context
construction, comfort defaults, and request resolution — see D4 below). Rather
than rename the type, the resolution adopted here is to eliminate the umbrella
type's need to exist at all: its real behavioral surface is redistributed by
capability across `Asset` and three new optional traits, so call sites end up
holding `Box<dyn Asset>` directly with no second name to argue about.

## Goals / Non-Goals

**Goals:**
- Zero behavior change — every existing test (UI unit, Rust unit/integration,
  E2E BDD, resilience) passes unchanged before and after.
- `AssetConfig` dispatch becomes symmetric with `AssetMilpContext`'s: new asset
  kind = new file + trait impl(s), no central enum edit.
- `AssetConfig`'s real behavioral surface (15 non-trait-mirrored methods,
  beyond the 3 `Asset`-trait ones it already forwards) is redistributed
  honestly, classified by actual dispatch mechanism (D4): 9 universal methods
  onto `Asset`, 6 partial/capability-specific methods onto new optional traits
  — not carried over as a second enum-shaped thing under a different name.
- Prepare cleanly for Spec B (shiftable load as the first trait-object-only
  asset) and Spec C/D, per `docs/plans/asset-max-power-forecast-master-plan.md`.
  A shiftable load should be able to decline `MilpParticipant`/
  `RequestResolvable`/`Thermostat` outright (it needs its own MILP treatment per
  Spec B, and has no thermostat or request-resolution behavior) without
  implementing stub methods for any of them.

**Non-Goals:**
- Runtime/third-party plugin registration (see proposal.md Non-Goals).
- Converting `AssetState` or `delegate_asset_state!` (see D1 below).
- Any change to the *existing* `Asset` trait methods' (`step`/`capability`/
  `flexibility_floor`/`simulate_forward`) signatures or semantics. The trait's
  surface does grow (D4) — that growth is in scope, changing what's already there
  is not.
- Adding shiftable load or any new asset type in this change.
- Designing the *content* of `MilpParticipant`/`RequestResolvable`/`Thermostat`
  beyond their method lists (D4) — e.g. whether `MilpParticipant` subsumes or
  wraps `AssetMilpContext` is an implementation-time call, not decided here (see
  proposal.md's Impact section).

## Decisions

### D1: Convert `AssetConfig` only; leave `AssetState` as a plain enum

**Decision:** Only `AssetConfig` is converted to trait-object dispatch.
`AssetState` remains the closed enum it is today, with `delegate_asset_state!`
unchanged.

**Rationale:** `AssetState`'s own doc comment states it is "State-only... Variants
hold only mutable runtime state — no config fields." It carries no `step`/
`capability`/`flexibility_floor` behavior — those all live on `AssetConfig`. There
is therefore no dynamic-dispatch need for `AssetState`: dynamic dispatch buys
you polymorphic *behavior*, and `AssetState` has none to offer. Converting it
anyway would mean solving trait-object `Deserialize` (its current
`#[serde(tag = "asset_type", rename_all = "snake_case")]` derive doesn't survive
a move to `Box<dyn Trait>` without extra machinery) for zero behavioral benefit.

**Alternative considered:** Convert both enums for full symmetry with the master
plan's original phrasing ("`AssetConfig`/`AssetState` ... become trait objects").
Rejected — the serde problem is real work with no payoff, and `AssetState`'s
wire format (consumed by persistence and possibly the UI) staying untouched
lowers this change's risk. This narrowing was confirmed with the user before
implementation (see the change's proposal.md scope note).

### D2: Extend `AssetHandle`, don't introduce a parallel registry type

**Decision:** `AssetHandle` (or a renamed/generalized version of it) becomes the
primary way an `Asset` implementor is referenced, rather than introducing a new
dedicated wrapper type alongside it.

**Rationale:** `AssetHandle` already implements `Asset` correctly today (see its
existing test module, `handle_tests`, in `asset_trait.rs`) by holding borrowed
references to `config`, `id`, `state`, `history`. It already demonstrates the
target shape compiles and behaves correctly — reusing it avoids a redundant
"why do we have two things that both wrap an asset" question later. Concretely,
this means `AssetHandle`'s `config: &'a AssetConfig` field becomes
`config: &'a dyn Asset` once the enum is retired (Phase 3) — `AssetHandle`
itself keeps its existing role (bundling a physics implementor with `id`/
`state`/`history` for callers that need all four), it just holds a trait object
reference instead of an enum reference. Concrete physics types (`Battery`,
`EvCharger`, etc.) implement `Asset`'s new 9 universal methods and any
capability traits directly, the same as they implement `step`/`capability`
today; `AssetHandle` continues to forward to whatever it wraps.

**Alternative considered:** A brand-new `Box<dyn Asset>` registry type separate
from `AssetHandle`, with `AssetHandle` kept only for its current
borrowed-reference test usage. Rejected as unnecessary duplication — no evidence
`AssetHandle`'s current shape (borrowed fields, not owned) is a blocker for the
places `AssetConfig` gets stored today; if it turns out to be, that constraint
should surface and be resolved as part of implementing this change, not
speculated about here.

### D3: Confirm no exhaustive match on `AssetConfig` exists outside `delegate_asset!`

**Decision:** Before writing the detailed per-file migration tasks, verify via
`grep -rlnE "\bAssetConfig\b" VEN/src --include="*.rs"` — **not** the narrower
`AssetConfig::` pattern this decision originally used, which only catches
enum-variant construction/associated-function syntax and misses bare-type
usages (struct fields, function signatures, generic parameters) — that every
non-`assets/mod.rs` hit is a *construction or storage* site (building an
`AssetConfig::Battery(...)` value, or declaring a field/return type of
`AssetConfig`) rather than a *dispatch* site (a `match` that would need
trait-object-aware rewriting beyond simple type/construction-syntax changes).

**Correction from an earlier draft:** the narrower `AssetConfig::` grep found
13 files (see proposal.md's original Impact list); the broader pattern finds 4
more. Of those 4, `VEN/src/routes/hems/sessions.rs` is a genuine missed call
site (`use crate::assets::{AssetConfig as AC, ...}` plus
`.zip(sim.asset_configs.iter())`); the other 3
(`VEN/src/assets/grid.rs`, `VEN/src/controller/simulator_port.rs`,
`VEN/src/profile/schema.rs`) are doc-comment mentions of the type name only, no
real code dependency — worth a comment update during migration but not a
functional task. Separately, `VEN/src/simulator/mod.rs` was already on the
narrower list but under-described there as "probably a test/construction
site" — it is in fact the **primary storage site**: `SimState.asset_configs:
Vec<AssetConfig>` (`VEN/src/simulator/mod.rs:70-90`), a `Vec` held parallel by
index to `assets: Vec<AssetEntry>`, plus `SimState`'s own `find_asset`/
`find_asset_mut`/`iter_assets` accessor methods, which hand back
`(&AssetEntry, &AssetConfig)`/`(&mut AssetEntry, &mut AssetConfig)` pairs. This
is arguably the single most structurally important site in the whole
migration — Phase 1's "introduce trait-object storage" is, concretely, this
field's type changing from `Vec<AssetConfig>` to `Vec<Box<dyn Asset>>` — and it
should be called out as its own task rather than folded anonymously into a
generic per-file list.

**Rationale:** `delegate_asset!` is meant to be the sole *dispatch* point, and
that invariant still held under the broader search — no new dispatch-site
matches turned up, only a storage declaration and one missed construction
site. If migrating call sites turns out to need more than a mechanical type/
construction-syntax change somewhere, that file needs its own migration task,
not just a find/replace.

### D4: Redistribute `AssetConfig`'s 15 non-trait-mirrored methods by *dispatch mechanism*, not by which concept they read as

**Decision:** `impl AssetConfig` (`VEN/src/assets/mod.rs:214-404`) has 18
methods total: 3 mirror the `Asset` trait (`step`, `capability`,
`flexibility_floor`) and 15 more don't. The classification below is drawn from
each method's **actual dispatch mechanism in the source**, not from which
concept it reads as (an earlier draft of this table got this wrong by
classifying by feel — see the correction note after the table):

- **Dispatched via `delegate_asset!`** (the uniform macro over a single enum —
  an exhaustive match, no `_ => ...` arm at all): universal, no exceptions.
- **Dispatched via `delegate_asset_state!`** (the uniform macro over
  `(AssetConfig, AssetState)` matched as a pair): also universal in practice,
  even though this macro *does* define a `_ => $default` arm — that arm exists
  only as a defensive fallback for an impossible config/state variant mismatch
  (per its own doc comment), not because any of the 5 asset kinds lacks the
  behavior. Every real, valid `(config, state)` pairing for all 5 kinds hits a
  real per-type implementation.
- **Dispatched via a hand-written `match`** with a genuine `_ => None`/no-op
  arm for variants that structurally can't support the behavior (not a
  mismatch safety net — a real "this asset kind doesn't do this"): partial.

| Method | Dispatch | Implemented for | Verdict |
|---|---|---|---|
| `default_setpoint` | `delegate_asset!` | all 5 | universal → `Asset` |
| `control_schema` | `delegate_asset!` | all 5 | universal → `Asset` |
| `update_config` | `delegate_asset!` | all 5 | universal → `Asset` |
| `default_comfort_rates` | `delegate_asset!` | all 5 | universal → `Asset` |
| `default_completion_policy` | `delegate_asset!` | all 5 | universal → `Asset` |
| `default_post_deadline_comfort_bid` | `delegate_asset!` | all 5 | universal → `Asset` |
| `state_values` | `delegate_asset_state!` | all 5 | universal → `Asset` |
| `reset` | `delegate_asset_state!` | all 5 | universal → `Asset` |
| `forecast` | `delegate_asset_state!` | all 5 | universal → `Asset` |
| `plan_trajectory` | hand-written match | Heater only | partial → `Thermostat` |
| `thermostat_setpoint_kw` | hand-written match | Heater only | partial → `Thermostat` |
| `resolve_request_target` | hand-written match | Battery, EV | partial → `RequestResolvable` |
| `available_storage_kwh` | hand-written match | Battery, EV | partial → `RequestResolvable` |
| `surplus_charge_kw` | hand-written match | EV only | partial → `RequestResolvable` |
| `build_milp_context` | hand-written match | Battery, EV, Heater | partial → `MilpParticipant` |

**Correction from an earlier draft:** `default_comfort_rates`,
`default_completion_policy`, and `default_post_deadline_comfort_bid` were
initially classified as partial/`MilpParticipant` (Battery/EV/Heater) because
they *read* as MILP-flavored concepts. Rechecking the source: all three are
dispatched via the uniform `delegate_asset!` macro with no `_ => None` arm — PV
and BaseLoad already have real (if presumably trivial/empty) implementations of
them today. Per this decision's own stated method — classify by actual current
dispatch mechanism, not by which concept a name evokes — they belong on
`Asset`, not `MilpParticipant`. If PV/BaseLoad's existing implementations turn
out to be meaningless placeholder values, that's a pre-existing question about
those methods' design, out of scope for this change to fix as a side effect of
the split.

The 9 universal methods move onto `Asset` directly (every implementor already
has real behavior for them — no stubs introduced). The 6 partial methods split
into three optional capability traits, each scoped to the asset kinds that
already implement it non-trivially:

- **`MilpParticipant`** (Battery, EV, Heater — note this is exactly
  `AssetMilpContext`'s existing `AssetKind` scope): `build_milp_context` only,
  after the correction above. A single-method trait is a smaller "capability"
  than originally scoped, but still worth keeping as a named marker — it
  signals "this asset kind participates in MILP planning" as a concept
  (mirroring `AssetMilpContext`'s own three-kind scope), and gives a natural
  home for any future MILP-specific `Asset` behavior rather than needing a
  fourth trait invented later. See proposal.md's Impact section for the
  related question of whether `MilpParticipant` should simply *be* (or wrap)
  `AssetMilpContext` rather than a separate concept — left as an
  implementation-time call.
- **`RequestResolvable`** (Battery, EV — the two storage-shaped assets a user
  can issue a direct request against): `resolve_request_target`,
  `available_storage_kwh`, `surplus_charge_kw` (the last is EV-only in
  practice — Battery's implementation of this trait method returns `None`,
  same as it does today; normal per-type variation within a shared trait, not
  a modeling error).
- **`Thermostat`** (Heater only, today): `plan_trajectory`,
  `thermostat_setpoint_kw`.

`Asset` gains one accessor per capability trait, defaulting to `None`:

```rust
fn as_milp_participant(&self) -> Option<&dyn MilpParticipant> { None }
fn as_request_resolvable(&self) -> Option<&dyn RequestResolvable> { None }
fn as_thermostat(&self) -> Option<&dyn Thermostat> { None }
```

PV and BaseLoad implement none of the three overrides — they simply inherit the
`None` defaults, with no stub method bodies to write or maintain. This mirrors
`flexibility_floor`'s own existing doc comment ("No default: every asset type
must state its own answer explicitly rather than silently inherit a wrong one")
in spirit but inverted correctly: `flexibility_floor` has no safe default because
every asset *has* a floor and getting it wrong is a physics bug, whereas these
three capabilities have a safe, meaningful default (`None`, "this asset doesn't
do that") because most assets genuinely don't have the capability at all.

**Rationale:** the alternative — putting all 15 methods on `Asset` with default
no-op/`None` bodies — would mean every future asset (starting with Spec B's
shiftable load) inherits six methods it has no relationship to, and a reader of
`impl Asset for ShiftableLoad` can't tell from the trait alone which of those
inherited defaults are "correctly doesn't apply" versus "someone forgot to
override this." Splitting by capability makes that distinction structural: a
type simply doesn't implement `Thermostat` if it has no thermostat, rather than
implementing `Asset::thermostat_setpoint_kw` and returning `None`.

**Alternative considered:** Keep one `Asset` trait with default no-op/`None`
implementations for all 15 methods, no capability traits. Rejected per the
rationale above — this is the "fat trait" version of the same closed-enum
problem this whole change exists to fix, just moved from an enum to a trait.

**Alternative considered:** A single umbrella `AssetCapabilities` trait with all
6 partial methods as `Option`-returning stubs, rather than three separate
traits. Rejected — it re-merges concerns (MILP participation, user-request
resolution, thermostat behavior) that have no reason to travel together; a
future asset kind might plausibly want `RequestResolvable` without
`MilpParticipant` (or vice versa), and three focused traits keep that possible.

### D5: `SimState::tick()`'s hand-written override-injection match is a real, confirmed dispatch site

**Decision:** D3 flagged the *possibility* that some file has its own exhaustive
match on `AssetConfig` outside `delegate_asset!`, pending verification. Verified
during this change's drafting: `SimState::tick()`
(`VEN/src/simulator/mod.rs:231-289`) has exactly that —
`match cfg { AssetConfig::Pv(pv) => ..., AssetConfig::Heater(h) => ...,
AssetConfig::BaseLoad(bl) => ..., AssetConfig::Ev(ev) => ..., _ => {} }` —
injecting tick-time environment overrides (irradiance, ambient temp, load
overrides, EV plugged-state) directly into each concrete type's fields before
calling `cfg.step(...)`. Battery has no arm (falls to `_ => {}`) — this is a
fourth genuinely-partial-but-real per-type behavior, structurally identical in
shape to D4's capability split, but it isn't a method on `AssetConfig` at all
(D4's audit only covered `impl AssetConfig`'s own methods) — it's a hand-rolled
match living in `tick()`'s own body, operating on fields directly.

**Resolution:** treat this the same way D4 treats partial behaviors — add a
fourth optional capability trait, e.g. `TickOverridable` (`apply_tick_overrides
(&mut self, overrides: &TickOverrides)`, a struct bundling the ~15 override
parameters `tick()` currently takes as bare function arguments), implemented by
Pv/Heater/BaseLoad/Ev, declined by Battery/(future ShiftableLoad) via the same
`as_*() -> Option<&mut dyn TickOverridable>` accessor pattern. `tick()`'s match
becomes a loop calling the accessor, not a hand-written enum match — closing
the exact gap D3 was written to catch.

**Rationale:** this is not new scope creep — it's the concrete instance D3
explicitly reserved a task for ("that file needs its own migration task, not
just a syntax find/replace"). Folding it into the existing capability-trait
mechanism (rather than leaving `tick()`'s match as a bespoke survivor)
keeps exactly one dispatch pattern in this codebase for "some asset kinds do
this, some don't," instead of two.

## Risks / Trade-offs

- **R1 — Loss of exhaustive-match compiler enforcement.** Once dispatch is
  trait-object-based, the compiler no longer forces every method to handle a new
  variant when one is added.
  → Mitigation: this is the explicit goal, not an accidental side effect — Spec B
  should be able to add shiftable load as a new `Asset` implementor without the
  compiler needing to walk it through every existing match arm. Any method that
  genuinely needs to enumerate all asset kinds (rare — `AssetKind`-style
  discriminants for logging, e.g.) should say so explicitly via its own smaller
  enum, not by relying on `AssetConfig` being closed.

- **R2 — Dynamic dispatch cost in the simulation loop.** `Box<dyn Asset>` calls
  are a vtable indirection versus a monomorphized match arm.
  → Mitigation: the simulation loop's per-asset `step()` call is not the
  dominant cost relative to the MILP solve itself (`good_lp`/HiGHS). Do not
  pre-optimize; profile only if a real regression is observed after this change
  lands.

- **R3 — Capability-trait boundaries drawn wrong.** D4's grouping
  (`MilpParticipant`/`RequestResolvable`/`Thermostat`) is based on which of the
  5 *current* asset kinds implement each method today; a future asset kind
  (Spec B's shiftable load, or something later) might need a combination that
  doesn't fit neatly — e.g. a capability that's genuinely universal-but-optional
  rather than cleanly partitioned.
  → Mitigation: this is a call-site-driven design, not a speculative one — it's
  based on the actual 5-variant audit in D4, not a guess at future needs. If
  Spec B reveals a grouping problem, that's real information the current split
  didn't have; fix the trait boundaries then rather than over-designing for
  hypothetical future asset kinds now.

## Migration Plan

### Phase 0 — Define `Asset`'s 9 new methods and the 3 capability traits
Add the 9 universal methods (`default_setpoint`, `control_schema`,
`update_config`, `default_comfort_rates`, `default_completion_policy`,
`default_post_deadline_comfort_bid`, `state_values`, `reset`, `forecast`) to the
`Asset` trait, and define `MilpParticipant` (`build_milp_context` only),
`RequestResolvable`, `Thermostat` per D4, plus the three `as_*` accessor
methods on `Asset` (default `None`). No asset type implements any of this yet —
this phase only adds trait/type definitions, which compile independently of
`AssetConfig`.

### Phase 1 — Introduce trait-object storage alongside the existing enum
Add the trait-object-based storage/construction path without removing
`AssetConfig` yet. Both compile and pass tests simultaneously — this phase adds
code, it doesn't delete any.

### Phase 2a — Implement each type's `Asset`/capability-trait surface, one at a time
For each of Battery, EV, Heater, PV, BaseLoad (in that order — Battery first as
the simplest physics, PV/BaseLoad last as the ones most entangled with forecast
frame code per `VEN/src/simulator/forecast.rs`/`pv_preview.rs`): implement the 9
new `Asset` methods (moving the existing per-type logic verbatim, not
rewriting it) plus whichever of `MilpParticipant`/`RequestResolvable`/
`Thermostat` D4's table says it needs. This is genuinely incremental — each
type's trait impl compiles and can be unit-tested for equivalence against the
old enum-dispatched behavior independently of the others, since `AssetConfig`
is still what's actually stored and dispatched in production code paths during
this phase. Never leave two types' trait impls half-written — always keep a
single, clearly-stated frontier so partial progress is legible to whoever picks
this up next.

### Phase 2b — Cut over `SimState`'s storage in one atomic step
**Not incremental, unlike 2a.** `SimState.asset_configs: Vec<AssetConfig>`
(`VEN/src/simulator/mod.rs`) is one homogeneous collection holding all 5 asset
types together — its declared element type cannot change from `AssetConfig` to
`Box<dyn Asset>` gradually per asset type; it changes in one commit, once every
type from Phase 2a has its full trait surface ready. That single change forces
every consumer of `SimState.asset_configs`/`find_asset`/`find_asset_mut`/
`iter_assets` (D3's corrected survey — the 13 `AssetConfig::` sites plus the
newly-found `VEN/src/routes/hems/sessions.rs`) to be updated in the same
commit, since they all compile against `SimState`'s one field type. Run the
full test suite immediately after this single commit, not partway through it.

### Phase 3 — Delete the closed enum and `delegate_asset!`
Once no call site references `AssetConfig::<Variant>` directly (verified by
re-running D3's grep and getting zero non-`assets/mod.rs` hits) and every one of
`AssetConfig`'s 15 non-trait-mirrored methods has a home on `Asset` or a
capability trait (9 + 6, per D4's corrected table), delete the enum and the
`delegate_asset!` macro.

### Rollback
Safe to revert at any phase boundary: behavior is unchanged throughout, and per
D1, `AssetState`'s persisted/serialized wire format is never touched, so there is
no data migration to reverse.

## Open Questions

None outstanding. D1's scope narrowing (convert `AssetConfig` only, leave
`AssetState` untouched) and D4's capability-trait split (redistribute
`AssetConfig`'s 15 non-trait-mirrored methods across `Asset` plus
`MilpParticipant`/`RequestResolvable`/`Thermostat`, rather than a straight
rename) were both raised and confirmed with the user during this change's
drafting.
