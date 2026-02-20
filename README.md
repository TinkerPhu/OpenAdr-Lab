# OpenADR 3 Raspberry Pi Lab

A Raspberry Pi4-hosted **OpenADR 3.0 laboratory environment** for demand response experimentation, multi-VEN simulation, and edge computing research.

## The context

This project is built around the https://github.com/OpenLEADR/openleadr-rs project, and adds infrastructure to experiment and demonstrate. It is close to 100% written by AI which allowed me to get fast progress in short time. 
Tests guarantee the expected behaviour, side effects have not been checked, so no warranties for that!

## Architecture

```
Raspberry Pi 4 (Docker)
+------------------------------------------------+
|  VTN Stack                                     |
|  +--------+  +--------+  +---------+           |
|  | VTN    |  | BFF    |  | VTN UI  |           |
|  | :8200  |  | :8220  |  | :8221   |           |
|  +--------+  +--------+  +---------+           |
|  | DB     |                                    |
|  | :8201  |      openadr-net                   |
|  +--------+                                    |
|                                                |
|  VEN Stack                                     |
|  +---------+  +---------+  +---------+         |
|  | VEN-1   |  | VEN-2   |  | VEN-3   |         |
|  | :8211   |  | :8212   |  | :8213   |         |
|  +---------+  +---------+  +---------+         |
|  | VEN UI  |                                   |
|  | :8214   |                                   |
|  +---------+                                   |
+-----------------------------------------------+
```

| Component | Technology | Description |
|---|---|---|
| VTN | [openleadr-rs](https://github.com/OpenLEADR/openleadr-rs) (Rust) | OpenADR 3.0 Virtual Top Node |
| DB | PostgreSQL 16 | VTN persistence (auto-migrated) |
| BFF | Rust (axum) | Backend-for-frontend with dual OAuth credentials |
| VTN UI | React + MUI + nginx | Operator dashboard (programs, events, VENs, reports) |
| VEN | Rust (axum + tokio) | Polling-based VEN with sensor simulation |
| VEN UI | React + MUI + nginx | Device dashboard (events, programs, sensors) |

## Quick Start

```bash
# Clone with submodules (openleadr-rs fork)
git clone --recursive https://github.com/TinkerPhu/OpenAdr-Lab.git

# Deploy VTN stack
ssh Pi4-Server "cd /srv/docker/openadr_lab/VTN && docker compose up -d --build"

# Seed programs and events
ssh Pi4-Server "cd /srv/docker/openadr_lab && python3 scripts/seed_vtn.py"

# Deploy VEN stack
ssh Pi4-Server "cd /srv/docker/openadr_lab/VEN && docker compose up -d --build"
```

## Testing

The project includes integration tests (behave/BDD), resilience tests, and E2E browser tests (Playwright), all running in Docker.

```bash
# Run all tests locally
cd tests
docker compose -f docker-compose.test.yml run --build --rm test-runner

# Run only resilience tests
docker compose -f docker-compose.test.yml run --build --rm test-runner --tags=@resilience
```

Tests also run automatically via GitHub Actions on push to `main`.

## Project Structure

```
OpenAdr-Lab/
  VTN/
    docker-compose.yml    # VTN + DB + BFF + UI
    bff/                  # Rust axum BFF (dual-credential proxy)
    ui/                   # React VTN operator UI
  VEN/
    src/                  # Rust VEN application
    docker-compose.yml    # 3 VEN instances + VEN UI
    ui/                   # React VEN device UI
  openleadr-rs/           # Git submodule (TinkerPhu fork)
  scripts/
    seed_vtn.py           # Seed programs, events, and VEN enrollment
  tests/
    features/             # Behave BDD scenarios (15 features, 49 scenarios)
    docker-compose.test.yml
  docs/                   # Project documentation
```

## Documentation

| Document | Description |
|---|---|
| [System Design](docs/open_adr_3_raspberry_pi_lab_complete_system_design.md) | Full architecture, data flows, and design decisions |
| [Project Journal](docs/project_journal.md) | Implementation history, key learnings, and phase summaries |
| [Use Case Manual](docs/USE-CASE-MANUAL.md) | Step-by-step guide for all demand response use cases |
| [Use Cases](docs/USE-CASES.md) | Use case definitions and test coverage |
| [Testing Guide](docs/TESTING.md) | Test strategy, running tests, and CI setup |
| [FAQ](docs/FAQ.md) | Common questions and troubleshooting |
| [Glossary](docs/GLOSSARY.md) | OpenADR terminology reference |
| [DR Simulation Concept](docs/concept_vtn_ven_demand_response_simulation.md) | Concept for realistic demand response simulation |
| [React Guidelines](docs/REACT_GUIDELINES.md) | UI development conventions |

## Seeded Data

| Program | Enrolled VENs | Description |
|---|---|---|
| Summer Peak DR | ven-1, ven-2 | Peak demand reduction |
| EV Managed Charging | ven-2, ven-3 | Coordinated EV charging |
| HVAC Optimization | all (open) | Building HVAC load management |

## License

[MIT](LICENSE)
