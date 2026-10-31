# Review Queue

Human-in-the-loop items found during sync/ingest/lint: contradictions, uncertain claims,
coverage gaps. Claude appends items instead of guessing; the human resolves or delegates.
Open items only — resolved items are deleted (resolution records live in `log.md` and git
history), per `wiki/CLAUDE.md`.

Format: `- [ ] YYYY-MM-DD — <description> (found during <workflow>; pages: page-slug, other-slug)`

- [ ] 2026-10-21 — Coverage gap: no page of its own for the CO₂ comfort bid (BL-17) and for the site-headroom / capacity-curve engine (`entities/site_headroom.rs`, `entities/capacity_curve.rs`, `simulator/capacity_headroom`); both are only mentioned inside `simulator`, `ven-ui` and `three-tier-plan-grid`. Comms-loss is covered across `dispatcher`, `deviation-arbiter` and `asset-layer`. (found during /wiki-sync; pages: simulator, ven-ui)
- [ ] 2026-10-31 — `SessionFlexibility` reports the same acceptable-rate band (0.35 / 0.05 €/kWh) and an effectively unlimited budget (1.0e9 €) for every session (`VEN/src/controller/milp_planner/session_flexibility.rs`); not in the debt register. Decide: derive from the session's comfort curve, omit, or declare as a profile default (a wire-contract question as well as a magic number). (found during /wiki-sync; pages: ven-code-vs-docs-audit, reliability-and-config)
- [ ] 2026-10-22 — `fleet.sh up` generator is behind the VEN: generated compose builds from `VEN/` (image now builds from repo root), sets the removed `POLL_STARTUP_JITTER_S` (jitter is profile `polling.*_pct` now) and no `FLEET_MQTT_*`. Needs a docker host to confirm, then fix or retire the generated-fleet path. (found during /wiki-sync; pages: fleet-tooling)
