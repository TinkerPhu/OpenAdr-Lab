# Quickstart: Grid-Aligned UI Timeline

## Prerequisites

- RF-05c (010-uniform-grid-timeline) backend must be deployed and running
- Node.js + npm installed for local UI development
- VEN running with simulator producing timeline data

## Development

```bash
# Run VEN UI locally (from real path, not subst drive)
cd /c/DriveD/Tinker/OpenAdr-Lab/VEN/ui
npm install
npm run dev

# Run unit tests
cd /c/DriveD/Tinker/OpenAdr-Lab/VEN/ui
npm test

# Run specific test file
npm test -- GridAccumulatedCell
npm test -- dataBuilders
```

## Verification

1. **Unit tests**: `npm test` — all vitest tests pass
2. **Visual check**: Open Controller V2 page, verify:
   - Stacked area chart renders without zero-spike artifacts
   - Individual asset cell charts render correctly
   - Tariff cell grid power line aligns with asset charts
   - Now cursor line is positioned correctly
3. **BDD tests**: Run existing Controller V2 BDD scenarios to confirm no regressions

## Key Files

| File | What to check |
|------|---------------|
| `VEN/ui/src/components/controller-v2/types.ts` | `values` is `Record<string, number> \| null` |
| `VEN/ui/src/components/controller-v2/GridAccumulatedCell.tsx` | No `findNearest`, no `TOLERANCE_MS`, positional zip |
| `VEN/ui/src/components/controller-v2/dataBuilders.ts` | `computeForecastEnergy` skips null values |
| `VEN/ui/src/components/controller-v2/tariffBuilders.ts` | `buildPowerPoints` handles null values |
| `VEN/ui/src/api/client.ts` | `resolution` parameter supported |
