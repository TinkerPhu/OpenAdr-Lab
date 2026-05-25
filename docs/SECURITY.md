# Security Concept

> Scope: OpenADR-Lab — VEN, VTN BFF, VTN UI, VEN UI.
> Deployment context: Pi4 local network; not internet-exposed. Lab/research environment.

---

## Authentication model

| Interface | Mechanism | Notes |
|-----------|-----------|-------|
| VEN → VTN | OAuth 2.0 client credentials grant | Credentials in profile YAML; token fetched from `/auth/token`, cached in-memory, refreshed on expiry |
| VTN BFF → VTN | OAuth 2.0 client credentials grant | Two clients: `business` (operator) + `ven-mgr` (VEN management). Credentials in BFF env/config |
| VEN UI → VEN | None | Local network only; no auth layer on VEN REST API |
| VTN UI → VTN BFF | None | Local network only; BFF relies on network isolation |

OAuth flow handled by `openleadr-rs` (VTN side) and `VEN/src/vtn.rs` (client side).

---

## Threat model

| # | Threat | Likelihood | Impact | Mitigation |
|---|--------|-----------|--------|------------|
| T-1 | OAuth credentials exposed in profile YAML committed to a public repo | Medium | High | `profile.yaml` in `.gitignore`; inject via docker env in production |
| T-2 | Malformed/malicious OpenADR events from VTN corrupting VEN state | Low | Medium | `serde` deserialization rejects unknown or type-mismatched payloads |
| T-3 | No TLS between VEN and VTN | Low | Low | Acceptable for local lab network; add TLS if exposed beyond Pi4 |
| T-4 | VEN REST API has no authentication (any local process can send commands) | Low | Low | Local network trust; acceptable for lab; add token auth before production use |
| T-5 | Prometheus `/metrics` endpoint exposes internal counters without auth | Low | Low | Same network-isolation mitigation as T-4 |

---

## Known unmitigated risks

- No rate limiting on VEN REST API (`/user-requests`, `/sim/inject`, etc.)
- No request signing or HMAC verification on VEN REST API
- `cargo audit` and `npm audit` not yet run; dependency vulnerability status unknown
- `.github/workflows/` empty — no automated security scanning in CI

Add findings from `cargo audit` / `npm audit` to `docs/BACKLOG.md` with severity when run.

---

## Security review cadence

Run `/security-review` before each release and after each major feature.
Feed all findings into `docs/BACKLOG.md` as tracked items (never as inline code comments).

---

## Out of scope (lab environment assumptions)

- Certificate management / PKI
- Secret rotation
- Audit logging / SIEM integration
- Multi-tenant isolation
