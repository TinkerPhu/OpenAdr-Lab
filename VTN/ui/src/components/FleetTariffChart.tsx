import { useMemo } from "react";
import { Alert, Typography } from "@mui/material";
import { TimeSeriesChart } from "@lab/charts/TimeSeriesChart";
import type { TimeSeriesSeriesSpec } from "@lab/charts/TimeSeriesChart";
import { tightSpanDomain, MIN_TARIFF_SPAN_EUR_KWH } from "@lab/charts/axisDomain";
import { EmptyState } from "@lab/charts/EmptyState";
import { CELL_CHART_HEIGHT } from "@lab/charts/chartLayout";
import { COLOR_IMPORT_TARIFF, COLOR_EXPORT_TARIFF, TARIFF_LINE_STYLE } from "@lab/charts/types";
import type { FleetSignals } from "../api/types";
import { sharedTariff, tariffRows, IMPORT_KEY, EXPORT_KEY } from "../utils/fleetTariff";
import { fleetChartWindow, FLEET_AXIS_WIDTH } from "./fleetChartWindow";

const AXIS_ID = "tariff";

/** Twice the standard cell height, as asked for — deliberately the literal 2x
 *  rather than the existing CELL_CHART_HEIGHT_TALL, which is 2.5x. */
const CHART_HEIGHT = CELL_CHART_HEIGHT * 2;

/**
 * What the fleet was being charged, above what it did.
 *
 * The power chart says a site dropped 3 kW at 19:00. On its own that looks the
 * same whether a capacity limit landed or the evening tariff stepped up. This
 * draws the price on the same time axis so the two can be read together, which
 * is the whole reason it sits directly above rather than on its own tab.
 *
 * It costs no new request: the Fleet page already fetches `/api/fleet/signals`
 * for the shaded windows behind the power curves, and the price bands ride in
 * the same response.
 */
export function FleetTariffChart({
  signals,
  windowMinutes,
  nowMs,
}: {
  signals?: FleetSignals;
  windowMinutes: number;
  nowMs: number;
}) {
  const { tMin, tMax } = fleetChartWindow(nowMs, windowMinutes);

  const { rows, values, agreeingVens, dissentingVens, knownVens } = useMemo(() => {
    const shared = sharedTariff(signals);
    const rows = tariffRows(shared.bands, tMin, tMax);
    return {
      rows,
      values: rows.flatMap((row) => Object.values(row.values ?? {})),
      ...shared,
    };
  }, [signals, tMin, tMax]);

  const series: TimeSeriesSeriesSpec[] = [
    {
      key: IMPORT_KEY,
      axisId: AXIS_ID,
      dataKey: (row) => row.values?.[IMPORT_KEY] ?? null,
      color: COLOR_IMPORT_TARIFF,
      // `type` and `strokeWidth` are deliberately unset: inheriting
      // TimeSeriesChart's stepAfter/1.5 defaults is what keeps this identical
      // to the VEN Controller tab without restating them here.
      ...TARIFF_LINE_STYLE,
    },
    {
      key: EXPORT_KEY,
      axisId: AXIS_ID,
      dataKey: (row) => row.values?.[EXPORT_KEY] ?? null,
      color: COLOR_EXPORT_TARIFF,
      ...TARIFF_LINE_STYLE,
    },
  ];

  if (rows.length === 0) {
    return (
      <EmptyState
        message={`No tariff was announced to the fleet in the last ${windowMinutes} minutes.`}
        testId="fleet-tariff-empty"
        height={CHART_HEIGHT}
      />
    );
  }

  return (
    <>
      {/* Not today's case, and the reason this is here rather than assumed: an
          event may target a subset of the fleet, so "the lab tariff" can stop
          being one thing without anything else noticing. */}
      {dissentingVens.length > 0 && (
        <Alert severity="warning" data-testid="fleet-tariff-disagree">
          {dissentingVens.length} VEN(s) were sent a different tariff (
          {dissentingVens.slice(0, 3).join(", ")}
          {dissentingVens.length > 3 ? ", …" : ""}) — the line below is what the other{" "}
          {agreeingVens} were sent. See the Signals tab for each site.
        </Alert>
      )}
      <TimeSeriesChart
        data={rows}
        tMin={tMin}
        tMax={tMax}
        axes={[
          {
            id: AXIS_ID,
            unit: " €/kWh",
            width: FLEET_AXIS_WIDTH,
            // tightSpanDomain, never minSpanDomain: a tariff is strictly
            // positive with no meaningful zero, and anchoring the axis at 0
            // squashes a 0.20–0.40 band into the top of the chart.
            //
            // No autoScale either — both series are the same quantity on one
            // axis, so there is no outlier for a toggle to reveal.
            domain: tightSpanDomain(values, MIN_TARIFF_SPAN_EUR_KWH),
          },
        ]}
        series={series}
        nowMs={nowMs}
        referenceAxisId={AXIS_ID}
        height={CHART_HEIGHT}
        xAxisTickFormatter={(ts: number) => new Date(ts).toLocaleTimeString()}
        interactiveLegend
        testId="fleet-tariff-chart"
      />
      {/* States the fold rather than leaving "one line for twenty sites" to be
          assumed. */}
      <Typography variant="caption" color="text.secondary" data-testid="fleet-tariff-source">
        shared lab tariff · {agreeingVens} of {knownVens} VENs
      </Typography>
    </>
  );
}
