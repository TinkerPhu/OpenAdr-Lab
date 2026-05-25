# Interface Reference

> Source of truth: `VEN/src/routes/mod.rs` and `VTN/bff/src/main.rs`.
> OpenADR wire types: `openleadr-rs/openleadr-wire/src/` (git submodule).
> Update this file whenever routes are added or removed.

---

## VEN REST API

Served by each VEN instance (ven-1 :8081, ven-2 :8082, ven-3 :8083 in docker-compose).
No authentication. CORS: any origin.

### System & observability

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/health` | Liveness check |
| GET | `/metrics` | Prometheus metrics |
| GET | `/trace/events` | Recent OpenADR event trace |
| GET | `/trace/history` | Historical event trace |

### OpenADR data (polled from VTN)

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/events` | OpenADR events visible to this VEN |
| GET | `/programs` | OpenADR programs this VEN participates in |
| GET | `/sensors` | Current sensor readings |
| POST | `/sensors` | Inject sensor data |

### Reports

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/reports` | VEN reports |
| POST | `/reports` | Create report |
| PUT | `/reports/:id` | Update report |

### Simulation control

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/sim` | Full simulation state snapshot |
| GET | `/sim/schema` | JSON schema for sim state |
| POST | `/sim/reset/:asset_id` | Reset one asset to initial state |
| PUT | `/sim/config/battery` | Update battery physics config |
| GET | `/sim/inject` | Current override injection state |
| POST | `/sim/inject` | Inject override values |
| POST | `/sim/inject/reset` | Clear all injections |
| POST | `/plan/trigger` | Manually trigger MILP planner |

### HEMS — plan, tariffs & grid

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/plan` | Current MILP plan |
| PUT | `/plan/objective` | Set optimisation objective |
| GET | `/plan/events` | Events included in plan |
| GET | `/tariffs` | Active tariff schedule |
| GET | `/capacity` | Grid capacity limits |
| GET | `/obligations` | OpenADR obligations affecting plan |
| GET | `/ledger` | Energy/cost ledger |
| GET | `/flexibility` | Flexibility envelope |

### HEMS — user overrides

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/user-requests` | List user override requests |
| POST | `/user-requests` | Create user request |
| DELETE | `/user-requests/:id` | Remove user request |
| GET | `/ev-session` | Current EV session |
| POST | `/ev-session` | Start EV session |
| DELETE | `/ev-session` | End EV session |
| GET | `/ev-settings` | EV charging settings |
| PUT | `/ev-settings` | Update EV settings |
| GET | `/heater-target` | Heater target temperature |
| POST | `/heater-target` | Set heater target |
| DELETE | `/heater-target` | Clear heater target |
| GET | `/shiftable-loads` | List shiftable loads |
| POST | `/shiftable-loads` | Register shiftable load |
| DELETE | `/shiftable-loads/:id` | Remove shiftable load |
| GET | `/baseline-override` | Baseline override state |
| POST | `/baseline-override` | Set baseline override |
| DELETE | `/baseline-override` | Clear baseline override |

Valid `:asset_id` values: `battery`, `ev`, `heater`, `pv`, `base_load`

### Asset timelines & forecasts

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/timeline/all` | Timeline for all assets |
| GET | `/timeline/:asset_id` | Timeline for one asset |
| GET | `/forecast/:asset_id` | Forecast for one asset |
| GET | `/history/:asset_id` | Historical data for one asset |
| GET | `/capability/:asset_id` | Capability envelope for one asset |

---

## VTN BFF REST API

Served at `/api/*` prefix. Auth: OAuth 2.0 client credentials (two clients: `business`,
`ven-mgr`). Proxies to openleadr-rs VTN server.
Source: `VTN/bff/src/main.rs`.

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/health` | Liveness check |
| GET | `/api/programs` | List OpenADR programs |
| POST | `/api/programs` | Create program |
| PUT | `/api/programs/:id` | Update program |
| DELETE | `/api/programs/:id` | Delete program |
| GET | `/api/events` | List OpenADR events |
| POST | `/api/events` | Create event |
| PUT | `/api/events/:id` | Update event |
| DELETE | `/api/events/:id` | Delete event |
| GET | `/api/vens` | List registered VENs |
| DELETE | `/api/vens/:id` | Delete VEN registration |
| GET | `/api/reports` | List reports |
| DELETE | `/api/reports/:id` | Delete report |
| GET | `/api/metrics` | Prometheus metrics (BFF-level) |

---

## OpenADR 3 wire types

Defined in `openleadr-rs/openleadr-wire/src/` (git submodule).
All types serialize/deserialize with field names matching the OpenADR 3 specification
(DTO passthrough — no renaming across layers).

| Type | Purpose |
|------|---------|
| `Program` | DR programme definition (intervals, targets) |
| `Event` | DR event within a programme (payload, time window) |
| `Report` | VEN telemetry report sent to VTN |
| `Ven` | VEN registration record |
| `Interval` | Time-bounded segment of a programme or event |
| `ValDescriptor` | Describes the unit/type of a payload value |
| `EventPayload` | Typed payload carried by an event interval |
| `ReportPayload` | Typed payload carried by a report interval |

For full type definitions, read `openleadr-rs/openleadr-wire/src/`.
