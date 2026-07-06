# Phase 6 — Fidelity & Certification Track

> **Goal:** planner cost fidelity (bills computed right), UI control realism, and the
> deferred transport-modernisation work (TLS, webhooks, MQTT) plus the accumulated
> hygiene queue. Unlike Phases 1–5, this is a **grab-bag of independent work
> packages** — they can be interleaved into earlier phases whenever priorities shift
> or a WP elsewhere touches the same files.
> **Items:** BL-11, BL-13, BL-09, BL-27, BL-18, Cluster H (TLS, webhooks, MQTT,
> `/auth/server`, gzip, `randomizeStart`, "now" sentinel, runtime reconfig, mDNS),
> dependency-vulnerability batch, Cluster I hygiene (BL-21/22/23/26/29,
> GB-01/04/05/08).
> **Prerequisites:** none hard; WP6.3 benefits from Phase 3's constraint work.
> **Exit demonstration:** (a) a slot-cost unit-test suite proving bills match
> boundary-straddling tariffs and peak penalties; (b) `cargo audit` + `npm audit`
> clean; (c) cert-readiness re-audit of `docs/BACKLOG_OpenADR_Cert.md` with updated
> percentages (target: Communication §2 and Subscriptions §7 no longer 0 %).
> **Total effort:** ~5–7 weeks if done as one block; more likely spread out.

## Track A — planner fidelity

### WP6.1 — BL-11: time-weighted tariff averaging (S–M)

1. Unit test first (BL-11's verify): 10-min slot spanning a tariff boundary at
   minute 7 → weighted average `(7×0.20 + 3×0.15)/10 = 0.185`.
2. Replace `tariff_at(slot.start)` with overlap-weighted mean via the existing
   `TimeSeries` abstraction; capacity uses `min()` over overlapping intervals.
3. This also closes UC:§7.4's "irregular interval edges blur" gap at the costing
   level (grid resolution unchanged, but cost now integrates correctly across edges).
4. Expect small numeric shifts in existing planner tests — adjust expectations
   *with justification per test* (the values become more correct, document why).

### WP6.2 — BL-13: early firm-up heuristic (S)

1. Unit tests per BL-13: flat rate (CoV < 0.10) → FLEXIBLE reclassified FIRM;
   variable rates → unchanged.
2. After Phase 7 of the plan pipeline: compute coefficient of variation across
   FLEXIBLE slots; if < 0.10, reclassify and re-run allocation Phases 2–5.
3. Interaction check: with Phase 4's OPPORTUNISTIC mode, firm-up must not defeat
   "only when ~free" semantics — add one combined test.

### WP6.3 — BL-09: penalty threshold check — planner Phase 6 (L)

1. Config: penalty rules in profile (threshold kW per measurement window, penalty
   €/kW), typed params struct (profile rule).
2. After allocation: project per-window peak; if above threshold, compare penalty
   cost vs. rescheduling cost; reallocate when cheaper. Prefer expressing this
   *inside* the MILP as a soft-penalty term over post-hoc reallocation if the solver
   formulation allows — decide at proposal time with a small formulation spike.
3. BDD per BL-09's verify: 10 kW threshold, 12 kW load in one slot → planner splits
   across two slots.
4. KPI hook: `kpi.py` gains penalty-cost-avoided — makes S-3 (capacity limit)
   vs. penalty-based control comparable, a nice experiment extension (S-7 candidate).

### WP6.4 — BL-27 + BL-18: control-mode metadata + live flex widget (M)

1. BL-27: add `adjustability: PowerAdjustability` + `power_steps_kw: Vec<f64>` to the
   live `AssetCapability`/`ControlDescriptor` path (per the BL's own recommendation —
   do *not* revive the entities-level duplicates); each asset's `capability()`
   reports its true mode (EV: stepped amps; battery: continuous; heater: on/off).
   UI sliders snap to reported steps (BL-27 verify).
2. BL-18 (`AssetFlexibility`): **resolve the recorded scope question first** —
   recommendation: implement as a thin on-demand computation for a live UI widget
   ("this asset can flex ±X kW right now"), no persistence; if the UI team-of-one
   doesn't want the widget, close BL-18 as superseded by `FlexibilityEnvelope` and
   delete the sketch. Either outcome is a valid resolution; record it.

## Track B — transport modernisation (cert cluster)

Order chosen so each step de-risks the next; the vuln batch rides with TLS since
both touch the reqwest stack.

### WP6.5 — Dependency-vulnerability batch + TLS (M–L)

1. Upgrade `reqwest` (and thereby `aws-lc-sys ≥ 0.39`, `rustls-webpki ≥ 0.103.13`,
   `quinn-proto ≥ 0.11.14`) per the BACKLOG audit table; `npm audit fix` in both UIs.
   Full test suites after — TLS-stack upgrades occasionally change error text that
   resilience tests match on.
2. HTTPS VTN transport: rustls features on, certificate verification default-on,
   profile flag `tls.allow_unverified` for the lab's self-signed setup (cert §2
   SHOULD). Lab stack: terminate TLS at a reverse proxy (e.g. caddy/nginx container)
   in front of the VTN rather than patching openleadr-rs — keeps the fork diff clean.
