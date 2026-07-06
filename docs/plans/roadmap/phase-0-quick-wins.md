# Phase 0 — Quick Wins

> **Goal:** bank the highest value-density small items before any new architecture.
> **Items:** BL-02, GB-02, GB-03, BL-12, GB-10.
> **Exit demonstration:** event-priority arbitration and EV realism merged; all three
> VENs named/ID'd uniformly; every build (VEN, BFF, UIs) compiles with zero warnings.
> **Total effort:** ~1 week.

## WP0.1 — BL-02: Event priority ordering before merge (S)

**Problem recap:** `openadr_interface.rs` merges events in array order; the `priority`
field is never read, so a low-priority event received later overwrites a high-priority one.

Steps (test-first):
1. Unit test 1: two PRICE events covering the same interval, priorities 1 and 5 —
   assert the priority-1 value survives the merge regardless of array order.
2. Unit test 2: equal priority, different `createdDateTime` — newer wins.
3. Unit test 3: event with absent `priority` — sorts *after* any event with an explicit
   priority (spec: lower integer = higher priority; treat `None` as lowest).
4. Implement: extract `priority: Option<i64>` and `createdDateTime` during translation;
   sort the event list by `(priority ascending, createdDateTime descending)` before the
   existing merge loop. No merge-loop changes beyond the pre-sort.
5. BDD (optional, if cheap): extend an existing events feature with an overlapping
   two-priority scenario rather than adding a new feature file.

Files: `VEN/src/controller/openadr_interface.rs` (+ its test module).
Watch: file is in the Domain ring — no new outer-ring imports.

## WP0.2 — GB-02 + GB-03: Uniform VEN naming and UUID IDs (M)

Do this **before** Phase 2's fleet generator bakes the old names into templates.

1. Inventory every occurrence of the current VEN-1 naming scheme:
   `VEN/profiles/ven-1.yaml` (vs. `ven-2/3`), VTN seed data, docker-compose service
   names/env, `tests/features/` fixtures and step implementations, VTN UI expectations.
   (`grep -ri "ven-1\|ven1" --include="*.yaml" --include="*.py" --include="*.feature" …`)
2. Decide the canonical scheme to match VEN-2/VEN-3 (this is the whole point of GB-02 —
   one pattern, no special case).
3. Switch VEN IDs to UUIDs (GB-03): generate one per profile, update seed script and
   every test/fixture reference. Keep `venName` human-readable; only the *ID* becomes
   a UUID.
4. Run the full E2E suite on Pi4 — this WP's risk is entirely in test fixtures, so E2E
   is the real verification. Fix every failure per the no-blame-categorisation rule.

Files: `VEN/profiles/*.yaml`, VTN seed scripts, `tests/features/**`, compose files.
Risk: silent references in BDD step code — budget time for a full E2E pass, not just grep.

## WP0.3 — BL-12: EV minimum charge rate + response delay (S)

1. Unit test 1: setpoint 0.5 kW with `min_charge_kw` = 1.5 → actual power 0 kW.
2. Unit test 2: setpoint 2.0 kW → actual 2.0 kW (above floor, unchanged).
3. Unit test 3: setpoint 7 kW at t=0 → actual still previous value at t=0, becomes
   7 kW one delay-step later (single-step lag buffer).
4. Implement in `VEN/src/assets/ev.rs` update logic: snap `0 < sp < min_charge_kw` to 0;
   store commanded setpoint, apply previous tick's command.
5. `min_charge_kw` and `response_delay_s` come from `BatteryParams`-style typed profile
   struct (profile rule: no `use crate::profile` below the application layer) — add to
   the existing EV params struct with defaults (1.5 kW, 10 s) so existing profiles need
   no edits.
6. Check E2E timeline scenarios that assert EV power values — the one-tick lag may
   shift assertions by one sample.

Files: `VEN/src/assets/ev.rs`, EV params struct, possibly one BDD fixture.

## WP0.4 — GB-10: Zero compiler warnings (S–M)

1. `wsl cargo build 2>&1 | grep warning` in `VEN/` and `VTN/bff/`; `npm run build` in
   both UIs; collect the list.
2. Fix root causes; `#[allow(...)]` only with a same-line justification comment
   (linting rule). Dead code that maps to a BL item gets quarantined the way the R5
   review did (see `docs/plans/review_items_resolution_strategy.md`), not deleted.
3. Consider adding `RUSTFLAGS="-D warnings"` to the Pi4 docker build once clean, so
   regressions fail the build (matches the clippy gate that already exists).

## Order & bookkeeping

WP0.1 → WP0.3 → WP0.4 → WP0.2 (WP0.2 last: it's the only one that touches the shared
E2E fixtures, so land the code-only WPs first). Mark BL-02/BL-12/GB-02/GB-03/GB-10
resolved in `docs/BACKLOG.md`; journal entry; `/wiki-sync` (touches
[[openadr-interface]] and [[asset-layer]] wiki pages).
