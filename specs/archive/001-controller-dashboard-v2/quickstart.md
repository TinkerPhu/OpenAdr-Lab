# Quickstart: VEN Controller Dashboard V2

**Branch**: `001-controller-dashboard-v2`
**Date**: 2026-03-14

---

## Prerequisites

- SSH access to Pi4-Server
- VEN stack running (`vtn-vtn-1`, `ven-ven-1-1` on port 8211)
- Local repo up to date on branch `001-controller-dashboard-v2`

---

## Development Flow

### 1. Write BDD scenarios first (Principle II)

Feature files live at `tests/features/controller/`. Write `.feature` files before writing any implementation code. Run them to confirm they fail (red phase).

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  features/controller/01_layout.feature"
```

### 2. Add backend stubs (VEN/src/state.rs)

Add 3 fields to `UserOverrides`. Build the VEN image:

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose build ven-ven-1 && \
  docker compose up -d ven-ven-1"
```

### 3. Implement React page and components

Edit files in `VEN/ui/src/`. After changes, rebuild the VEN UI image:

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose build ven-ui && \
  docker compose up -d ven-ui"
```

VEN UI is served at `http://pi4server.local:8214`.

### 4. Run unit tests

```bash
# From local machine (mapped drive path):
cd /c/DriveD/Tinker/OpenAdr-Lab/VEN/ui
npm test
```

Or run on Pi4 if needed:
```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab/VEN/ui && npm test"
```

### 5. Run BDD integration tests

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner \
  features/controller/"
```

`--build` is **always required** when feature files, step definitions, or VEN source changed.

### 6. Run full test suite

```bash
ssh Pi4-Server "cd /srv/docker/openadr_lab && \
  docker compose -f tests/docker-compose.test.yml run --build --rm test-runner"
```

Zero failures required before marking the feature complete.

---

## Key Files

| File | Purpose |
|---|---|
| `VEN/ui/src/pages/ControllerV2.tsx` | New page — top-level layout |
| `VEN/ui/src/components/controller/types.ts` | Shared TypeScript types |
| `VEN/ui/src/components/controller/dataBuilders.ts` | Data transform functions |
| `VEN/ui/src/App.tsx` | Add route + nav tab here |
| `VEN/src/state.rs` | Add 3 stub fields to UserOverrides |
| `tests/features/controller/` | BDD scenarios |
| `VEN/ui/src/__tests__/ControllerV2.test.tsx` | Unit tests |

---

## Viewing the Dashboard

Navigate to `http://pi4server.local:8214/controller` after VEN UI is running.

The VEN selector dropdown (top right) switches between VEN-1 (8211), VEN-2 (8212), VEN-3 (8213).

---

## Notes

- **POST /sim/override is full-replace**: When posting control changes, always read current override state first and merge with changes. Never overwrite unrelated fields.
- **Trace limit**: `useTrace(500)` provides ~8 min of history at 1s tick rate. The graph will have sparse or no data for the leftmost part of the 1h window — this is expected behavior.
- **VEN UI vitest path**: Run tests from `/c/DriveD/Tinker/OpenAdr-Lab/VEN/ui` (real path), not `/d/...` (subst drive). See KEY_LEARNINGS.md.
