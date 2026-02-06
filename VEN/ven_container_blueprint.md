# VEN Container Blueprint (OpenADR / openleadr-rs)

## Purpose of this Document

This document is a **complete blueprint** to design, build, deploy, and operate a minimal but production‑extendable **Virtual End Node (VEN)** container application compatible with an OpenADR VTN (specifically aligned with the openleadr‑rs ecosystem).

It is written for:
- Humans (engineers, operators)
- AI agents (code generation, automation, infra tooling)

It consolidates:
- Architectural decisions
- Security posture
- Runtime configs
- Container layout
- API design
- Reporting model
- State handling
- Scaling to multiple VENs
- Known gaps / TODOs

---

# 1. System Context

## Deployment Target
- Hardware: Raspberry Pi 4
- VEN instances: 3–5
- VTN: Running locally (Docker) with PostgreSQL
- Network: Internal Docker network (LAN only)

## Load Characteristics
- Reports: Every 5 minutes per VEN
- Event polling: ~30 seconds
- Programs polling: ~5 minutes

This is a **low write / low concurrency** scenario.

---

# 2. Architectural Overview

Each VEN runs as its own container.

```
+----------------------+
|      Web UI          |
| (Browser / Tablet)  |
+----------+-----------+
           |
           v
+----------------------+
|   VEN Container      |
|----------------------|
| Local REST API       |
| Event Poller         |
| Program Poller       |
| Sensor Sampler       |
| Report Sender        |
| OAuth Client         |
| In‑Memory State      |
+----------+-----------+
           |
           v
+----------------------+
|        VTN           |
| (openleadr‑rs)       |
| PostgreSQL backend   |
+----------------------+
```

---

# 3. Design Decisions

## 3.1 One VEN per container
Rationale:
- Isolation of credentials
- Easier scaling
- Fault containment
- Matches OpenADR identity hookup

## 3.2 No database inside VEN
State is:
- Ephemeral in memory
- Optionally persisted to JSON file

Reason:
- Reports are near‑real‑time
- Low risk if container restarts
- Simplifies footprint on Pi

## 3.3 Minimal security posture
- OAuth client credentials required by VTN
- Secrets stored in container env
- No direct browser → VTN calls
- Browser calls VEN API only

## 3.4 Rust implementation
Reasons:
- Native compatibility with openleadr‑rs
- Low resource footprint on Pi
- Strong typing for report schemas

---

# 4. Functional Components

## 4.1 OAuth Client
Purpose:
- Acquire bearer token from VTN
- Use client_credentials grant

Inputs:
- CLIENT_ID
- CLIENT_SECRET
- VTN_BASE_URL

Outputs:
- Access token

Used by:
- Event poller
- Program poller
- Report sender

---

## 4.2 Event Poller
Purpose:
- Retrieve events assigned to VEN

Frequency:
- 30 seconds (configurable)

Output:
- Stored in local state
- Exposed via API

Future extensions:
- Event acknowledgment
- OptIn / OptOut handling
- Control signal dispatch

---

## 4.3 Program Poller
Purpose:
- Retrieve programs available to VEN

Frequency:
- 5 minutes

Usage:
- UI display
- Report alignment

---

## 4.4 Sensor Sampler
Purpose:
- Produce telemetry data

Initial implementation:
- Simulated values

Future integrations:
- MQTT
- Modbus
- GPIO / I2C
- Smart meters

Sampling interval:
- 10 seconds internal
- Aggregated into 5‑minute reports

---

## 4.5 Report Sender
Purpose:
- Send telemetry reports to VTN

Report type:
- telemetry_usage

Measurement:
- powerReal (kW)

Interval:
- 5 minutes

Data includes:
- Timestamp
- Resource ID
- Measurement value

---

## 4.6 Local REST API
Purpose:
- Provide data to Web UI
- Avoid exposing VTN credentials

Endpoints:

### GET /health
Returns service status.

### GET /events
Returns received events.
Query:
- limit

