# VEN Backend — Refactoring Backlog

> Detailed diagnostics for the open refactoring items. Scope: `VEN/src/` (Rust backend).
>
> Priority legend: 🔴 High / 🟠 Medium-High / 🟡 Medium / 🔵 Low (large, deferred)
>
> Authoritative status register: `docs/reference/TECHNICAL_DEBTS.md`

---

## Open Items

_(none currently — R-08, the last item tracked here, is resolved; see
`docs/history/project_journal.md`.)_

---

## Notes

- `AssetProfile` (YAML deserialized, in `profile.rs`) and `AssetConfig` (runtime physics,
  in `assets/mod.rs`) share the same variant names (`Ev`, `Battery`, etc.) but hold different
  inner types. Consider renaming `AssetProfile` → `AssetSpec` to make the distinction explicit.

- `SimInjectState` mixes three injection behaviours (A = one-shot, B = frozen+EMA, C = frozen+snap)
  in a single flat struct. The clearing/decay logic for each behaviour is scattered across
  `state.rs` and `simulator/mod.rs`. A small `InjectBehaviour` tagged enum per field would
  make the intent self-documenting.
