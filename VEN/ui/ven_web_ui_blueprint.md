# VEN WebUI Blueprint (React + MUI)

## Purpose of this Document

This document is a **complete blueprint** for designing, building, deploying, and operating a Web User Interface (WebUI) for Virtual End Nodes (VENs) implemented in the OpenADR ecosystem.

It is intended for:
- Human engineers
- AI agents generating code or infrastructure
- Operators deploying VEN fleets

It consolidates all design decisions, architecture, functionality, configurations, and implementation details derived from the VEN design defined previously.

The WebUI consumes the **VEN Container API** — it does NOT talk directly to the VTN.

---

# 1. System Context

## Deployment Environment

- Hardware: Raspberry Pi 4
- VEN instances: 3–5 containers
- VTN: openleadr‑rs VTN running locally
- Database: PostgreSQL (VTN only)
- Network: Internal Docker network + optional LAN exposure

## WebUI Role

The WebUI provides:
- Observability into VEN state
- Visualization of programs and events
- Telemetry display
- Manual sensor injection (optional)

The WebUI does **not**:
- Store credentials
- Communicate with VTN directly
- Control OAuth flows

---

# 2. Architectural Overview

```
Browser
   |
   v
VEN WebUI (React + MUI)
   |
   v
VEN Container API
   |
   v
VTN (openleadr‑rs)
```

Key rule:

**All browser communication terminates at the VEN API.**

---

# 3. Technology Stack

## Core
- React 18
- TypeScript
- Vite (build tool)
- Material UI (MUI)

## Supporting Libraries
- react-router-dom → routing
- fetch → HTTP client
- Optional: react-json-view → JSON inspector

## Rationale

| Decision | Reason |
|---------|--------|
| React | Component ecosystem |
| MUI | Enterprise UI patterns |
| TypeScript | Schema alignment |
| Vite | Fast build on Pi |

---

# 4. Supported VEN API Endpoints

The WebUI is built strictly around the VEN API.

| Endpoint | Purpose |
|---------|---------|
| GET /health | Service status |
| GET /programs | Available programs |
| GET /events | Received events |
| GET /sensors | Latest telemetry |
| POST /sensors | Inject telemetry |

---

# 5. Functional Scope

The WebUI implements **all functionality possible** given the above API.

## 5.1 Health Monitoring

Displays:
- Online/offline state
- Polling errors

Refresh interval: 10s

---

## 5.2 Programs Visualization

Displays:
- Program list
- Program IDs
- Optional names

Features:
- Search filter
- Copy ID
- Quick preview on dashboard

---

## 5.3 Events Visualization

Displays:
- Event ID
- Program association
- Status
- Creation time

Features:
- Status filters
- Program filters
- Text search
- Sortable table
- Raw payload viewer
- JSON export

---

## 5.4 Sensor Telemetry

Displays:
- Power (W)
- Temperature (°C)
- Voltage (V)
- Timestamp

Features:
- Raw JSON viewer
- Optional chart history (client-side buffer)

---

## 5.5 Sensor Injection (Optional)

Allows manual POST:
- power_w
- temperature_c
- voltage_v
- raw JSON

Use cases:
- Testing reports
- Debugging telemetry

---

# 6. Multi‑VEN Support

The WebUI supports switching between VENs.

## Mechanism
Dropdown selector of base URLs.

Example:

```
VEN1 → http://raspberrypi:8081
VEN2 → http://raspberrypi:8082
VEN3 → http://raspberrypi:8083
```

No credential handling required.

---

# 7. UI Layout

## 7.1 App Shell

Components:
- AppBar
- VEN selector
- Refresh button
- Auto-refresh toggle
- Navigation tabs

---

## 7.2 Pages

### Dashboard
Cards:
- Health
- Programs count
- Events count
- Sensor snapshot

### Programs
- Searchable list
- ID + name

### Events
- Filterable table
- Status chips
- Detail dialog

### Sensors
- Snapshot card
- Raw viewer

---

# 8. Component Architecture

```
App
 ├─ VenSelector
 ├─ StatusChip
 ├─ NavigationTabs
 ├─ DashboardPage
 ├─ ProgramsPage
 ├─ EventsPage
 ├─ SensorsPage
 ├─ JsonDialog
 └─ ErrorSnackbar
```

---

# 9. Data Models

## Program

```
{
  id: string
  name?: string
}
```

## Event

```
{
  id: string
  program_id?: string
  status?: string
  created_at?: string
  raw: any
}
```

## SensorSnapshot

```
{
  id: string
  ts: string
  power_w?: number
  temperature_c?: number
  voltage_v?: number
  raw: any
}
```

---

# 10. Polling Model

| Data | Interval |
|------|-----------|
| Health | 10s |
| Events | 30s |
| Programs | 5m |
| Sensors | 10s |

Configurable via UI toggle.

---

# 11. State Management

Local React state only.

Stores:
- programs[]
- events[]
- sensor snapshot
- health status
- timestamps

No backend persistence.

---

# 12. Security Model

## Principles

- No OAuth in browser
- No secrets stored
- Only VEN API access

## Network

- Same Docker network or LAN
- Optional reverse proxy

---

# 13. Deployment Model

## Option A — Single UI container

```
ven-ui:3000
```
Switch between VENs.

## Option B — One UI per VEN

```
ven1-ui → ven1
ven2-ui → ven2
```

Option A recommended.

---

# 14. Build Pipeline

```
npm create vite
npm install
npm run build
serve dist/
```

Containerized via Nginx or Node.

---

# 15. Observability Features

- Last refresh timestamps
- Offline detection
- Error snackbars
- Empty-state messaging

---

# 16. Limitations (API‑Bound)

Not possible without new endpoints:

- Program enrollment
- Event opt-in/out
- Report subscription management
- Report history
- Device control

---

# 17. Future Extensions

## UI
- Charts (Recharts)
- Event timelines
- Load reduction graphs

## API
- /reports
- /opt
- /enroll

## Security
- TLS
- Auth proxy

---

# 18. Configuration Reference

## Default VEN URLs

```
http://raspberrypi:8081
http://raspberrypi:8082
http://raspberrypi:8083
```

---

# 19. Build Checklist

- [ ] Scaffold React app
- [ ] Install MUI
- [ ] Implement API client
- [ ] Implement polling hooks
- [ ] Build dashboard
- [ ] Build programs page
- [ ] Build events page
- [ ] Build sensors page
- [ ] Add VEN selector
- [ ] Add error handling
- [ ] Containerize

---

# 20. Operational Runbook

Startup order:
1. VTN
2. VEN containers
3. WebUI

Health validation:
- /health reachable
- Events polling
- Sensors updating

---

# 21. Sources

Derived from system design and VEN architecture defined in this conversation, aligned with:

- openleadr-rs repository
- openleadr-client crate documentation
- OpenADR 2.0b specification concepts

Reference URLs:

https://github.com/OpenLEADR/openleadr-rs
https://docs.rs/openleadr-client
https://www.openadr.org

---

# 22. Summary

The VEN WebUI provides:
- Full visibility into VEN state
- Event and program inspection
- Telemetry visualization
- Multi‑VEN switching
- Zero credential exposure

It is:
- Lightweight
- Pi-friendly
- Container deployable
- Extensible toward production

---

END OF DOCUMENT

