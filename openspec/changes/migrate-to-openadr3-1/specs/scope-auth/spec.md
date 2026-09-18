## ADDED Requirements

### Requirement: Scope-based access control replaces role-based access control
The VTN SHALL use OAuth scopes to determine access permissions. The supported scopes are
`read_all`, `read_targets`, `read_ven_objects`, `write_programs`, `write_events`,
`write_reports_bl`, `write_reports_ven`, `write_subscriptions_bl`, `write_subscriptions_ven`,
`write_vens_bl`, `write_vens_ven`, and `write_users`.

Fixtures and configuration SHALL name scopes in full. The bare aliases `write_reports`,
`write_subscriptions` and `write_vens` MUST NOT be used: each resolves to the **VEN** variant,
not the business one.

#### Scenario: Business client has full read and business write access
- **WHEN** a token is issued for `bl-client`
- **THEN** it grants `read_all write_programs write_events write_vens_bl write_reports_bl write_users`
- **AND** `GET /programs`, `GET /events`, `GET /vens`, `GET /reports` all return HTTP 200

#### Scenario: VEN client has filtered read access
- **WHEN** a token is issued for a VEN credential (e.g. `ven-1`)
- **THEN** it grants `read_targets read_ven_objects write_reports_ven`
- **AND** `GET /programs` returns only programs whose `targets` contain its clientId or are empty
- **AND** `GET /vens` returns only the VEN object linked to that clientId

#### Scenario: Missing scope returns 403
- **WHEN** a VEN token calls `POST /programs`
- **THEN** the response is HTTP 403

#### Scenario: read_all bypasses target filtering
- **GIVEN** programs targeted at various VENs
- **WHEN** a `read_all` holder lists programs
- **THEN** every program is returned with its full `targets` list

### Requirement: The business client creates VEN objects under the VEN's own identity
A client creating VEN objects on behalf of VENs SHALL hold `write_vens_bl`, so that `clientID`
is taken from the request body rather than from the caller's token subject.

#### Scenario: Business-created VEN keeps its own clientID
- **GIVEN** a client holding `write_vens_bl`
- **WHEN** it calls `POST /vens` with `clientID: "ven-7"`
- **THEN** the created VEN object has `clientID: "ven-7"`, not the caller's own client id

#### Scenario: The VEN-variant scope is rejected for business use
- **GIVEN** a client holding only `write_vens_ven`
- **WHEN** it calls `POST /vens` with `clientID: "ven-7"` in the body
- **THEN** the created VEN object's `clientID` is the caller's token subject
- **AND** a second such call fails on the `ven_client_id_unique` index
  <!-- this is why the fixture must say write_vens_bl, never the write_vens alias -->

### Requirement: BFF uses a single business credential
The BFF SHALL authenticate to the VTN with one `bl-client` credential, replacing the
dual-credential (any-business + ven-manager) model.

#### Scenario: BFF proxies all resource types with one token
- **WHEN** the BFF starts
- **THEN** it obtains one token as `bl-client`
- **AND** `GET /api/programs`, `GET /api/events`, `GET /api/vens`, `GET /api/reports` all succeed

#### Scenario: BFF token is refreshed on expiry
- **WHEN** the cached BFF token expires
- **THEN** the BFF transparently obtains a new token before the next proxied request

### Requirement: Every fleet VEN has a credential with VEN scopes
The fixtures SHALL provision credentials for **ven-1 … ven-20**, each with
`read_targets read_ven_objects write_reports_ven`, keeping the existing `ven-N` client ids.
`write_subscriptions_ven` SHALL be granted only once a subscription exists to write.

#### Scenario: Each VEN credential can authenticate
- **WHEN** `POST /auth/token` is called with a VEN's client id and secret
- **THEN** the response is HTTP 200 with a token whose `sub` is that client id

#### Scenario: Scopes exist before credentials are issued
- **WHEN** a VEN is provisioned
- **THEN** its user and scopes are created before its credential
- **AND** a token minted immediately afterwards carries the VEN scopes
  <!-- GB-49: credential-first ordering yields a scope-less token and a silently blind VEN -->
