# Tasks: Reconcile Battery Round-Trip-Efficiency Models

## 1. Decide

- [ ] 1.1 Resolve `design.md`'s open decision (D-A symmetric-split vs. D-B
      asymmetric) with the user before writing any code.

## 2. Test-first

- [ ] 2.1 Write a partial-cycle unit test (charge N kWh, then discharge M < N
      kWh, assert resulting SoC) in `VEN/src/assets/battery.rs`'s test module
      that encodes the *chosen* model's expected result. Confirm it fails
      against the current code (unless the chosen direction is D-B, in which
      case `battery.rs` already passes and the new test belongs in
      `battery_milp.rs` instead).
- [ ] 2.2 Same for `VEN/src/assets/battery_milp.rs`'s test module (a small
      solve or direct constraint-coefficient check), covering whichever file
      isn't already covered by 2.1.

## 3. Implement

- [ ] 3.1 Apply the chosen model to whichever file needs to change (D-A:
      `battery.rs::step_inner`/`forecast`; D-B:
      `battery_milp.rs::build_milp_context`'s `eff_ch`/`eff_dis` derivation
      only — the SoC-evolution constraint at lines 76-78 already takes
      independent `eff_ch`/`eff_dis` scalars and needs no change either way,
      per `design.md`'s correction note).
- [ ] 3.2 Confirm both new tests from section 2 pass, and no existing
      full-cycle test regresses (their assertions should be unchanged, since
      both models agree on full-cycle totals).

## 4. Verify

- [ ] 4.1 `wsl cargo test -p ven-app` (acquire `wsl_lock.sh` first).
- [ ] 4.2 `cargo fmt --check` and
      `cargo clippy --all-targets --all-features -- -D warnings`.
- [ ] 4.3 Once merged: delete `openspec/changes/battery-efficiency-model-reconciliation/`
      per this repo's workflow rule; note the resolved model choice in
      `battery.rs`'s/`battery_milp.rs`'s doc comments so it doesn't drift
      again silently.
