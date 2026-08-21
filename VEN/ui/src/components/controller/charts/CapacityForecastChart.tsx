import { Stack, Typography } from "@mui/material";
import type { CapacityCurve, CapacityCurvesResponse } from "../../../api/types";
import type { NamedSample, TimestampedRow } from "../../charts/mergeSeries";
import {
  mergeTimestampedSeries,
  locfFillKeys,
  clipRowsToWindow,
  ensureNonEmptyRows,
} from "../../charts/mergeSeries";
import {
  minSpanDomain,
  MIN_POWER_SPAN_KW,
  zeroAnchoredTicks,
  formatPowerTick,
} from "../../charts/axisDomain";
import { formatPowerValue, formatEnergyKwh } from "../../charts/unitFormat";
import { CELL_CHART_HEIGHT } from "../../charts/chartLayout";
import { TimeSeriesChart, type TimeSeriesSeriesSpec } from "../../charts/TimeSeriesChart";
import { formatTs } from "./tariffChartShared";

interface CapacityForecastChartProps {
  /** `GET /flexibility/capacity` — null before the first dispatcher tick. */
  curves: CapacityCurvesResponse | null;
  height?: number;
}

/** Cumulative energy (kWh) across a step curve — mirrors
 * `CapacityCurve::energy_kwh_total` (VEN/src/entities/capacity_curve.rs) so
 * the number shown here always matches what the backend itself would report. */
function energyKwhTotal(curve: CapacityCurve): number {
  let energy = 0;
  for (let i = 0; i < curve.steps.length - 1; i++) {
    const dtH = (curve.steps[i + 1].elapsed_s - curve.steps[i].elapsed_s) / 3600;
    energy += Math.abs(curve.steps[i].power_kw) * dtH;
  }
  return energy;
}

/**
 * BL-flexibility-capacity-forecast: renders both sustained-commitment
 * capacity curves (power vs. elapsed time since commitment) as step lines,
 * with each direction's cumulative energy total shown alongside — distinct
 * from `SiteHeadroomChart`, which stays instantaneous-only. See
 * `openspec/changes/flexibility-capacity-forecast/design.md` for why this is
 * a separate chart rather than an extension of that one.
 */
export function CapacityForecastChart({ curves, height }: CapacityForecastChartProps) {
  if (!curves) {
    return (
      <Typography variant="body2" color="text.secondary" data-testid="capacity-forecast-empty">
        No capacity forecast yet — waiting for the first dispatcher tick.
      </Typography>
    );
  }

  const { import: importCurve, export: exportCurve } = curves;
  const startMs = new Date(importCurve.start).getTime();
  const tMin = startMs;
  const tMax = Math.max(
    ...importCurve.steps.map((s) => startMs + s.elapsed_s * 1000),
    ...exportCurve.steps.map((s) => startMs + s.elapsed_s * 1000),
    startMs
  );

  const importSamples: NamedSample[] = importCurve.steps.map((s) => ({
    ts: startMs + s.elapsed_s * 1000,
    key: "importKw",
    value: s.power_kw,
  }));
  const exportSamples: NamedSample[] = exportCurve.steps.map((s) => ({
    ts: startMs + s.elapsed_s * 1000,
    key: "exportKw",
    value: s.power_kw,
  }));

  const merged = mergeTimestampedSeries([], [...importSamples, ...exportSamples]);
  // Step curves hold their value until the next breakpoint — LOCF forward-fills
  // each series across the other's breakpoints, same reasoning as
  // SiteHeadroomChart's gridPowerKw fill.
  const filled = locfFillKeys(merged, ["importKw", "exportKw"]);
  const clipped = clipRowsToWindow(filled, tMin, tMax);
  const chartData: TimestampedRow[] = ensureNonEmptyRows(clipped, tMin, tMax);

  const domain = minSpanDomain(
    chartData.flatMap((row) => [row.values?.importKw, row.values?.exportKw]),
    MIN_POWER_SPAN_KW
  );

  const series: TimeSeriesSeriesSpec[] = [
    {
      key: "Import commitment [kW]",
      axisId: "power",
      dataKey: (row) => row.values?.["importKw"] ?? null,
      color: "#D32F2F",
      type: "stepAfter",
      connectNulls: true,
      formatter: formatPowerValue,
    },
    {
      key: "Export commitment [kW]",
      axisId: "power",
      dataKey: (row) => row.values?.["exportKw"] ?? null,
      color: "#2E7D32",
      type: "stepAfter",
      connectNulls: true,
      formatter: formatPowerValue,
    },
  ];

  return (
    <Stack spacing={1}>
      <Stack direction="row" spacing={3}>
        <Typography variant="body2" data-testid="capacity-forecast-import-energy">
          Import energy available: {formatEnergyKwh(energyKwhTotal(importCurve))}
        </Typography>
        <Typography variant="body2" data-testid="capacity-forecast-export-energy">
          Export energy available: {formatEnergyKwh(energyKwhTotal(exportCurve))}
        </Typography>
      </Stack>
      <TimeSeriesChart
        testId="capacity-forecast-chart"
        data={chartData}
        tMin={tMin}
        tMax={tMax}
        xAxisTickFormatter={formatTs}
        axes={[
          {
            id: "power",
            width: 46,
            domain,
            tickFormatter: formatPowerTick,
            ticks: zeroAnchoredTicks(domain),
          },
        ]}
        series={series}
        height={height ?? CELL_CHART_HEIGHT}
        margin={{ top: 4, right: 24, left: 0, bottom: 0 }}
      />
    </Stack>
  );
}