3. `/auth/server` token-endpoint discovery with `/auth/token` fallback + caching
   (cert §3).
4. Small cert line items alongside: gzip `Accept-Encoding` (reqwest feature flag),
   `randomizeStart` support, "0001-01-01 = now" start-time sentinel (each S, each
   with one unit test; they share `openadr_interface`/`vtn.rs` files with this WP).

### WP6.6 — Webhooks: subscriptions + receiver (L–XL)

1. VEN gains an inbound HTTP listener (it already serves routes — reuse the axum
   server): `POST /callback` endpoint with echo-challenge verification (cert §7 MUST).
2. Subscription lifecycle: on startup create subscription objects for
   programs/events (+ reports where useful), renew/verify periodically, delete on
   shutdown; polling stays as fallback when subscription creation fails (the fleet
   must keep working against a VTN without webhook support).
3. Check openleadr-rs fork's subscription support first — if the VTN side is
   incomplete, that becomes an upstream-contribution work item (upstream PR rules
   apply) and this WP splits into VTN-side and VEN-side halves.
4. Measure the payoff with the Phase-3 harness: compliance latency S-4 (emergency)
   with polling vs. webhooks — a genuinely interesting experiment result (30 s poll
   vs. push).

### WP6.7 — MQTT (optional, M) + mDNS (optional, S)

Only if WP6.6's latency experiment motivates a second transport. MQTT listener
beside the poller (rumqttc, pinned; broker container on Pi4); mDNS discovery is a
cert SHOULD with near-zero lab value — implement last or explicitly close as
won't-do-in-lab in the cert backlog.

## Track C — hygiene queue (Cluster I)

One consolidated cleanup PR (or opportunistic rides on Track A/B PRs touching the
same files):

| Item | Action |
|------|--------|
| BL-21 | Confirm `assets/heater.rs` params superset → delete entities-level `ThermalModelParams` |
| BL-26 | Compare entities `AssetState` vs. live `assets::AssetState` → delete or fold missing monitoring fields |
| BL-29 | Fold `FlexibilityDirection` into BL-10's report code (done in Phase 3 — verify), close the rest pending multi-currency demand |
| BL-22 | Decide: wire `apply_battery_correction_overlay` behind `battery.deviation_correction_enabled` (default off) **or** delete after user re-confirmation — ask, don't assume |
| BL-23 | Route `post_heater_target` through `HvacService` (consistency with EV path) or delete the shell |
| GB-01 | Prune orphan docker containers on Pi4 (project-owned only — never touch productive containers) |
| GB-04 | `ends_at timestamptz` index for `?active=true` (SQL-side filtering) |
| GB-05 | VTN UI: filter past events from the event table |
| GB-08 | VEN UI tests for UserRequests + Controller pages |

## Order & risks

Tracks A, B, C are independent. Within B: WP6.5 → WP6.6 → WP6.7.
Suggested interleaving if this phase runs as a block: WP6.1 + WP6.2 (quick fidelity
wins) → WP6.5 (vulns overdue by then) → WP6.3 → WP6.6 → WP6.4 → Track C → WP6.7.

Risks: (a) reqwest major-version upgrade ripples through `vtn.rs` error handling —
budget a full resilience-suite pass; (b) webhook receiver opens an inbound port on
the VEN — even in the lab, bind it to the docker network only and note the security
posture in [[openadr-security]]; (c) BL-22 needs an explicit user decision — do not
resolve it unilaterally.

Bookkeeping: re-run the full cert audit and update every touched percentage in
`docs/BACKLOG_OpenADR_Cert.md`; mark BL-09/11/13/18/21/22/23/26/27/29 and GB items
resolved; journal + `/wiki-sync` ([[openadr-security]], [[reliability-and-config]],
[[vision-and-roadmap]] — the certification OPEN QUESTION gets its answer here).
