# OpenADR 3 Raspberry Pi Lab
## Complete System Architecture & Design (Rewritten + Extended)

---

# 1. Purpose of This Document

This document defines the **complete architecture, deployment model, data flows, and implementation design** for a Raspberry Pi–hosted OpenADR 3 laboratory environment.

It preserves all previously defined details while:

- Adding missing OpenADR 3 lifecycle flows
- Clarifying VEN identity & provisioning
- Strengthening persistence and time handling
- Formalizing polling/backoff behavior
- Expanding security and observability
- Removing duplicated setup instructions
- Adding sequence diagrams and test strategy

The system is intended for:

- OpenADR 3 experimentation
- Multi‑VEN simulation
- Edge computing research
- Demand response prototyping

It is **not production‑grade** but follows production‑inspired patterns.

---

# 2. System Overview

## Hardware Platform

Single Raspberry Pi running:

- Linux (Raspberry Pi OS / Ubuntu Server)
- Docker + Docker Compose

The Pi hosts:

- One VTN stack
- Multiple VEN stacks
- Shared network and storage

---

# 3. High‑Level Architecture

```
+--------------------------------------------------+
|                 Raspberry Pi                     |
|                                                  |
|  Docker Network: openadr-net                     |
|                                                  |
|  +-------------------+                           |
|  |       VTN         |                           |
|  |-------------------|                           |
|  | VTN Server        |                           |
|  | Postgres DB       |                           |
|  | VTN UI + BFF      |                           |
|  +---------+---------+                           |
|            |                                     |
|  ----------- OpenADR 3 API ------------------    |
|            |                                     |
|  +---------+---------+                           |
|  |       VEN 1       |                           |
|  |-------------------|                           |
|  | Pollers           |                           |
|  | Sampler           |                           |
|  | State Store       |                           |
|  | VEN API           |                           |
|  | VEN UI            |                           |
|  +-------------------+                           |
|                                                  |
|  +-------------------+                           |
|  |       VEN 2       |   (same structure)         |
|  +-------------------+                           |
|                                                  |
+--------------------------------------------------+
```

---

# 4. Networking Model

All containers communicate over:

```
Docker bridge network: openadr-net
```

### Internal Name Resolution

| Service | Internal URL |
|--------|---------------|
| VTN API | http://vtn:3000 |
| Postgres | postgres:5432 |
| VEN1 API | ven1:8080 |

### Host Port Mapping

| Service | Container Port | Host Port | URL |
|--------|-----------------|-----------|-----|
| VTN UI/BFF | 3000 | 8080 | http://pi:8080 |
| VEN1 UI | 8080 | 8081 | http://pi:8081 |
| VEN2 UI | 8080 | 8082 | http://pi:8082 |

Browsers use host ports; containers use Docker DNS.

---

# 5. VTN Stack

## Components

### 5.1 VTN Server
Rust implementation (e.g., openleadr‑rs).

Responsibilities:

- OAuth2 authorization server
- OpenADR 3 API provider
- Program management
- Event lifecycle management
- Report ingestion

### 5.2 Database

PostgreSQL stores:

- Programs
- Events
- VEN registrations
- Resources
- Reports
- OAuth clients

Persistent Docker volume required.

### 5.3 VTN UI + BFF

Browser UI must not store OAuth secrets.

Pattern used:

```
Browser → BFF → OAuth → VTN API
```

BFF responsibilities:

- Token acquisition
- Token refresh
- API proxying
- Request correlation IDs

---

# 6. VEN Stack

Each VEN runs as an isolated container.

## Internal Modules

| Module | Responsibility |
|-------|----------------|
| Auth Client | OAuth token handling |
| Program Poller | Program discovery |
| Event Poller | Event retrieval |
| Sampler | Telemetry generation |
| State Store | Persistence |
| Report Client | Report submission |
| Local API | REST access |
| UI | Visualization |

---

# 7. VEN Identity & Provisioning

## Identity Model

Each VEN has:

- `ven_id` → Stable UUID
- `resource_id` → One per VEN (lab default)
- OAuth `client_id`
- OAuth `client_secret`

### ven_id Strategy

Options:

- Pre‑generated UUID in config
- Derived from container name
- Seeded in DB fixtures

Recommended: static UUID stored in `.env`.

### Provisioning Method

Lab uses **pre‑seeded OAuth clients** in VTN DB.

Future option: admin UI provisioning.

---

# 8. OpenADR 3 Lifecycle Flows

## 8.1 VEN Startup Flow

```
VEN boots
  ↓
Load config
  ↓
Request OAuth token
  ↓
Discover programs
  ↓
Register / confirm ven_id
  ↓
Start pollers
```

## 8.2 Event Distribution Flow

