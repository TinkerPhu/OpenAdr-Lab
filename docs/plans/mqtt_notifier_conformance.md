# Plan: make the VTN's MQTT notifier conform to OpenADR 3.1 (planned, not scheduled)

**Status: planned, not scheduled.** Decision (user, 2026-10-10): recorded as a plan and not
implemented now; for the lab it is not important yet and would only be a hindrance. TLS scope when
it is done: every client, plain port closed.

## Context

Asked "how is it supposed to be according to the standard?" for GB-52 (E2E broker anonymous,
production credentialed), the check against the 3.1 spec found that the gap is not GB-52 but the
VTN's MQTT notifier binding as a whole. Verified 2026-10-10 on `origin/main` `9b3c6eee`,
openleadr-rs `da565b4`:

| # | Finding | Evidence |
|---|---|---|
| 1 | No TLS. Spec: "MQTT clients **MUST** use MQTT over TLS (MQTTS) to connect to the VTN's MQTT broker" (Definition, "MQTT", L1725; TLS 1.2+, L1701; self-signed allowed on a private network, L1705). | `VTN/mosquitto/lab-mqtt.conf` has one plain `listener 1884`; `rumqttc = "0.25"` with `default-features = false` in `VEN/Cargo.toml` and `VTN/bff/Cargo.toml` (no TLS compiled) |
| 2 | The announced login cannot work. `GET /notifiers` always says `OAUTH2_BEARER_TOKEN`, username `{clientID}` ("Use token as password", Definition Table 12). The broker checks a fixed password file and has no auth plug-in. | `openleadr-vtn/src/api/subscription.rs:704-731`; `lab-mqtt.conf` |
| 3 | The announced address is the VTN's own internal dial string `mqtt://lab-mqtt:1884`: plain scheme, a Docker service name. | `subscription.rs:714` (`uris: vec![mqtt_state.url.clone()]`); deployed env on Node1 |
| 4 | The VTN's notifications are very likely all dropped. `MQTT_TOPIC_PREFIX=openadr-lab/vtn` (no trailing slash) is concatenated raw, so topics are `openadr-lab/vtnprograms/create`; the ACL grants `openadr-vtn` only `openadr-lab/vtn/#`. Inferred from code and config; mosquitto does not log the denial and still acknowledges the publish (`KEY_LEARNINGS.md` ~L2358). | `subscription.rs:476-512`, `VTN/docker-compose.yml:188`, `VTN/mosquitto/fleet-acl.conf` |
| 5 | Nobody consumes it. No VEN, BFF, script or test calls `/notifiers` or subscribes to `openadr-lab/vtn…`; VENs poll over HTTP. | search of `VEN/src`, `VTN/bff/src`, `lab-core`, `scripts`, `tests` |
| 6 | The spec offers three login methods only (`ANONYMOUS`, `OAUTH2_BEARER_TOKEN`, `CERTIFICATE`); a fixed username/password is not expressible. openleadr-wire lacks the `CERTIFICATE` variant and the required `WEBHOOK` field of `NotifiersResponse`. | YAML L2655-2725; `openleadr-wire/src/subscription.rs:208-240` |
| 7 | "A VTN **MUST** configure, and enforce, access to the MQTT broker's topics ... consistent with the security policy defined in the OpenAPI" (L1727). The lab ACL lets no VEN read any notifier topic, and has no per-VEN rule for them. | `fleet-acl.conf` |

What works and must keep working: fleet telemetry (VEN -> `lab-mqtt` -> BFF) with per-client
passwords and ACL (`docs/architecture/VTN_ARCHITECTURE.md` L260-303, `fleet_telemetry.feature`,
`scripts/test_fleet_acl.sh`).

## Target state

1. `lab-mqtt` accepts TLS only (port **8884**, mirroring the 1883/1884 house/lab convention); plain
   1884 is closed. VTN, BFF and all 20 VENs connect over TLS.
2. A client that follows `GET /notifiers` to the letter gets in: reachable `mqtts://` address,
   username = its OAuth client id, password = its access token.
