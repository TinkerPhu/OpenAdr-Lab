# OpenADR VTN Setup — Step‑By‑Step Instructions (Reproducing the Blog Post)

**Purpose**

This document provides exact, detailed instructions to reproduce the environment and outcome described in the blog post:

> “Setting up an OpenADR VTN from example”

The goal is to reach the **same functional state** as the blog author:

- OpenADR VTN running locally
- PostgreSQL database running in Docker
- SQL migrations applied
- Default credentials loaded
- OAuth token obtainable
- API reachable (e.g., `/programs` endpoint)

These instructions are written for both **humans and AI agents**.

---

# 1. Prerequisites

Ensure the following are installed on your system.

## 1.1 Git

```bash
git --version
```

If missing:

```bash
sudo apt-get update
sudo apt-get install git
```

---

## 1.2 Docker + Docker Compose

```bash
docker --version
docker compose version
```

If missing:

```bash
sudo apt-get update
sudo apt-get install docker.io docker-compose-plugin
sudo usermod -aG docker $USER
```

Log out and back in after adding yourself to the docker group.

---

## 1.3 NOT required for Docker‑based deployment

The original blog post installs Rust, sqlx‑cli, and psql on the host because
it builds and runs the VTN natively. **Our setup builds the VTN inside Docker,
so none of these are needed on the host.**