```
Operator creates event (UI)
  ↓
BFF proxies to VTN
  ↓
VTN stores event
  ↓
VEN polls /events
  ↓
VEN evaluates interval
  ↓
VEN acknowledges / opts
```

## 8.3 Event Update / Cancel Flow

```
Event modified in VTN
  ↓
Version incremented
  ↓
VEN next poll detects change
  ↓
State updated
```

## 8.4 Token Refresh Flow

```
Token nearing expiry
  ↓
VEN refreshes
  ↓
Retry on failure
```

---

# 9. Polling Strategy

Polling is used instead of webhooks.

## Intervals

| Endpoint | Interval |
|---------|-----------|
| Programs | 5–10 min |
| Events | 30–60 sec |
| Reports submit | 60 sec |

## Jitter

Add ±20% randomness.

## Backoff

On failure:

```
1m → 2m → 4m → 8m → max 15m
```

## Rate Protection

Minimum poll floor enforced.

---

# 10. Event State Persistence

VEN stores:

- Last event IDs seen
- Event versions
- Opt status
- Report cursors

## Storage Options

### Default

```
/data/state.json
```

### Write Safety

- Write temp file
- Atomic rename

### Versioning

Include schema version field.

Optional future: SQLite.

---

# 11. Reporting Model

## Telemetry Example

```json
{
  "timestamp": "2026-01-01T12:00:00Z",
  "power_kw": 4.2,
  "voltage_v": 230,
  "temperature_c": 21.5
}
```

## OpenADR Mapping

Reports align with:

- Report registration
- Report requests
- Periodic submissions

## Buffering Policy

If VTN unavailable:

- Buffer locally
- Retry with backoff
- Drop oldest if storage exceeded

---

# 12. Time & Clock Management

OpenADR is time‑sensitive.

Requirements:

- NTP or chrony enabled
- All timestamps UTC
- ISO‑8601 formatting

## Skew Handling

If clock drift detected:

- Log warning
- Avoid executing past events

---

# 13. Security Model (Lab Scope)

## Secrets Handling

- Stored in `.env`
- Not committed to Git
- Option: Docker secrets

## Network Exposure

Default:

- Bind to localhost
- No WAN exposure

## TLS

Optional for lab.

If enabled:

- Reverse proxy (Caddy / Traefik)

---

# 14. Observability

## Logging

Structured JSON logs.

Fields:

- ven_name
- event_id
- program_id
- request_id
- latency_ms

## Metrics (Optional)

Prometheus text endpoint:

- poll_success_total
- poll_errors_total
- reports_sent_total

## Correlation IDs

Propagated:

```
UI → BFF → VTN → VEN
```

---

# 15. Data Contracts & Validation

Define DTOs for:

- Events
- Programs
- Reports

Validation layers:

- VTN inbound schema validation
- VEN inbound validation

---

# 16. Resource Constraints (Pi Planning)

Estimated footprint:

| Service | RAM |
|--------|-----|
| Postgres | 150–300 MB |
| VTN Rust | 50–120 MB |
| Each VEN | 40–80 MB |

Pi 4GB supports ~5 VENs comfortably.

---

# 17. TLS (Optional Enhancement)

Optional TLS termination via reverse proxy (Caddy / Traefik) for encrypted access to individual services.

---

# 18. Deployment Structure

```
openadr-lab/
 ├─ docker-compose.yml
 ├─ .env
 ├─ vtn/
 ├─ ven1/
 ├─ ven2/
 └─ data/
```

Volumes persist DB + VEN state.

---

# 19. Implementation Order

1. Bring up Postgres
2. Start VTN server
3. Configure OAuth clients
4. Deploy BFF + UI
5. Start single VEN
6. Validate polling
7. Add reporting
8. Scale VENs

---

# 20. Test Strategy

## Smoke Tests

- VEN obtains token
- Programs retrieved
- Events received

## Integration Tests

- Multi‑VEN polling
- Event distribution
- Report ingestion

## Failure Tests

- Restart VTN
- Restart VEN
- Kill DB

Validate recovery.

---

# 21. Failure & Recovery Behavior

| Failure | Behavior |
|--------|-----------|
| Token expired | Refresh |
| VTN down | Backoff |
| DB down | Retry |
| Clock skew | Warn |

---

# 22. Future Extensions

- Webhook/event push
- Real device telemetry
- Kubernetes deployment
- Multi‑resource VENs

---

# 23. Conclusion

This lab architecture provides:

- Realistic OpenADR 3 flows
- Multi‑VEN simulation
- Edge deployment model
- Observable, testable behavior

It balances:

- Simplicity for Raspberry Pi
- Fidelity to OpenADR 3
- Extensibility for research

---

**End of Document**

