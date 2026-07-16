# VEN Backend — Refactoring Backlog

> Detailed diagnostics for the open refactoring items. Scope: `VEN/src/` (Rust backend).
>
> Priority legend: 🔴 High / 🟠 Medium-High / 🟡 Medium / 🔵 Low (large, deferred)
>
> Authoritative status register: `docs/reference/TECHNICAL_DEBTS.md`

---

## Open Items

| # | Issue | Priority | Effort | Risk |
|---|-------|----------|--------|------|
| R-08 | `AssetConfig` → `dyn Asset` dispatch or macro forwarder | 🔵 | Large | Correctness risk, deferred |

---

## Detailed Findings

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