### GET /programs
Returns available programs.

### GET /sensors
Returns latest sensor snapshot.

### POST /sensors
Allows external sensor push.

---

# 5. Data Models

## 5.1 Program

Fields:
- id
- name

## 5.2 Event

Fields:
- id
- program_id
- status
- created_at
- raw payload

## 5.3 Sensor Snapshot

Fields:
- timestamp
- power_w
- temperature_c
- voltage_v
- raw

---

# 6. State Management

## In‑Memory Stores
- Vec<Event>
- Vec<Program>
- Latest SensorSnapshot

Retention:
- Last 500 events

## Optional Persistence
Path:
```
/data/state.json
```

Use cases:
- Restart recovery
- Debugging

---

# 7. Reporting Model

## Purpose of Reports
Reports communicate VEN → VTN telemetry:
- Power demand
- Energy usage
- Forecasts
- Capacity

Used for:
- Event verification
- Grid balancing
- Settlement

## Report Lifecycle
1. VEN advertises capability
2. VTN subscribes
3. VEN streams data

## Example Telemetry Payload

```json
{
  "report_name": "telemetry_usage",
  "resource_id": "ven-1-meter",
  "measurement": "powerReal",
  "unit": "kW",
  "interval": "PT5M",
  "data": [
    {"ts": "2026-02-05T12:00:00Z", "value": 42.1}
  ]
}
```

---

# 8. Container Configuration

## Environment Variables

| Variable | Purpose |
|---------|---------|
| LISTEN_ADDR | API bind |
| VTN_BASE_URL | VTN endpoint |
| CLIENT_ID | OAuth client |
| CLIENT_SECRET | OAuth secret |
| VEN_NAME | Identity |
| POLL_EVENTS_SECS | Event poll |
| POLL_PROGRAMS_SECS | Program poll |
| PERSIST_PATH | State file |

---

# 9. Docker Design

## Dockerfile
- Multi‑stage Rust build
- Slim Debian runtime
- Exposes port 8080

## Volumes
Per VEN:
```
venX-data:/data
```

---

# 10. Multi‑VEN Deployment

Example ports:

| VEN | Port |
|-----|------|
| ven1 | 8081 |
| ven2 | 8082 |
| ven3 | 8083 |

All connect to:
```
http://vtn:3000
```

---

# 11. Security Model

Minimal but sufficient:

- OAuth client credentials
- One credential per VEN
- Private Docker network
- No browser secrets

Future hardening:
- TLS
- Secret manager
- Network segmentation

---

# 12. Web UI Integration

The UI calls VEN APIs only.

Possible dashboards:
- Event timeline
- Program enrollment
- Load telemetry graph

No direct VTN access required.

---

# 13. Missing / Future Work

## Protocol completeness
- Event opt responses
- Report subscriptions
- Report registration negotiation

## Reliability
- Offline buffering
- Retry queues
- Delivery guarantees

## Observability
- Metrics
- Prometheus
- Structured logs

## Control integration
- Device actuation
- Load shedding automation

## Scaling
- Multi‑host VEN fleets
- Remote VTNs

---

# 14. Build Checklist

- [ ] Implement OAuth client
- [ ] Implement pollers
- [ ] Implement sensor sampler
- [ ] Implement report sender
- [ ] Implement REST API
- [ ] Add persistence
- [ ] Containerize
- [ ] Deploy 3–5 instances
- [ ] Register clients in VTN

---

# 15. Operational Runbook

Startup order:
1. PostgreSQL
2. VTN
3. VEN containers
4. Web UI

Health checks:
- /health endpoint
- Token acquisition
- Event polling success

---

# 16. Summary

This VEN blueprint provides:
- Minimal footprint
- Multi‑instance scaling
- Web UI observability
- Telemetry reporting
- Event visibility

It is intentionally:
- Stateless‑leaning
- Container‑native
- Extensible toward production

---

END OF DOCUMENT

