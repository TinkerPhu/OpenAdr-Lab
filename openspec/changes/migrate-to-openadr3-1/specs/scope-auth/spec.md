## ADDED Requirements

### Requirement: Scope-based access control replaces role-based access control
The VTN SHALL use OAuth scopes to determine access permissions. The following scopes SHALL be
supported: `read_all`, `read_targets`, `read_ven_objects`, `write_programs`, `write_events`,
`write_reports`, `write_subscriptions`, `write_vens`, `write_users`.

#### Scenario: bl-client has full read and write access
- **WHEN** a token is issued for `bl-client`
- **THEN** the token grants scopes `read_all, write_vens, write_programs, write_events, write_users`
- **AND** `GET /programs`, `GET /events`, `GET /vens`, `GET /reports` all return HTTP 200

#### Scenario: VEN client has filtered read access
- **WHEN** a token is issued for a VEN credential (e.g., `ven-1-client`)
- **THEN** the token grants scopes `read_targets, read_ven_objects, write_reports, write_subscriptions`
- **AND** `GET /programs` returns only programs where the client_id is in `targets` or `targets` is empty
- **AND** `GET /vens` returns only the VEN object linked to that client_id

#### Scenario: Missing scope returns 403
- **WHEN** a VEN token calls `POST /programs`
- **THEN** the response is HTTP 403

### Requirement: BFF uses a single bl-client credential
The BFF SHALL authenticate to the VTN with a single `bl-client` credential instead of the
former dual-credential (any-business + ven-manager) model.

#### Scenario: BFF proxies all resource types with one token
- **WHEN** the BFF starts
- **THEN** it obtains one token using `bl-client` / `bl-client`
- **AND** `GET /api/programs`, `GET /api/events`, `GET /api/vens`, `GET /api/reports` all succeed

#### Scenario: BFF token is refreshed on expiry
- **WHEN** the cached BFF token expires
- **THEN** the BFF transparently obtains a new token before the next proxied request

### Requirement: Three VEN fixtures exist with correct scopes
The VTN fixture SQL SHALL provision three VEN credentials (`ven-1-client`, `ven-2-client`,
`ven-3-client`) each with scopes `read_targets, read_ven_objects, write_reports, write_subscriptions`.

#### Scenario: Each VEN credential can authenticate
- **WHEN** `POST /auth/token` is called with `ven-1-client` / `ven-1-client`
- **THEN** the response is HTTP 200 with a token
- **AND** the same holds for `ven-2-client` and `ven-3-client`