3. The broker enforces topic access that mirrors the HTTP scopes: a VEN reads only
   `…/vens/{its venID}/#` (and the program/event topics its scope allows); the business-logic
   client reads all; only the VTN writes.
4. The test broker runs the same TLS, credentials and ACL as production (closes GB-52), and a BDD
   scenario connects exactly as announced.
5. The lab's VENs keep polling; MQTT notifications are an offered interface, used in the lab by the
   BFF (one real consumer, see stage 2) and by the BDD client.

## Stages

Each stage is one branch, test-first, full E2E, and leaves nothing false announced.

### Stage 0: stop the false announcement (small, can be done alone)

- `MQTT_TOPIC_PREFIX` default becomes `openadr-lab/vtn/` in `VTN/docker-compose.yml`,
  `tests/docker-compose.test.yml`, `tests/docker-compose.openleadr-test.yml`. Test first: a
  scenario (or `scripts/test_fleet_acl.sh` case) that subscribes as `openadr-vtn` to
  `openadr-lab/vtn/#`, creates a program and sees `…/vtn/programs/create`.
- Fork patch in openleadr-rs (record in `docs/reference/FORK_PATCHES.md`): a `MQTT_ANNOUNCE`
  switch (default on, upstream behaviour); the lab sets it off, so `GET /notifiers` returns no MQTT
  binding until stage 2 makes one true. The VTN keeps publishing.
- `docs/reference/WIRE_PROFILE.md`: one line that the lab VTN currently offers no MQTT notifier
  binding to clients.

### Stage 1: TLS for every client, plain port closed

- **Certificates, never in the repo** (it is public). `scripts/gen_lab_mqtt_tls.sh`, modelled on
  `scripts/gen_fleet_mqtt_secrets.py`: a lab CA plus a broker certificate with SANs `lab-mqtt`,
  the Node1 LAN IP and hostname, written to a git-ignored directory on Node1; the CA certificate
  (public part only) copied to Node2. Rotation = re-run and redeploy.
- **Broker** (`VTN/mosquitto/lab-mqtt.conf`): add `listener 8884` with `cafile`/`certfile`/
  `keyfile`, `tls_version tlsv1.2`; keep `listener 1884` during the migration only.
- **VTN**: `MQTT_URL=mqtts://lab-mqtt:8884`, `MQTT_CA_PATH` (already supported:
  `openleadr-vtn/src/state.rs:314-322`, `subscription.rs:87-91`; paho links OpenSSL already).
- **VEN and BFF**: enable rumqttc's rustls feature; **one** shared connection builder in `lab-core`
  (host, port, credentials, CA path -> `MqttOptions` with `Transport::tls`), used by
  `VEN/src/fleet_telemetry.rs` and `VTN/bff/src/fleet.rs`, which today each build their own
  (`one-concept-one-function`). New env `FLEET_MQTT_CA_PATH`; with it unset and a TLS port, fail
  visibly at start-up. The house-broker clients (`VEN/src/measurement.rs`, `weather.rs`) stay plain
  on 1883 (not the VTN's broker; R-54 is the house's own decision).
- **Dependencies**: `cargo audit` and licence review of the rustls tree (aws-lc-sys/OpenSSL
  licence is already on the accepted list); check the image build time on Node1.
- **Tests / GB-52**: `tests/mosquitto-test.conf` gets the TLS listener, `allow_anonymous false`,
  a password file and the production ACL on 8884; 1883 stays anonymous for the house-side
  weather/measurement scenarios (per-listener settings: verify `per_listener_settings true` works
  with this config before relying on it). Test certificates are generated at stack start, not
  committed. `fleet_telemetry.feature` then runs over TLS with credentials; add a scenario that a
  wrong password and a plain connection are both refused.
- **UI** (`ui-transparency`): the VEN UI's and VTN UI's connection status rows show "TLS" for the
  fleet broker.
- **Migration**: deploy broker with both listeners -> VTN and BFF -> VEN canary -> fleet in
  batches (`/tmp/fleet_roll.sh` pattern) -> verify no client left on 1884 (`mosquitto` connection
  log) -> remove the plain listener and the host port mapping.

