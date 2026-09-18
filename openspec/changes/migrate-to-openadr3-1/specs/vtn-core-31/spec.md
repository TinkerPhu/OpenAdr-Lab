## ADDED Requirements

### Requirement: Submodule tracks a fork rebased onto upstream main
The `openleadr-rs` submodule SHALL reference `TinkerPhu/openleadr-rs` at a branch rebased onto
`upstream/main` (OpenADR 3.1, merged upstream 2026-03-13 as #313). The abandoned
`openadr3_1` branch and the `v0.2.0-rc1` tag SHALL NOT be used as the base.

#### Scenario: Submodule is on the rebased branch
- **WHEN** `git submodule status` is run in the lab root
- **THEN** the hash matches the tip of the rebased fork branch
- **AND** that branch's merge base with `upstream/main` is `upstream/main` itself

#### Scenario: Lab patches are present after the rebase
- **WHEN** the submodule is inspected
- **THEN** the active-event filter, the report cascade-delete migration and the cached Docker
  build are present
- **AND** the VEN_NAME target reconstruction code is absent

### Requirement: VTN starts with the 3.1 schema applied
The VTN SHALL apply migration `20260213100612_openadr_3.1.sql` and every later migration on a
fresh database.

#### Scenario: Schema matches 3.1
- **WHEN** the VTN container starts against an empty database
- **THEN** `program`, `event`, `ven`, `resource` have `targets text[] NOT NULL`
- **AND** `ven` has `client_id text NOT NULL` with a unique index
- **AND** `event` has a `duration` column and `report` has `client_id` but neither `program_id` nor `ven_id`
- **AND** `ven_program` does not exist
- **AND** the role tables (`any_business_user`, `user_ven`, `ven_manager`, `user_business`,
  `user_manager`, `business`) do not exist

### Requirement: The token endpoint is present in the deployed image
The deployed VTN SHALL expose `POST /auth/token` and the `/users` API, which exist only when the
image is built with the `internal-oauth` feature.

#### Scenario: Token endpoint answers after a fresh deploy
- **WHEN** `POST /auth/token` is called with the `bl-client` fixture credentials
- **THEN** the response is HTTP 200 with a Bearer access token

#### Scenario: A build without the feature is caught immediately
- **WHEN** `POST /auth/token` returns HTTP 404 after deploy
- **THEN** the deployment is treated as failed at this step
  <!-- upstream drops internal-oauth from default features; our Dockerfile must pass it -->

#### Scenario: Programs endpoint reachable with a business token
- **WHEN** `GET /programs` is called with a valid `bl-client` token
- **THEN** the response is HTTP 200 with a JSON array
