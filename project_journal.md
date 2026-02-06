# OpenADR Lab — Project Journal

---

## Project Overview

This project builds a **Raspberry Pi 4–hosted OpenADR 3 lab environment** for demand response experimentation. The Pi runs Docker and hosts a VTN (Virtual Top Node) stack, multiple VEN (Virtual End Node) containers, and web UIs — all communicating over a shared Docker bridge network (`openadr-net`).

The system design is defined in `open_adr_3_raspberry_pi_lab_complete_system_design.md`.

---

## What Has Been Done

### 1. VTN Stack — Deployed and Running

**Status: COMPLETE**

The VTN stack is live on Pi4-Server with two healthy containers:

| Container | Image | Status | Port |
|-----------|-------|--------|------|
| `vtn-vtn-1` | openleadr-rs (built from source) | healthy | 3000 |
| `vtn-vtn-db-1` | postgres:16-alpine | healthy | 5432 |

**What was done:**
- Created `VTN/docker-compose.yml` with services `vtn-db` (PostgreSQL) and `vtn` (openleadr-rs)
- Cloned `openleadr-rs` at project root (not inside VTN/); docker-compose references `../openleadr-rs`
- Built VTN from source inside Docker (~25 min on Pi4 ARM64, cached afterwards)
- Confirmed VTN auto-runs SQLx migrations at startup (15 tables created)
- Loaded test credential fixtures (5 users: any-business, ven-manager, user-manager, business-1, ven-1)

**Confirmed VTN behavior:**
- Health endpoint: `GET /health` returns `OK`
- Token endpoint: `POST /auth/token` (not `/oauth/token`)
- Token expiry: 2,592,000 seconds (30 days)
- Role-based access: `any-business` can access /programs, /events but NOT /vens (403)
- `ven-manager` credentials required for VEN management

### 2. Step-by-Step Setup Guide — Written and Verified

**Status: COMPLETE**

`VTN/vtn_setup_from_blog_step_by_step.md` was updated with all confirmed findings from the actual deployment. Every section was verified against the running system — no assumptions remain.

### 3. Infrastructure — Git + Deployment Pipeline

**Status: COMPLETE**

- Repository on GitHub, Pi4-Server pulls via HTTPS with PAT
- `ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull"` works
- `.gitignore` excludes `openleadr-rs/` (cloned third-party repo)

### 4. Design Documents — All Written

**Status: COMPLETE (design phase)**

| Document | Location | Purpose |
|----------|----------|---------|
| System Design | `open_adr_3_raspberry_pi_lab_complete_system_design.md` | Master architecture |
| VTN Setup Guide | `VTN/vtn_setup_from_blog_step_by_step.md` | Deployment instructions |
| VTN BFF Blueprint | `VTN/vtn_rust_bff_blueprint.md` | Rust backend-for-frontend |
| VTN Web UI Blueprint | `VTN/vtn_web_ui_blueprint.md` | React + MUI operator console |
| VEN Container Blueprint | `VEN/ven_container_blueprint.md` | Rust VEN application |
| VEN Web UI Blueprint | `VEN/ui/ven_web_ui_blueprint.md` | React + MUI VEN dashboard |
| VTN DTO Examples | `VTN/DTO examples/` | JSON/TS sample payloads |

### 5. VEN Application — Scaffolded (Not Complete)

**Status: IN PROGRESS — code scaffolded, not building/running yet**

Rust source files exist:
- `VEN/Cargo.toml` — dependencies defined (axum, tokio, reqwest, chrono, etc.)
- `VEN/src/config.rs` — env-based config loader (complete)
- `VEN/src/main.rs` — exists
- `VEN/src/models.rs` — exists
- `VEN/src/state.rs` — exists
- `VEN/src/vtn.rs` — exists

Local `cargo` build started (target/ artifacts present) but **no Dockerfile, no docker-compose, not deployed to Pi yet**.

### 6. VEN Web UI — Scaffolded (Not Complete)

**Status: IN PROGRESS — components started, not buildable yet**

React/TypeScript files exist:
- `VEN/ui/src/App.tsx`
- `VEN/ui/src/api/client.ts`
- `VEN/ui/src/hooks/usePoll.ts`
- `VEN/ui/src/pages/Dashboard.tsx`, `Events.tsx`, `Programs.tsx`, `Sensors.tsx`
- `VEN/ui/src/components/JsonDialog.tsx`
- `VEN/ui/src/datamodel.ts`

