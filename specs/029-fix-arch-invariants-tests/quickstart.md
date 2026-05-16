# Quickstart: Fix Architecture Invariant Gaps and Missing Tests (029)

## Verify invariants (before and after)

```powershell
# All five must return empty:
wsl bash -c "grep 'use crate::simulator\|use crate::assets' VEN/src/controller/reporter.rs"
wsl bash -c "grep 'use crate::assets' VEN/src/controller/timeline.rs"
wsl bash -c "grep -r 'use crate::profile' VEN/src/tasks"
wsl bash -c "grep -r 'use crate::vtn::VtnClient' VEN/src/tasks"
wsl bash -c "grep -r 'use crate::assets\|use crate::simulator' VEN/src/services"

# Line count check (must be ≤ 200):
wsl bash -c "wc -l VEN/src/tasks/sim_tick/tick.rs"
```

## Compile check (after each item)

```powershell
wsl cargo check --manifest-path VEN/Cargo.toml
```

## Unit tests (after all items)

```powershell
wsl bash -c "cd VEN && cargo test 2>&1 | tail -40"
```

## BDD integration test (final gate — Pi4-Server)

```powershell
ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull && docker compose run --rm ven-test 2>&1 | tail -30"
```

Expected: 44 features, 238 scenarios, 0 failures.

## Implementation order

1. Item 5 (doc fix — `docs/plans/ven_backend_architecture_refactoring.md`)
2. Item 2 (ObligationService signature + task extraction)
3. Item 3 (`AbsorberState::Default` + `tick_tests.rs`)
4. Item 4 (`planning.rs` smoke test)
