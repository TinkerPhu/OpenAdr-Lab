# Design

## Inventory

| Place | Today | After |
|---|---|---|
| `profile/validate.rs::validate_asset_roster` | refuses a second asset of a kind | refuses a repeated asset id |
| `docs/reference/architectural_smells.md`, `VEN_ARCHITECTURE.md` intro | the one-per-kind boundary | the reversed decision (W1-W7) |
| E2E | no multi-asset VEN | `test-ven-multi` with `multi_asset_test.yaml` |

Reused: `fleet_profile` (moved from `phase2_spikes.rs` to the shared test fixtures), the
production `simulator::plan_context::build_asset_contexts`, the profile-routing step.

## Decisions

- **M1** Ids are the identity everywhere, so uniqueness is the validation; the number of assets
  of a kind is not limited.
- **M2** The one-measured-asset-per-kind rule stays (one MQTT topic each).
