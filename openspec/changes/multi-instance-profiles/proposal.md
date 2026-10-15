# Proposal

## Why

W1-W6 made the planner, real-time control, sessions, feeds and UI work per asset id. The profile
validation still refused a second asset of a kind ("at most one 'pv' per VEN"), a decision
recorded as a boundary in `docs/reference/architectural_smells.md`. This is W7: lift it.

## What Changes

- Profile validation accepts several assets of one kind and instead refuses an asset id used
  twice (two EVs that omit their ids would both default to `ev`).
- A multi-asset test profile (two EVs, an east and a west PV array) with its own E2E VEN
  (`test-ven-multi`), a planner test through the production context builder, and BDD.
- The reversed decision is recorded where the boundary was.

## Capabilities

### New Capabilities
- `multi-instance-profiles`: a profile may declare several assets of one kind, each with its own
  id.

## Impact

- `VEN/src/profile/validate.rs`, `VEN/profiles/multi_asset_test.yaml`,
  `tests/docker-compose.test.yml`, `tests/features/ven_multi_asset.feature`, docs.