**No `package.json`, no `vite.config.ts`, no Dockerfile** — not buildable yet.

### 7. VTN BFF + VTN Web UI — Not Started

**Status: NOT STARTED — blueprints written, no code**

---

## What To Do Next

Based on the system design's implementation order (Section 19) and current state:

### Phase 1: Complete the VEN Application (Priority: HIGH)

This is the next logical step. The VTN is running but there's nothing talking to it.

**Tasks:**
1. **Finish the VEN Rust application** — complete `main.rs`, `models.rs`, `state.rs`, `vtn.rs` to implement:
   - OAuth client (client_credentials grant against `/auth/token`)
   - Program poller (every 5 min)
   - Event poller (every 30 sec)
   - Sensor sampler (simulated telemetry)
   - Local REST API (GET /health, /events, /programs, /sensors)
2. **Create VEN Dockerfile** — multi-stage Rust build (similar to vtn.Dockerfile)
3. **Create VEN docker-compose.yml** — initially for 1 VEN, then scale to 3
4. **Register VEN OAuth clients in VTN DB** — the test fixtures only have `ven-1`; add `ven-2`, `ven-3`
5. **Deploy to Pi4-Server** — build and run alongside VTN stack
6. **Validate end-to-end** — VEN obtains token, discovers programs, polls events

### Phase 2: VEN Web UI (Priority: MEDIUM)

Once VENs are running and their APIs are accessible:

1. **Scaffold the React app properly** — `package.json`, Vite config, MUI
2. **Complete the UI pages** — Dashboard, Events, Programs, Sensors
3. **Add multi-VEN selector** — switch between VEN1/VEN2/VEN3
4. **Containerize** — Nginx-based Docker image
5. **Deploy to Pi4-Server**

### Phase 3: Create Programs and Events via VTN API (Priority: MEDIUM)

The VTN is running but empty — no programs or events exist yet.

1. **Create test programs** via curl/API (using `any-business` credentials)
2. **Create test events** targeted at VENs
3. **Verify VEN polls pick up events**
4. **Verify event distribution across multiple VENs**

### Phase 4: VTN BFF + VTN Web UI (Priority: LOWER)

The operator console for managing the VTN:

1. **Build the Rust BFF** (axum) — OAuth proxy, caching, event composition
2. **Build the React VTN UI** — Dashboard, Programs, VENs, Events, Event Composer
3. **Deploy as additional containers on Pi4-Server**

### Phase 5: Reporting and Telemetry (Priority: LOWER)

1. **Implement report sender in VEN** — submit telemetry to VTN
2. **Verify VTN report ingestion**
3. **Add telemetry charts to VEN UI**
4. **Evaluate whether VTN persists report datapoints** (known open question)

### Phase 6: Hardening and Observability (Priority: FUTURE)

- Structured JSON logging across all containers
- Prometheus metrics endpoints
- Retry/backoff on VEN failures
- Offline telemetry buffering
- Optional TLS via reverse proxy

---

## Architecture Reference

```
Raspberry Pi 4 — Docker Host
├── openadr-net (bridge network)
│
├── vtn-vtn-db-1      [postgres:16-alpine]     :5432  RUNNING
├── vtn-vtn-1          [openleadr-rs]           :3000  RUNNING
│
├── ven-1              [ven-app]                :8081  NOT YET
├── ven-2              [ven-app]                :8082  NOT YET
├── ven-3              [ven-app]                :8083  NOT YET
│
├── vtn-bff            [rust axum]              :8090  NOT YET
├── vtn-ui             [react+nginx]            :8080  NOT YET
└── ven-ui             [react+nginx]            :8084  NOT YET
```

---

## Key Learnings

- VTN auto-migrates on first boot — no need for manual `cargo sqlx migrate run`
- Token endpoint is `/auth/token`, not `/oauth/token`
- Token expires in 30 days (2,592,000 sec), not 1 hour
- VTN build takes ~25 min on Pi4 ARM64 (first time); cached builds are fast
- SSH to Pi has no interactive terminal — git credentials must be written directly to `~/.git-credentials`
- Role-based access is enforced: wrong role = 403 Forbidden

---

*Last updated: 2026-02-06*
