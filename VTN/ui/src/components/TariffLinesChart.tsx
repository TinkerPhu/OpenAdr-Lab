import { useMemo, type ReactElement } from "react";
import { TimeSeriesChart } from "@lab/ui/TimeSeriesChart";
import type { TimeSeriesSeriesSpec } from "@lab/ui/TimeSeriesChart";
import { tightSpanDomain, MIN_TARIFF_SPAN_EUR_KWH } from "@lab/ui/axisDomain";
import { CELL_CHART_HEIGHT } from "@lab/ui/chartLayout";
import { renderDayNightShading } from "@lab/ui/DayNightShading";
import type { TimestampedRow } from "@lab/ui/mergeSeries";
import {
  fleetChartWindow,
  fleetChartTicks,
  FLEET_AXIS_WIDTH,
  FLEET_TICK_INTERVAL,
} from "./fleetChartWindow";
import { LAB_LOCATION } from "../utils/labLocation";

/** The tariff y axis; an overlay (the workbench's drag layer) reads its scale by this id. */
export const TARIFF_AXIS_ID = "tariff";

/** Twice the standard cell height, as asked for — deliberately the literal 2x
 *  rather than the existing CELL_CHART_HEIGHT_TALL, which is 2.5x. */
export const TARIFF_CHART_HEIGHT = CELL_CHART_HEIGHT * 2;

/**
 * The one tariff chart: price lines over time on the fleet's shared window, ticks, day/night
 * shading and price axis. The workbench's live-plus-draft chart and the Fleet page's per-group
 * import and export charts differ only in which lines they hand it.
 */
export function TariffLinesChart({
  rows,
  series,
  windowMinutes,
  tickMinutes,
  nowMs,
  aheadMinutes = 0,
  overlay,
  testId,
}: {
  rows: TimestampedRow[];
  series: TimeSeriesSeriesSpec[];
  windowMinutes: number;
  /** Tick spacing, chosen per window by the page so both charts agree. */
  tickMinutes: number;
  nowMs: number;
  /** Minutes past now the axis runs. The Fleet page looks back only; the tariff workbench
   *  looks ahead, because the interesting part of a tariff is the part that has not happened. */
  aheadMinutes?: number;
  /** Painted over the plot (recharts `<Customized>`): the workbench's pointer-editing layer. */
  overlay?: ReactElement;
  testId: string;
}) {
  const { tMin, tMax } = fleetChartWindow(nowMs, windowMinutes, aheadMinutes);
  const ticks = fleetChartTicks(tMin, tMax, tickMinutes);

  // Memoised: one React element plus one DOM rect per band, rebuilt on every
  // render otherwise, and this component re-renders on each poll.
  const dayNight = useMemo(
    () =>
      renderDayNightShading(TARIFF_AXIS_ID, {
        tMin,
        tMax,
        latitudeDeg: LAB_LOCATION.latitudeDeg,
        longitudeDeg: LAB_LOCATION.longitudeDeg,
      }),
    [tMin, tMax],
  );
  const values = useMemo(() => rows.flatMap((row) => Object.values(row.values ?? {})), [rows]);

  return (
    <TimeSeriesChart
      data={rows}
      tMin={tMin}
      tMax={tMax}
      axes={[
        {
          id: TARIFF_AXIS_ID,
          unit: " €/kWh",
          width: FLEET_AXIS_WIDTH,
          // tightSpanDomain, never minSpanDomain: a tariff is strictly
          // positive with no meaningful zero, and anchoring the axis at 0
          // squashes a 0.20–0.40 band into the top of the chart.
          //
          // No autoScale either — every line is the same quantity on one
          // axis, so there is no outlier for a toggle to reveal.
          domain: tightSpanDomain(values, MIN_TARIFF_SPAN_EUR_KWH),
        },
      ]}
      series={series}
      nowMs={nowMs}
      referenceAxisId={TARIFF_AXIS_ID}
      backgroundAreas={dayNight}
      extraReferenceAreas={overlay ? [overlay] : undefined}
      xAxisTicks={ticks}
      xAxisInterval={FLEET_TICK_INTERVAL}
      height={TARIFF_CHART_HEIGHT}
      xAxisTickFormatter={(ts: number) => new Date(ts).toLocaleTimeString()}
      interactiveLegend
      testId={testId}
    />
  );
}
