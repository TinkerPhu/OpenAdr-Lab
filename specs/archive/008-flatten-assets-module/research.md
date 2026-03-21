# Research: Flatten Assets Module (008)

## Overview

This is a pure structural refactor — no logic changes, no new dependencies, no external APIs.
All questions are answerable from the existing codebase. No external research needed.

---

## Decision 1: Flat files vs. sub-directories for moved assets

**Decision**: Keep flat `.rs` files (`assets/pv.rs`, `assets/battery.rs`, …).

**Rationale**: The current `simulator/assets/` layout already uses flat files. The backlog notation
(`pv/`, `battery/`, …) suggests future expansion but carries no current requirement. Converting
flat files to sub-directories would change every import path twice (once now, again when a second
file is added) for no present benefit. Lean Architecture (Constitution §IV) prohibits designing
for hypothetical future requirements.

**Alternatives considered**:
- Sub-directories with `mod.rs` — rejected; no current need, doubles churn.

---

## Decision 2: Where does AssetEntry live after the move?

**Decision**: `AssetEntry`, `SimState`, and `GridMeter` remain in `simulator/mod.rs`. Only the
five asset implementation files and their shared `mod.rs` (which holds `AssetState` and dispatch
helpers) move to `assets/`.

**Rationale**: The backlog wording ("AssetInterface trait + AssetEntry + Vec<AssetEntry> SimState"
in `assets/mod.rs`) describes a *desired end-state* for a later, broader restructure. RF-02 scope
is explicitly "Move simulator/assets/ to a top-level assets/ directory." Moving `AssetEntry` and
`SimState` is out-of-scope and would require changes throughout `simulator/mod.rs`,
`controller/dispatcher.rs`, and `main.rs` beyond what is described. This is deferred to a follow-up
task.

**Alternatives considered**:
- Move `AssetEntry`+`SimState` as well — rejected; exceeds stated scope, risks breaking dispatcher tests.

---

## Decision 3: How to wire the new top-level module

**Decision**: Declare `pub mod assets;` at the top of `main.rs` (alongside the existing `mod simulator;`
declaration). `simulator/mod.rs` replaces `pub mod assets;` with `use crate::assets::*;` (or targeted
imports) so that existing code in `simulator/mod.rs` that references `AssetState`, `AssetCapabilities`,
etc. continues to compile without changes at call sites.

**Rationale**: Adding `mod assets;` at crate root (`main.rs`) is the idiomatic Rust way to introduce
a new top-level module. Re-exporting from `simulator/mod.rs` preserves the `crate::simulator::AssetState`
path for any call site that uses it, avoiding a wide blast radius of `use` changes.

**Alternatives considered**:
- Remove all `crate::simulator::AssetState` references and replace with `crate::assets::AssetState` — valid
  but touches more files; can be done as a follow-up cleanup if desired.

---

## Decision 4: File move order

**Decision**:
1. Create `VEN/src/assets/` with `mod.rs` and the five asset files (content copied from `simulator/assets/`).
2. Update `simulator/mod.rs` (remove `pub mod assets;`, add targeted `use crate::assets::*;`).
3. Declare `pub mod assets;` in `main.rs`.
4. `cargo build` — iterate on compiler errors.
5. Delete `VEN/src/simulator/assets/` directory.

**Rationale**: Build first, delete after — compiler errors guide any missed references before the
originals are gone.

---

## Current use-path inventory (from codebase scan)

| Path | Source | Notes |
|------|--------|-------|
| `crate::simulator::assets::*` | `simulator/mod.rs` | Direct sub-module declaration |
| `crate::simulator::AssetEntry` | `main.rs` likely via `simulator::*` | Stays in `simulator/mod.rs` |
| `crate::simulator::AssetState` | `simulator/mod.rs`, `controller/*`, `entities/asset.rs` | Will be `crate::assets::AssetState` |
| `crate::common::*` | `simulator/assets/mod.rs` | References `TimeSeries`, `AssetInterface` — these stay in `common/` |

> Exact use paths are confirmed at build time; the compiler will emit errors for every missed
> reference.
