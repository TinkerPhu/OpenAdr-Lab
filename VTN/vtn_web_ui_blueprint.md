# VTN WebUI Blueprint

**Purpose:**
This document is a full implementation blueprint for building a Virtual Top Node (VTN) Web User Interface for an OpenADR 3 environment.

It is intended for:
- Human developers
- AI coding agents
- System architects

The document consolidates architectural decisions, UI structure, data flows, controls, and future‑proofing considerations derived from system design discussions.

---

# 1. System Context

## 1.1 Deployment Context

- Platform: Raspberry Pi 4
- VTN: openleadr‑rs implementation
- Database: PostgreSQL (VTN persistence)
- VENs: 3–5 instances, containerized
- Reports cadence: every 5 minutes

The WebUI operates as an operational control and observability layer for the VTN.

---

# 2. Architectural Positioning

## 2.1 UI Role

The WebUI is:
- A visualization layer
- A control interface
- An event orchestration console

The WebUI is **not**:
- A persistence authority
- A telemetry ingestion engine
- An OAuth credential holder (handled outside UI)

---

# 3. Data Ownership and Persistence

## 3.1 Persisted in VTN

The VTN database (PostgreSQL) persists:

- Programs
- VEN registrations
- Resources
- Events
- Report definitions
- Report subscriptions

## 3.2 Report Data Persistence

Open questions / implementation‑dependent:

- Whether time‑series telemetry datapoints are retained long‑term in VTN
- Whether range queries are supported

### Decision

Phase 1:
- UI assumes VTN provides report metadata + latest datapoints
- No UI persistence

Phase 2 (optional):
- Add telemetry store if historical analytics required

---

# 4. Real‑Time Data Strategy

VTN implementation does not support:
- Webhooks
- Subscriptions
- Push updates

### Decision

UI must use polling:

| Data Type | Poll Interval |
|-----------|---------------|
| Dashboard metrics | 5 s |
| Events list | 5–10 s |
| VEN details | 10 s |
| Programs | 30–60 s |
| Reports | 30 s |

---

# 5. Security Model

## 5.1 OAuth

VTN requires bearer tokens via OAuth client credentials.

### Decision

- UI does NOT handle OAuth
- Tokens handled server‑side (outside scope of this doc)

## 5.2 UI Exposure

- LAN‑only deployment acceptable
- TLS optional in lab phase
- No secrets in browser

---

# 6. Technology Stack

## 6.1 Frontend

- React
- TypeScript
- Vite
- MUI (Material UI)
- MUI X DataGrid
- MUI Date/Time Pickers
- TanStack Query (React Query)
- React Router

## 6.2 Visualization

- Line charts for telemetry
- Sparkline mini‑charts

Chart library options:
- Recharts
- Nivo
- Chart.js

---

# 7. Navigation Structure

```
Dashboard
Programs
VENs
Events
Event Composer
Reports (future)
Settings (optional)
```

---

# 8. Page Specifications

---

## 8.1 Dashboard

### Purpose
Provide real‑time operational awareness.

### Components

**Summary Cards**
- VTN Health
- Total Programs
- Registered VENs
- Active Events
- Upcoming Events

**Tables**
- Recent Events
- VEN Activity

**Indicators**
- Last report timestamp per VEN
- Polling freshness indicator

---

## 8.2 Programs Page

### Purpose
Manage demand response programs.

### Views

**List View**
Columns:
- Program ID
- Name
- Status
- Created timestamp

**Detail View**
Displays:
- Associated events
- Assigned VENs/resources
- Metadata

### Controls
- Create program (optional)
- Edit metadata (optional)
- Launch Event Composer

---

## 8.3 VENs Page

### Purpose
Monitor participating endpoints.

### List Columns
- VEN ID / Name
- Status
- Resource count
- Last activity
- Last report timestamp

### Detail View
Sections:
- Resources
- Assigned programs
- Active events
- Report capabilities
- Latest telemetry snapshot
- Raw JSON inspector

---

## 8.4 Events Page

### Purpose
Track demand response actions.

### Filters
- Status
- Program
- VEN
- Time range

### List Columns
- Event ID
- Program
- Status
- Start time
- Duration
- Targets

### Detail View
Displays:
- Timeline
- Targets
- Payload
- Status transitions
- Raw JSON

---

## 8.5 Event Composer

### Purpose
Create and dispatch events.

### Workflow Wizard

#### Step 1 — Program Selection
- Dropdown

#### Step 2 — Target Selection
- Multi‑select VENs
- Optional resource selection

#### Step 3 — Timing
- Start now / schedule
- Duration
- Ramp/recovery (optional)

#### Step 4 — Payload
Modes:
- Form editor
- JSON editor

#### Step 5 — Review
- Final JSON preview

#### Step 6 — Dispatch
- Submit event
- Confirmation
- Link to event detail

---

## 8.6 Reports Page (Future)

### Sections

**Report Definitions**
- Type
- Resource
- Interval

**Telemetry Explorer**
- VEN selector
- Resource selector
- Metric selector
- Time range picker
- Line chart

**Latest Values Panel**
- Last value
- Last timestamp
- Min/max

---

# 9. UI Components

## Layout
- AppShell
- Responsive Drawer
- Top AppBar
- Breadcrumbs

## Data
- DataTable (DataGrid wrapper)
- StatusChip
- JsonViewer
- PollingIndicator

## Event Composer
- TargetSelector
- TimingForm
- PayloadForm
- JsonEditorDialog

---

# 10. State Management

Using TanStack Query.

### Query Keys

```
health
programs
program
vens
ven
events
event
reports
telemetry
```

---

# 11. Data Models (UI‑side)

## EventDraft

```ts
type EventDraft = {
  programId: string;
  targets: { venIds: string[]; resourceIds?: string[] };
  timing: { start: string; durationMinutes: number };
  payload: Record<string, any>;
};
```

---

# 12. Configuration

## Environment

```
VITE_API_BASE_URL=http://ui-backend:3000
POLL_INTERVAL_DASHBOARD=5000
POLL_INTERVAL_EVENTS=8000
```

---

# 13. Deployment

## Containerization

- One UI container
- Served via Nginx or Node

## Networking

- Same Docker network as VTN
- LAN exposure optional

---

# 14. Performance Considerations

- Use pagination for events
- Lazy load detail pages
- Debounce filters
- Cache static resources

---

# 15. Accessibility & UX

- Dark/light theme
- Status color coding
- Keyboard navigation
- Responsive layout

---

# 16. Missing / Future Enhancements

- Historical telemetry persistence
- Event performance analytics
- Settlement calculations
- User authentication / RBAC
- Multi‑VTN federation view
- Alerting / notifications
- Export (CSV/JSON)

---

# 17. Source References

(Informational, non‑binding to implementation)

- OpenADR 3 specification overview
- OpenLEADR‑RS repository documentation
- OpenADR 3 introduction webinars
- OAuth 2.0 client credentials flow

---

# 18. Implementation Readiness Checklist

- [ ] VTN reachable
- [ ] OAuth configured
- [ ] Programs endpoint verified
- [ ] Events endpoint verified
- [ ] VEN endpoint verified
- [ ] Polling tested
- [ ] Event creation tested
- [ ] Multi‑VEN targeting tested
- [ ] Report metadata accessible

---

# End of Blueprint

