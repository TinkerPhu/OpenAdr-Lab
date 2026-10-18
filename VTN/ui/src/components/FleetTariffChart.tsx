import { useMemo, type ReactElement } from "react";
import { Alert, Typography } from "@mui/material";
import { TimeSeriesChart } from "@lab/ui/TimeSeriesChart";
import type { TimeSeriesSeriesSpec } from "@lab/ui/TimeSeriesChart";
import { tightSpanDomain, MIN_TARIFF_SPAN_EUR_KWH } from "@lab/ui/axisDomain";
import { EmptyState } from "@lab/ui/EmptyState";
import { CELL_CHART_HEIGHT } from "@lab/ui/chartLayout";
import { COLOR_IMPORT_TARIFF, COLOR_EXPORT_TARIFF, TARIFF_LINE_STYLE } from "@lab/ui/types";
import type { FleetSignals, SignalBand } from "../api/types";
import {
  sharedTariff,
  tariffRows,
  IMPORT_KEY,
  EXPORT_KEY,
  DRAFT_IMPORT_KEY,
  DRAFT_EXPORT_KEY,
} from "../utils/fleetTariff";
import {
  fleetChartWindow,
  fleetChartTicks,
  FLEET_AXIS_WIDTH,
  FLEET_TICK_INTERVAL,
} from "./fleetChartWindow";
import { renderDayNightShading } from "@lab/ui/DayNightShading";
import { LAB_LOCATION } from "../utils/labLocation";


/** The tariff y axis; an overlay (the workbench's drag layer) reads its scale by this id. */
export const TARIFF_AXIS_ID = "tariff";
const AXIS_ID = TARIFF_AXIS_ID;

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
  tickMinutes,
  nowMs,
  aheadMinutes = 0,
  draftBands = [],
  overlay,
}: {
  signals?: FleetSignals;
  windowMinutes: number;
  /** Tick spacing, chosen per window by the page so both charts agree. */
  tickMinutes: number;
  nowMs: number;
  /** Minutes past now the axis runs. The Fleet page looks back only; the tariff workbench
   *  looks ahead, because the interesting part of a tariff is the part that has not happened. */
  aheadMinutes?: number;
  /** An unpublished tariff (the signal workbench's draft), drawn solid beside the live one. */
  draftBands?: SignalBand[];
  /** Painted over the plot (recharts `<Customized>`): the workbench's pointer-editing layer. */
  overlay?: ReactElement;
}) {
  const { tMin, tMax } = fleetChartWindow(nowMs, windowMinutes, aheadMinutes);
  const ticks = fleetChartTicks(tMin, tMax, tickMinutes);

  // Memoised: one React element plus one DOM rect per band, rebuilt on every
  // render otherwise, and this component re-renders on each poll.
  const dayNight = useMemo(
    () =>
      renderDayNightShading(AXIS_ID, {
        tMin,
        tMax,
        latitudeDeg: LAB_LOCATION.latitudeDeg,
        longitudeDeg: LAB_LOCATION.longitudeDeg,
      }),
    [tMin, tMax],
  );

  const { rows, values, agreeingVens, dissentingVens, knownVens } = useMemo(() => {
    const shared = sharedTariff(signals);
    const rows = tariffRows(shared.bands, tMin, tMax, draftBands);
    return {
      rows,
      values: rows.flatMap((row) => Object.values(row.values ?? {})),
      ...shared,
    };
  }, [signals, tMin, tMax, draftBands]);

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
    // The draft is solid where the live tariff is dashed, so the two read apart even
    // where they share a colour and a value.
    ...(draftBands.length > 0
      ? [
          { key: DRAFT_IMPORT_KEY, color: COLOR_IMPORT_TARIFF },
          { key: DRAFT_EXPORT_KEY, color: COLOR_EXPORT_TARIFF },
        ].map(
          ({ key, color }): TimeSeriesSeriesSpec => ({
            key,
            axisId: AXIS_ID,
            dataKey: (row) => row.values?.[key] ?? null,
            color,
            strokeWidth: 2.5,
            connectNulls: TARIFF_LINE_STYLE.connectNulls,
            formatter: TARIFF_LINE_STYLE.formatter,
          }),
        )
      : []),
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
        backgroundAreas={dayNight}
        extraReferenceAreas={overlay ? [overlay] : undefined}
        xAxisTicks={ticks}
        xAxisInterval={FLEET_TICK_INTERVAL}
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
