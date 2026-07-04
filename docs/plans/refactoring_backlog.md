# VEN Backend — Refactoring Backlog

> Code quality review conducted 2026-04-28. Updated 2026-05-25 to reflect resolved items.
> Scope: `VEN/src/` (Rust backend). UI and BFF not yet reviewed.
>
> Priority legend: 🔴 High / 🟠 Medium-High / 🟡 Medium / 🔵 Low (large, deferred)
>
> Authoritative status register: `docs/reference/TECHNICAL_DEBTS.md`

---

## Summary Map

```
VEN BACKEND — FRAGMENTATION MAP (as of 2026-05-25)
════════════════════════════════════════════════════════════════

  AssetConfig enum
  ┌──────────────────────────────────────────────────────┐
  │  ~9 methods × 5 variants = ~45 manual match arms     │
  │  Every new asset type: +9 match blocks               │
  │  Every new method: +5 match arms                     │
  └──────────────────────────────────────────────────────┘

  ────────────────────────────────────────────────────────────

  "battery","heater","ev","pv" string literals in dispatcher.rs
  ↑ each asset's asset_id() method is the authoritative source,
    but dispatcher.rs still duplicates them inline
```

---

## Open Items

| # | Issue | Priority | Effort | Risk |
|---|-------|----------|--------|------|
| R-03 | Replace hardcoded string asset IDs in `dispatcher.rs` with constants | 🟠 | Small | Mechanical |
| R-08 | `AssetConfig` → `dyn Asset` dispatch or macro forwarder | 🔵 | Large | Correctness risk, deferred |

---

## Detailed Findings

---

### R-03 — Hardcoded string asset IDs in `dispatcher.rs` 🟠

**File:** `VEN/src/controller/dispatcher.rs`

Asset ID string literals are duplicated in `dispatcher.rs`. The canonical source for each
ID is the `asset_id()` method in the respective asset file (e.g. `assets/battery.rs` returns
`"battery"`). `dispatcher.rs` re-declares these inline instead of referencing a shared constant:

```
"battery"  — dispatcher.rs:266, 269, 297, 338 (approx)
"ev"       — dispatcher.rs:297, 300 (approx)
"heater"   — dispatcher.rs:361, 364 (approx)
"pv"       — dispatcher.rs:319, 322 (approx)
"base_load"— dispatcher.rs:338, 341 (approx)
```

The `"boiler"` alias issue (previously in `routes/hems.rs`) is resolved — it now appears
only in a doc comment, not in runtime matching logic.

**Status:** Constants already created in `VEN/src/ids.rs` (`ASSET_EV`, `ASSET_BATTERY`, etc.).
**Remaining work:** Replace the inline string literals in `dispatcher.rs` with these constants and have each asset's `asset_id()` return the constant rather than a string literal.

---

### R-08 — `AssetConfig` dispatch explosion 🔵 *(deferred)*

**File:** `VEN/src/assets/mod.rs`

`AssetConfig` is a manual dispatch enum with ~9 methods, each a full `match` over 5 variants
(~45 match arms total). Every new asset type requires 9 new match arms; every new method
requires 5.

```rust
pub enum AssetConfig {
    Battery(Battery),
    Ev(EvCharger),
    Heater(Heater),
    Pv(PvInverter),
    BaseLoad(BaseLoad),
}

// × 9 methods, each:
pub fn step(&self, state: &AssetState, setpoint_kw: f64, dt: Duration) -> (AssetState, f64) {
    match self {
        Self::Battery(cfg) => cfg.step(state, setpoint_kw, dt),
        Self::Ev(cfg)      => cfg.step(state, setpoint_kw, dt),
        ...
    }
}
```

The `Asset` trait exists and is implemented by each physics type, but `AssetConfig` bypasses
it with manual dispatch instead of `Box<dyn Asset>` or a macro-generated forwarder.

**Why deferred:** Switching to `dyn Asset` changes object layout, potentially impacts
serialisation (`AssetConfig` derives `Serialize`/`Deserialize`), and requires threading
lifetime/ownership concerns through `SimState`. High correctness risk for incremental gain.

A lighter alternative: a `delegate_asset!` macro that generates all match arms from a single
declaration:

```rust
delegate_asset! {
    impl AssetConfig {
        fn step(state, setpoint_kw, dt) -> (AssetState, f64);
        fn capability(state) -> AssetCapability;
        ...
    }
}
```

---

## Notes

- `AssetProfile` (YAML deserialized, in `profile.rs`) and `AssetConfig` (runtime physics,
  in `assets/mod.rs`) share the same variant names (`Ev`, `Battery`, etc.) but hold different
  inner types. Consider renaming `AssetProfile` → `AssetSpec` to make the distinction explicit.

- `SimInjectState` mixes three injection behaviours (A = one-shot, B = frozen+EMA, C = frozen+snap)
  in a single flat struct. The clearing/decay logic for each behaviour is scattered across
  `state.rs` and `simulator/mod.rs`. A small `InjectBehaviour` tagged enum per field would
  make the intent self-documenting.
