## ADDED Requirements

### Requirement: Submodule points to openadr3_1 branch
The `openleadr-rs` git submodule SHALL reference the `TinkerPhu/openleadr-rs` fork at the
`openadr3_1` branch, which tracks `upstream/openadr3_1` (tag `v0.2.0-rc1`).

#### Scenario: Submodule is on the correct branch
- **WHEN** `git submodule status` is run in the lab root
- **THEN** the submodule hash matches the tip of `TinkerPhu/openleadr-rs@openadr3_1`

#### Scenario: VTN starts with 3.1 migration applied
- **WHEN** the VTN container starts with a fresh (empty) database
- **THEN** the database schema includes migration `20260213100612_openadr_3.1.sql`
- **AND** the `program`, `event`, `ven`, `resource`, `report` tables have `targets text[]` columns
- **AND** the `ven` table has a `client_id text NOT NULL` column
- **AND** the `ven_program` table does NOT exist
- **AND** the old role tables (`any_business_user`, `user_ven`, `ven_manager`, etc.) do NOT exist

### Requirement: VTN health check passes after 3.1 migration
The VTN SHALL respond to auth and core object endpoints after fresh deploy with the 3.1 schema.

#### Scenario: Token endpoint works after migration
- **WHEN** `POST /auth/token` is called with `bl-client` / `bl-client` credentials
- **THEN** the response is HTTP 200 with a `Bearer` access token

#### Scenario: Programs endpoint reachable with bl-client token
- **WHEN** `GET /programs` is called with a valid `bl-client` token
- **THEN** the response is HTTP 200 with a JSON array (empty or seeded)