### Stage 2: bearer-token login and per-client topic access

- **Broker auth**: mosquitto with an auth plug-in whose HTTP backend asks a lab service
  "may this username/password connect" and "may it read/write this topic" (candidate:
  mosquitto-go-auth with its `http` backend, plus its `files` backend for the three fixed accounts
  that are not OAuth clients: `openadr-vtn`, `openadr-bff`, and the VEN telemetry users, or move
  those to tokens too). **Spike first**: one day to prove the plug-in image runs on arm64 (Node1 and
  Node2), validates a real VTN token, and survives a broker restart; if it does not, fall back to a
  small custom broker-side check or to the spec's `CERTIFICATE` method, and re-plan.
- **The decision service** lives in the BFF (it already holds a VTN client): validate the token
  (signature and expiry, with the VTN's key), map client id -> role and venID, answer the ACL from
  the same rules as the HTTP scopes (`read_all`, `read_bl`, `read_ven_objects`; Definition
  L1125-1151). One rule table, shared with nothing else.
- **VTN (fork patches)**: announce a configured public address (`MQTT_PUBLIC_URIS`, e.g.
  `mqtts://<Node1>:8884`) instead of the internal dial string; add the `CERTIFICATE` variant and the
  `WEBHOOK` field to openleadr-wire so the wire types match the schema; turn `MQTT_ANNOUNCE` back
  on. Offer these upstream once tested (upstream calls broker security out of scope, so the public
  address and the wire fixes are the upstreamable part).
- **A real consumer** (`no-half-built-features`): the BFF subscribes to the notifier topics as the
  business-logic client and uses them to refresh its event/program cache instead of its poll, with
  the poll kept as fallback; the VTN UI shows notifier status (connected, last message).
- **BDD**: a scenario that reads `GET /notifiers` and `/notifiers/mqtt/topics/vens/{venID}/events`
  as a VEN client, connects exactly as announced (mqtts, client id, token), receives the
  notification for a new event targeted at it, and is refused on another VEN's topic and with an
  expired token.
- **Docs**: `docs/architecture/VTN_ARCHITECTURE.md` (authorisation model), `WIRE_PROFILE.md`
  (the lab's MQTT profile: TLS, bearer token, topic rules), `docs/use-cases/` (a VEN subscribing
  instead of polling), `wiki/concepts/openadr-security.md`.

## Why not the alternatives

- **Withdraw the notifier** (unset `MQTT_URL`): honest and tiny, but drops a 3.1 feature the lab
  migrated on purpose; kept as the fallback if the stage 2 spike fails.
- **Announce `ANONYMOUS`**: allowed by the schema, but breaks the topic-access rule (L1727): any
  client could read every VEN's events.
- **Announce `CERTIFICATE`**: the schema makes the VTN hand out a client certificate *and private
  key* in an API response; more moving parts than tokens and a worse secret-handling story.

## Costs and risks (why it is deferred)

- Stage 1 touches every image and needs a full fleet redeploy and certificate distribution to
  Node2; a certificate mistake takes fleet telemetry down until fixed.
- Stage 2 adds a third-party broker plug-in and a new always-on dependency (broker -> BFF) in the
  login path; token expiry means clients must reconnect with a fresh token.
- None of it changes what the lab's VENs do today: they poll.

## Verification (when implemented)

- Stage 0: `GET /notifiers` shows no MQTT binding; the prefix scenario sees `…/vtn/programs/create`.
- Stage 1: `openssl s_client -connect <Node1>:8884` shows the lab CA chain and TLS >= 1.2; a plain
  connect to 1884 is refused; `fleet_telemetry.feature` green over TLS; all 20 VENs report
  telemetry; full E2E.
- Stage 2: the announced-login BDD scenario green on Node2 and against the production stack on
  Node1; a wrong token and a foreign topic are refused; full E2E.

## Register

GB-52 (`docs/BACKLOG.md`) points here: its first step is stage 1's credentialed, TLS test broker.
Findings 1-7 live in this plan only, with no register row each, until a stage is started.
