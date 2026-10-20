# Phase 6 — Certification Track (transport modernisation)

> **Status (2026-10-11):** not started, not scheduled. What is left of phase 6 is the transport
> work of the certification cluster. The planner-fidelity track and the hygiene queue are done
> (see `docs/history/project_journal.md`), so they are no longer listed here.
> **Standing decision:** lab-learning first. The lab's VENs poll over HTTP on a trusted LAN, and
> transport work does not change fleet dynamics at a 30 s poll. Revisit when a latency experiment
> or certification becomes a goal.
> **Items:** Cluster H of `docs/BACKLOG_OpenADR_Cert.md` — TLS, `/auth/server` discovery, gzip,
> webhooks/subscriptions, MQTT, mDNS.
> **Prerequisites:** none.
> **Exit demonstration:** a cert-readiness re-audit of `docs/BACKLOG_OpenADR_Cert.md` with
> updated percentages (target: Communication §2 and Subscriptions §7 no longer 0 %).

The work packages keep their original numbers; the gaps (WP6.1–WP6.4) are finished work.

## WP6.5 — TLS and token-endpoint discovery (M–L)

1. HTTPS VTN transport: `reqwest` already compiles `rustls-tls` (`VEN/Cargo.toml`), but the VEN
   talks plain HTTP to the VTN. Certificate verification default-on, a profile flag
   `tls.allow_unverified` for the lab's self-signed setup (cert §2 SHOULD). Lab stack: terminate
   TLS at a reverse proxy (e.g. caddy/nginx container) in front of the VTN rather than patching
   openleadr-rs — keeps the fork diff clean (`docs/reference/FORK_PATCHES.md`).
2. `/auth/server` token-endpoint discovery with `/auth/token` fallback + caching (cert §3). The
   token fetch has one home to change: the client-credentials cache shared by `VEN/src/vtn.rs` and
   `VTN/bff/src/vtn_client.rs` (R-135 in `docs/reference/TECHNICAL_DEBTS.md`).
3. gzip `Accept-Encoding` (a `reqwest` feature flag, one unit test).

A full resilience-suite pass belongs to this WP: TLS-stack changes occasionally alter error text
that the resilience tests match on.

## WP6.6 — Webhooks: subscriptions + receiver (L–XL)

1. VEN gains an inbound HTTP listener (it already serves routes — reuse the axum server):
   `POST /callback` endpoint with echo-challenge verification (cert §7 MUST).
2. Subscription lifecycle: on startup create subscription objects for programs/events (+ reports
   where useful), renew/verify periodically, delete on shutdown; polling stays as fallback when
   subscription creation fails (the fleet must keep working against a VTN without webhook
   support).
3. VTN side: the 3.1 VTN has a subscription API (`openleadr-rs/openleadr-vtn/src/api/subscription.rs`,
   `WEBHOOK` mechanism). Check first whether its webhook delivery and echo challenge are complete;
   if not, that becomes an upstream-contribution work item (upstream PR rules apply) and this WP
   splits into VTN-side and VEN-side halves.
4. Measure the payoff with the experiment harness: compliance latency of S-4 (emergency) with
   polling vs. webhooks — 30 s poll vs. push is the one result that would justify this WP in the
   lab.

## WP6.7 — MQTT (optional, M) + mDNS (optional, S)

Only if WP6.6's latency experiment motivates a second transport.

- MQTT: the VTN side is planned separately in `docs/plans/mqtt_notifier_conformance.md` (planned,
  not scheduled, decision 2026-10-10). What belongs here is the VEN side: a notifier subscriber
  beside the poller, reusing the `rumqttc` connection setup of `VEN/src/weather.rs` and
  `VEN/src/measurement.rs`.
- mDNS discovery is a cert SHOULD with near-zero lab value — implement last or explicitly close
  as won't-do-in-lab in the cert backlog.

## Order & risks

WP6.5 → WP6.6 → WP6.7.

Risks: (a) the webhook receiver opens an inbound port on the VEN — even in the lab, bind it to
the docker network only and note the security posture in [[openadr-security]]; (b) closing plain
HTTP breaks every script and test that talks to the VTN directly (`scripts/seed_vtn.py`, the BDD
steps), so the proxy has to serve both until they are moved.

## Bookkeeping

`docs/BACKLOG_OpenADR_Cert.md` was re-audited row by row on 2026-10-11 and lists three open
MUSTs: TLS, `/auth/server` discovery and the webhook callback with echo challenge
(`DELETE /reports/{id}` is done, as a client call only tests use). At the end of the phase update every touched row and percentage, then journal + `/wiki-sync`
([[openadr-security]], [[reliability-and-config]], [[vision-and-roadmap]] — the certification
OPEN QUESTION gets its answer here).
