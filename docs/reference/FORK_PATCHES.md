# openleadr-rs fork patches

What this lab carries on top of upstream `openleadr-rs`, and why. The submodule tracks
`TinkerPhu/openleadr-rs` at `rebase/openadr3_1`, branched from `upstream/main` (OpenADR 3.1).

**A patch is removed only when upstream has actually solved it** — merged, or superseded by an
upstream implementation that provably does the same job. "Looks obsolete" is not a reason; the
evidence goes in the Retired table below, including how it was verified.

Check this file before any rebase onto a newer upstream. After rebasing, verify each live patch
is still present:

```bash
cd openleadr-rs && git log --oneline upstream/main..HEAD
```

## Live patches

| # | Commit | What | Why it is ours |
|---|---|---|---|
| **P-1** | `d86b23f` | **GB-04 active-event filter.** `event.ends_at` column + index (`migrations/20260918000200`), `EventRequest::ends_at()` in `openleadr-wire` with 13 unit tests, and SQL-side filtering in `data_source/postgres/event.rs` with 3 sqlx tests including a pagination regression. | Upstream has no `active` query param at all. Without this the VTN fetches every event and filters in Rust *after* `OFFSET/LIMIT`, so `?active=` plus pagination silently returns short or wrong pages. Under 3.1 `ends_at()` also resolves the three ways an event can declare its end — see `design.md` D9 (longest wins). |
| **P-2** | `45c1d83` | **Report cascade-delete.** `migrations/20260918000100` re-adds `report.event_id` with `ON DELETE CASCADE`. | Upstream leaves the FK without cascade, so `DELETE /events/{id}` fails with a foreign-key violation once any VEN has reported against that event. More pressing under 3.1, where `eventID` is a report's only object link. |
| **P-3** | `148fbfc` | **Cached Docker build.** Four-stage cargo-chef + BuildKit cache mounts in `vtn.Dockerfile`, and a fixed runtime `COPY` path. | Turns a ~50-minute cold rebuild into an incremental one. **Keeps upstream's `--features internal-oauth`**: that feature is not in the crate's defaults and gates `POST /auth/token` plus the whole `/users` tree, so dropping it yields a VTN that builds clean and then 404s every token request. |
| **P-5** | `a477199` | **`sqlx` optional in `openleadr-wire`.** The four identifier newtypes derive `sqlx::Type` behind a non-default `sqlx` feature; `openleadr-vtn` enables it. | The wire crate is for encoding and decoding OpenADR messages, but depended unconditionally on sqlx with postgres + tls-rustls, so any client wanting the types pulled in a Postgres driver and a TLS stack. Verified again 2026-09-20, now first-hand: the VEN takes `openleadr-wire` as a git dependency (branch 045) and `cargo tree` reports **zero** sqlx in its graph. This patch is what makes that possible -- without it every VEN image would carry a Postgres driver and a TLS stack to parse JSON. **Good upstream PR candidate** once tested. |

`8486be5` on top of these is the regenerated sqlx offline cache plus `cargo fmt` — an artefact of
P-1, not a patch in its own right.

## Retired

| # | What | Why it could go |
|---|---|---|
| **P-4** | VEN_NAME target reconstruction and stripping (upstream PRs #372, #374) | **Superseded by an upstream implementation, verified.** 3.1 does target hiding natively in `retrieve_all_with_client_id`, citing the spec in its own code: *"Target hiding: for program and event objects, a VTN will only include requested targets in a response"* (3.1.1 Definition.md). It filters on `e.targets && $3 OR array_length(e.targets,1) IS NULL` and redacts with `intersection(&e.targets, &ven_targets)`. Confirmed live on 2026-09-19: a program targeting `["ven-1","ven-2"]` shows `['ven-1']` to ven-1 and `['ven-2']` to ven-2, while the `read_all` business view sees both. Upstream's own tests cover all five properties our patch asserted, so porting ours would only have duplicated them. |

## Upstreaming

Per the project rule, upstream PRs are only opened once the code is fully tested and ready for
upstream CI. P-5 is the cleanest candidate (small, self-contained, no lab-specific behaviour).
P-1 and P-2 are plausible but carry lab-shaped decisions worth discussing first. P-3 is a
build-time preference and probably stays ours.
