import { useMemo } from "react";
import { Typography } from "@mui/material";
import { TimeSeriesChart } from "@lab/charts/TimeSeriesChart";
import type { TimeSeriesSeriesSpec } from "@lab/charts/TimeSeriesChart";
import { mergeTimestampedSeries } from "@lab/charts/mergeSeries";
import type { NamedSample } from "@lab/charts/mergeSeries";
import { tightSpanDomain, formatPowerTick } from "@lab/charts/axisDomain";
import { EmptyState } from "@lab/charts/EmptyState";
import { CELL_CHART_HEIGHT } from "@lab/charts/chartLayout";
import { fleetChartWindow, FLEET_AXIS_WIDTH } from "./fleetChartWindow";
import type { FleetHistory, FleetSignals } from "../api/types";
import { venColor, FLEET_SUM_COLOR } from "../utils/venColor";
import { compareVenNames } from "../utils/venOrder";

/** The series key the fleet total is drawn under. Prefixed so it can never
 *  collide with a VEN actually named "fleet". */
export const FLEET_KEY = "__fleet__";

const AXIS_ID = "kw";

/** Three times the standard cell height. Twenty overlapping curves need the
 *  vertical room to be told apart at all — at cell height they are a band, and
 *  "which site moved" is unanswerable. */
const CHART_HEIGHT = CELL_CHART_HEIGHT * 3;

/** Narrowest kW span the axis will scale down to. A fleet sitting still should
 *  read as sitting still, not as amplified rounding noise. */
const MIN_SPAN_KW = 1;

/** kW, like everything else in this lab. The API speaks watts. */
const toKw = (watts: number) => watts / 1000;

/**
 * Every VEN's power on one time axis, plus the fleet total.
 *
 * One chart rather than twenty sparklines, because the question an operator
 * has after publishing a limit is "which site moved", and that is a comparison
 * — it needs a shared axis and a shared time grid. The grid is already shared:
 * the BFF resamples every VEN onto the same buckets through `lab-core`, which
 * is what makes these series addable rather than merely adjacent.
 *
 * Every series is folded into one `mergeTimestampedSeries` row array. That is
 * not a style choice: recharts resolves a hovered tooltip by *array index*, so
 * twenty independently-indexed series would happily show one VEN's value under
 * another VEN's name.
 */
export function FleetPowerChart({
  history,
  signals,
  windowMinutes,
  nowMs,
}: {
  history: FleetHistory;
  /** Bands for what the fleet was being *told*, drawn behind the curves. A
   *  dip means something different depending on whether a limit was in force,
   *  and the chart should not make the reader guess. */
  signals?: FleetSignals;
  windowMinutes: number;
  nowMs: number;
}) {
  const { rows, series, values } = useMemo(() => {
    const samples: NamedSample[] = [];

    for (const ven of history.vens) {
      for (const s of ven.samples) {
        samples.push({ ts: Date.parse(s.ts), key: ven.venName, value: toKw(s.netPowerW) });
      }
    }
    for (const point of history.fleet) {
      samples.push({ ts: Date.parse(point.ts), key: FLEET_KEY, value: toKw(point.netPowerW) });
    }

    const rows = mergeTimestampedSeries([], samples);

    // Sorted so the legend order does not shuffle between refetches as VENs
    // come and go -- and sorted the way a person counts, since a plain sort
    // puts ven-10 second and ven-2 two thirds of the way down a twenty-entry
    // legend.
    const venNames = history.vens.map((v) => v.venName).sort(compareVenNames);
    const series: TimeSeriesSeriesSpec[] = venNames.map((name) => ({
      key: name,
      axisId: AXIS_ID,
      dataKey: (row) => row.values?.[name] ?? null,
      color: venColor(name),
      // A meter reading holds until the next one: stepAfter states that, where
      // a smooth line would invent a ramp the site never had.
      type: "stepAfter",
      connectNulls: false,
      formatter: (v: number) => `${v.toFixed(2)} kW`,
    }));

    series.push({
      key: FLEET_KEY,
      label: "fleet total",
      axisId: AXIS_ID,
      dataKey: (row) => row.values?.[FLEET_KEY] ?? null,
      color: FLEET_SUM_COLOR,
      strokeWidth: 2.5,
      type: "stepAfter",
      connectNulls: false,
      formatter: (v: number) => `${v.toFixed(2)} kW`,
    });

    const values = rows.flatMap((row) => Object.values(row.values ?? {}));
    return { rows, series, values };
  }, [history]);

  // Zones are shared across VENs on purpose: twenty overlapping shaded bands
  // would be a wall of grey. This shades the windows in which *any* targeted
  // VEN was under a signal, which is the question the overlay answers —
  // "was something in force here" — with the per-VEN detail a click away in
  // the reactions table.
  const { tMin, tMax } = fleetChartWindow(nowMs, windowMinutes);

  const zones = useMemo(() => {
    if (!signals) return [];
    const seen = new Set<string>();
    const out: { from: string; to: string; step_s: number }[] = [];
    for (const ven of signals.vens) {
      for (const band of ven.bands) {
        const key = `${band.from}|${band.to}`;
        if (seen.has(key)) continue;
        seen.add(key);
        out.push({ from: band.from, to: band.to, step_s: 0 });
      }
    }
    return out;
  }, [signals]);

  if (rows.length === 0) {
    return (
      <EmptyState
        message={`No telemetry stored for the last ${windowMinutes} minutes.`}
        testId="fleet-chart-empty"
      />
    );
  }

  return (
    <>
      <TimeSeriesChart
        data={rows}
        // The *requested* window, not the stored extent: without a fixed
        // domain recharts sizes the x-axis to whatever telemetry happens to
        // exist, and the tariff chart above could not line up with it. It also
        // means a gap in telemetry now reads as a gap rather than silently
        // rescaling time.
        tMin={tMin}
        tMax={tMax}
        axes={[
          {
            id: AXIS_ID,
            unit: "kW",
            // Fitted to the curves actually on screen, and refitted when one is
            // toggled off: with twenty VENs plus a total that is their sum, the
            // total's range is an order of magnitude larger than any single
            // site's, so a shared fixed domain flattens every VEN into a
            // few-pixel band. Hiding the total should give the sites the whole
            // axis, and that only works if the axis follows the legend.
            //
            // This gives up the guaranteed zero line. That was worth having
            // when the domain was fixed -- import above, export below is the
            // shape of a fleet's day -- but it is what was compressing the
            // per-VEN detail this chart exists to show.
            width: FLEET_AXIS_WIDTH,
            autoScale: true,
            autoScaleMinSpan: MIN_SPAN_KW,
            // Fallback only, for when nothing is drawn: keeps zero in view so an
            // empty-but-framed axis still reads as a power axis.
            domain: tightSpanDomain([...values, 0], MIN_SPAN_KW),
            tickFormatter: formatPowerTick,
          },
        ]}
        series={series}
        nowMs={nowMs}
        zones={zones}
        referenceAxisId={AXIS_ID}
        height={CHART_HEIGHT}
        xAxisTickFormatter={(ts: number) => new Date(ts).toLocaleTimeString()}
        interactiveLegend
        testId="fleet-power-chart"
      />
      {/* A 1-minute rollup and a 5-second raw series are both true and not the
          same resolution; a reader comparing two windows has to be told which
          one they are looking at. */}
      <Typography variant="caption" color="text.secondary" data-testid="fleet-chart-source">
        {history.source === "raw" ? "raw samples" : "1-minute means"}, {history.stepSeconds}s buckets
      </Typography>
    </>
  );
}
