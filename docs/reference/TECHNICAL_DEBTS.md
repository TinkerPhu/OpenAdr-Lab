# Technical Debts Register

> Source review: `VEN/src/` code quality review conducted 2026-04-28.
> Detailed diagnostics: `docs/plans/refactoring_backlog.md`
>
> **Rule:** Before adding a feature in an affected area, check this file first.
> Refactor the relevant debt before adding new behaviour if effort is Small or Trivial.

Priority legend: 🔴 High / 🟠 Medium-High / 🟡 Medium / 🔵 Low (large, deferred)

---

| ID | Description | Affected files | Effort | Risk | Priority |
|----|-------------|----------------|--------|------|----------|
| R-03 | Replace hardcoded string asset IDs in `dispatcher.rs` with shared constants. `asset_id()` methods in each asset file are the authoritative source — keep them; only the duplicate literals in dispatcher need replacing. | `VEN/src/controller/dispatcher.rs` | Small | Mechanical | 🟠 |
| R-08 | Replace `AssetConfig` manual dispatch enum (~9 methods × 5 variants) with `dyn Asset` or macro forwarder | `VEN/src/assets/mod.rs` | Large | Serialisation risk | 🔵 |
| R-09 | Inject clock into `tasks/planning.rs` instead of calling `Utc::now()` directly. Accept a `Fn() -> DateTime<Utc>` parameter in `spawn_planning` so the planning loop is testable without wall-clock coupling. `align_to_step` is already a pure function. Blocked on threading the clock through `spawn_planning`'s argument list. | `VEN/src/tasks/planning.rs` | Small | Low | 🟡 |

---

## Notes

- `AssetProfile` (YAML, `profile.rs`) and `AssetConfig` (runtime physics, `assets/mod.rs`)
  share variant names but hold different inner types. Consider renaming `AssetProfile` →
  `AssetSpec` to avoid newcomer confusion.
- `SimInjectState` mixes three injection behaviours in one flat struct. A tagged `InjectBehaviour`
  enum per field would clarify intent. Track here if promoted to a formal debt item.
