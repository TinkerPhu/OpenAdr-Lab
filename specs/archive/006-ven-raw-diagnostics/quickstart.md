# Quickstart: VEN Raw Data Diagnostics Page

**Feature**: 006-ven-raw-diagnostics
**Date**: 2026-03-18

---

## Development Setup

No new dependencies required. All libraries are already installed:
- `recharts` — already used by `ControllerV2` charts
- `@mui/material` — already used throughout the UI
- `@tanstack/react-query` — already used throughout the UI

---

## Running the VEN UI locally

```bash
cd VEN/ui
npm run dev
```

Open http://localhost:5173 and select any VEN from the dropdown.

> **Note (Windows)**: Run from `C:/DriveD/Tinker/OpenAdr-Lab/VEN/ui` — running from a subst drive path (`D:\...`) causes Vite to fail resolving `__dirname`.

---

## Running unit tests

```bash
cd VEN/ui
npm test
```

Tests use Vitest + @testing-library/react. Each chart component has a corresponding test in `VEN/ui/src/__tests__/`.

---

## Deploying to Pi4-Server

```bash
# 1. Commit and push changes
git add VEN/ui/src/
git commit -m "feat(ven-ui): add raw data diagnostics page"
git push

# 2. Pull and rebuild on Pi4
ssh Pi4-Server "cd /srv/docker/openadr_lab && git pull && docker compose build ven-ui && docker compose up -d ven-ui"
```

The `ven-ui` service serves the React app via nginx on port 8214.

---

## Running BDD tests

```bash
# From repo root on Pi4-Server (or via SSH)
docker compose -f tests/docker-compose.test.yml run --build --rm test-runner features/ven_ui_raw_diagnostics.feature
```

> **Always pass `--build`** when any `.feature`, step definition, or VEN source has changed — the image bakes sources at build time.

---

## Key files to create

| File | Purpose |
|------|---------|
| `VEN/ui/src/pages/RawDiagnostics.tsx` | New page — three stacked cells |
| `VEN/ui/src/components/raw-diagnostics/DiagnosticCell.tsx` | Reusable cell wrapper |
| `VEN/ui/src/components/raw-diagnostics/SimProfileChart.tsx` | /sim chart |
| `VEN/ui/src/components/raw-diagnostics/TariffsLineChart.tsx` | /tariffs chart |
| `VEN/ui/src/components/raw-diagnostics/TimelineSeriesChart.tsx` | /timeline/all chart |
| `VEN/ui/src/components/raw-diagnostics/colors.ts` | Shared color palette |
| `VEN/ui/src/__tests__/RawDiagnostics.test.tsx` | Page-level unit tests |
| `VEN/ui/src/__tests__/DiagnosticCell.test.tsx` | Cell wrapper unit tests |
| `tests/features/ven_ui_raw_diagnostics.feature` | BDD acceptance scenarios |
| `tests/steps/ven_ui_raw_diagnostics_steps.py` | BDD step definitions |

## Key files to modify

| File | Change |
|------|--------|
| `VEN/ui/src/App.tsx` | Add route + nav button for Raw Data page |
