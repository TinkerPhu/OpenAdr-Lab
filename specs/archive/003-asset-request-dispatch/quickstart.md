# Quickstart: Asset Request Dispatch Refactor

**Feature**: 003-asset-request-dispatch

## Summary

This refactor moves per-asset charging target resolution out of `controller/user_request.rs` and into each asset's own type. After this change, `user_request.rs` no longer imports `Profile` or `SimSnapshot`, and the `AssetState` enum handles dispatch.

## Changed Files

```
VEN/src/
├── simulator/
│   └── assets/
│       ├── mod.rs          ← add resolve_request_target method to AssetState
│       ├── ev.rs           ← add resolve_request_target to EvCharger
│       └── battery.rs      ← add resolve_request_target to Battery
├── controller/
│   └── user_request.rs     ← replace resolve_target; remove Profile/SimSnapshot imports
└── main.rs                 ← update post_requests handler
```

## Run Tests

```bash
# BDD integration tests (on Pi4-Server)
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  features/ven_user_request.feature"

# Full suite sanity check
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"
```

## Verify Acceptance Criteria

1. `grep -n "match body.asset_id" VEN/src/controller/user_request.rs` → no output
2. `grep -n "use crate::profile\|use crate::simulator::SimSnapshot" VEN/src/controller/user_request.rs` → no output
3. `POST /user-requests` with `asset_id: "ev", target_soc: 0.9` → 201 Created
4. `POST /user-requests` with `asset_id: "battery", target_soc: 0.9` → 201 Created
5. `POST /user-requests` with `asset_id: "pv"` → 422 with `"zero or negative"` error