| Tool | Blog post needed it for | Why we skip it |
|------|-------------------------|----------------|
| **Rust toolchain** (~1–2 GB) | Building VTN binary | `vtn.Dockerfile` uses `rust:1.90-alpine` as its build stage — compilation happens inside the Docker image ([source: vtn.Dockerfile from repo](https://github.com/OpenLEADR/openleadr-rs/blob/main/vtn.Dockerfile)) |
| **sqlx‑cli** | Running `cargo sqlx migrate run` | The Dockerfile builds with `SQLX_OFFLINE=true`, embedding migration metadata. See section 4 for how we handle migrations. |
| **psql** (PostgreSQL client) | Loading test fixtures | We use `docker exec` into the Postgres container instead (see section 7). |

> **Note on build time:** The first `docker compose up` builds the VTN from
> source inside Docker. On a Pi4 (ARM64) this takes ~15–30 minutes. Subsequent
> builds use Docker layer cache and are much faster.

---

# 2. Clone the OpenLEADR Repository

Clone from the **project root** (one level above `VTN/`):

```bash
cd /srv/docker/openadr_lab          # or wherever your project root is
git clone https://github.com/OpenLEADR/openleadr-rs.git
```

> **Do not** `cd` into the clone. The `VTN/docker-compose.yml` references it
> via `build: context: ../openleadr-rs`.

This repository contains:

- VTN server implementation
- VEN client library
- Docker setup (`vtn.Dockerfile`)
- SQL migrations (`openleadr-vtn/migrations/`)
- Test credential fixtures (`fixtures/test_user_credentials.sql`)

---

# 3. Start PostgreSQL via Docker

From the `VTN/` directory (where `docker-compose.yml` lives):

```bash
cd VTN
docker compose up -d db
```

Expected result:

- A PostgreSQL 16 Alpine container starts in the background.
- Default DB name/user/password: `openadr` / `openadr` / `openadr` (from `.env`).

Verify:

```bash
docker ps
```

You should see a running Postgres container (`vtn-db-1`).

---

# 4. SQL Migrations — Automatic

Migrations create all required database tables (15 tables total).

**The VTN binary runs migrations automatically at startup.** No manual
migration step is needed. This was confirmed during deployment — the VTN
log shows SQLx migrations executing on first boot, creating all tables
including `_sqlx_migrations`, `program`, `event`, `ven`, `"user"`, etc.

> **Skip this step entirely.** It is listed here only to document that the
> blog post's `cargo sqlx migrate run` is unnecessary with the Docker setup.

---

# 5. Start the Full VTN Stack

From the `VTN/` directory:

```bash
docker compose up -d
```

This starts:

- `db` — PostgreSQL database (starts first, waits for healthy)
- `vtn` — OpenLEADR VTN server (builds from `../openleadr-rs/vtn.Dockerfile`)

> **First build takes ~25 minutes on a Pi4 (ARM64).** Subsequent builds use
> Docker layer cache and are much faster.

Verify containers:

```bash
docker ps
```

You should see two containers: `vtn-db-1` and `vtn-vtn-1`.

Check VTN health:

```bash
curl http://localhost:3000/health
```

Expected response: `OK`

---

# 6. Verify VTN API is Reachable

Test an unauthenticated call (confirms routing is working):

```bash
curl http://localhost:3000/programs
```

Expected result — HTTP 401 JSON error:

```json
{
  "type": "about:blank",
  "status": 401,
  "title": "Unauthorized",
  "detail": "..."
}
```

This confirms the VTN is running and enforcing authentication.

---

# 7. Load Default Credential Fixtures (Required)

After migrations, the database contains **no users or clients**. Without
loading fixtures, all OAuth token requests will fail with `invalid_client`.

The project provides a SQL fixture with test credentials:

```bash
docker exec -i vtn-db-1 \
  psql -U openadr openadr < ../openleadr-rs/fixtures/test_user_credentials.sql
```

> Run this from the `VTN/` directory so the relative path to the fixture
> file resolves correctly.

Expected result — five test users inserted:

| client_id | client_secret | Role | Access |
|-----------|--------------|------|--------|
| `any-business` | `any-business` | AnyBusiness | programs, events |
| `ven-manager` | `ven-manager` | VenManager | programs, events, **vens** |
| `user-manager` | `user-manager` | UserManager | user management |
| `business-1` | `business-1` | Business (scoped) | scoped access |
| `ven-1` | `ven-1` | VEN (scoped) | scoped VEN access |

---

# 8. Obtain OAuth Access Token

Use the client credentials grant against `/auth/token`:

```bash
curl -X POST \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -d 'grant_type=client_credentials&client_id=any-business&client_secret=any-business' \
  http://localhost:3000/auth/token
```

> **Confirmed:** The token endpoint is `/auth/token`. The path `/oauth/token`
> returns 404.

Expected response:

```json
{
  "access_token": "eyJ...",
  "token_type": "Bearer",
  "expires_in": 2592000
}
```

> **Note:** `expires_in` is 2592000 seconds (30 days), not 3600 (1 hour).

---

# 9. Call Authenticated Endpoints

Save the token for reuse:

```bash
TOKEN=$(curl -s -X POST \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -d 'grant_type=client_credentials&client_id=any-business&client_secret=any-business' \
  http://localhost:3000/auth/token | python3 -c "import sys,json; print(json.load(sys.stdin)['access_token'])")
```

Test authenticated endpoints:

```bash
curl -H "Authorization: Bearer $TOKEN" http://localhost:3000/programs
curl -H "Authorization: Bearer $TOKEN" http://localhost:3000/events
```

Expected result: `[]` (empty JSON arrays — no programs or events created yet).

> **Role limitations:** The `any-business` user can access `/programs` and
> `/events`, but **not** `/vens` (returns 403 Forbidden). To manage VENs,
> obtain a token with the `ven-manager` credentials instead.

---

# 10. Inspect Database (Optional)

Use any SQL client (psql, Beekeeper Studio, etc.), or use `docker exec`:

```bash
docker exec -it vtn-db-1 psql -U openadr openadr
```

List tables:

```sql
\dt
```

Inspect credential fixtures:

```sql
SELECT * FROM user_credentials;
```

> **Note:** The user table is named `"user"` (quoted — it's a SQL reserved
> word). Use double quotes when querying it: `SELECT * FROM "user";`

---

# 11. Final Expected State

If all steps succeeded, you now have:

- Running PostgreSQL container
- Migrated OpenADR schema
- Running VTN server
- OAuth client credentials loaded
- Working token endpoint
- Authenticated access to VTN APIs

This reproduces the blog post’s achieved environment.

---

# 12. Troubleshooting

## Containers not starting

```bash
docker compose logs
```

---

## Migration errors

Ensure DB is reachable:

```bash
docker ps
```

---

## Token request fails

Check:

- Fixture loaded
- Client ID/secret correct
- Token endpoint path

---

# 13. Next Steps (Beyond Blog)

Once this state is reached, typical next actions:

- Create programs
- Register VENs
- Send events
- Implement reports

---

# END

Following these steps reproduces the exact working state achieved in the referenced blog post.

