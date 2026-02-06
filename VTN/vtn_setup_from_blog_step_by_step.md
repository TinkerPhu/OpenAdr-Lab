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

```bash
git clone https://github.com/OpenLEADR/openleadr-rs.git
cd openleadr-rs
```

This repository contains:

- VTN server implementation
- VEN client library
- Docker setup
- SQL migrations
- Test credential fixtures

---

# 3. Start PostgreSQL via Docker

From the repository root:

```bash
docker compose up -d db
```

Expected result:

- A PostgreSQL container starts in the background.
- Default DB name/user/password are defined in the repo compose file.

Verify:

```bash
docker ps
```

You should see a running Postgres container.

---

# 4. Run SQL Migrations

Migrations create all required database tables.

**Blog post method** (requires Rust + sqlx‑cli on host):

```bash
cargo sqlx migrate run
```

**Docker‑based alternative** (no host tooling needed):

The VTN binary may run migrations automatically at startup. If it does not,
and tables are missing after step 5, you can run migrations from a temporary
container:

```bash
docker run --rm --network=host \
  -e DATABASE_URL=postgres://openadr:openadr@localhost:5432/openadr \
  ghcr.io/launchbadge/sqlx-cli migrate run
```

> **Assumption:** Whether the openleadr‑rs VTN auto‑migrates at startup has
> not been verified. We will confirm this when we first bring up the stack.
> If it does, this step can be skipped entirely.

Expected result:

- Migration scripts execute successfully.
- Tables for users, clients, programs, events, etc. are created.

If migration fails, ensure:

- DB container is running
- Environment variables in `.env` or compose match defaults

---

# 5. Start the Full VTN Stack

```bash
docker compose up -d
```

This starts:

- VTN server
- Any dependent services defined in compose

Verify containers:

```bash
docker ps
```

---

# 6. Verify VTN API is Reachable

Open in browser or curl:

```
http://localhost:3000
```

Test an endpoint (unauthenticated call will likely fail but confirms routing):

```bash
curl http://localhost:3000/programs
```

Expected result:

- HTTP response from VTN
- Possibly empty or unauthorized

---

# 7. Load Default Credential Fixtures

After migrations, the database contains no users/clients.

The project provides a SQL fixture used in tests.

**Blog post method** (requires psql on host):

```bash
psql -U openadr -W openadr -h localhost openadr < fixtures/test_user_credentials.sql
```

Password when prompted: `openadr`

**Docker‑based alternative** (no host psql needed):

```bash
docker exec -i $(docker compose ps -q db) \
  psql -U openadr openadr < openleadr-rs/fixtures/test_user_credentials.sql
```

Expected result:

- Default OAuth clients/users inserted
- Example client: `any-business`

---

# 8. Obtain OAuth Access Token

Use client credentials grant.

Endpoint name depends on configuration.

Common paths:

- `/auth/token`
- `/oauth/token`

Try:

```bash
curl -X POST \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -d 'grant_type=client_credentials&client_id=any-business&client_secret=any-business' \
  http://localhost:3000/auth/token
```

If 404, try:

```bash
http://localhost:3000/oauth/token
```

Expected response:

```json
{
  "access_token": "...",
  "token_type": "Bearer",
  "expires_in": 3600
}
```

---

# 9. Call an Authenticated Endpoint

Use the token:

```bash
curl -H "Authorization: Bearer <ACCESS_TOKEN>" \
  http://localhost:3000/programs
```

Expected result:

- Valid JSON response
- Likely empty list (no programs created yet)

This matches the blog author’s state.

---

# 10. Inspect Database (Optional)

Use any SQL client (psql, Beekeeper Studio, etc.), or use `docker exec`:

```bash
docker exec -it $(docker compose ps -q db) psql -U openadr openadr
```

List tables:

```sql
\dt
```

Inspect users:

```sql
SELECT * FROM users;
```

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

