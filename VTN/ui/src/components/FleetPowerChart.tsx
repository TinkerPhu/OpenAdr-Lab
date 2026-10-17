import { useMemo } from "react";
import { Typography } from "@mui/material";
import { TimeSeriesChart } from "@lab/ui/TimeSeriesChart";
import type { TimeSeriesSeriesSpec } from "@lab/ui/TimeSeriesChart";
import { mergeTimestampedSeries } from "@lab/ui/mergeSeries";
import type { NamedSample } from "@lab/ui/mergeSeries";
import { tightSpanDomain, formatPowerTick } from "@lab/ui/axisDomain";
import { EmptyState } from "@lab/ui/EmptyState";
import { CELL_CHART_HEIGHT } from "@lab/ui/chartLayout";
import {
  fleetChartWindow,
  fleetChartTicks,
  FLEET_AXIS_WIDTH,
  FLEET_TICK_INTERVAL,
} from "./fleetChartWindow";
import { renderDayNightShading } from "@lab/ui/DayNightShading";
import { LAB_LOCATION } from "../utils/labLocation";

import type { FleetForecast, FleetForecastSlot } from "../api/types";
import type { FleetHistory } from "../api/types";
import { venColor, FLEET_SUM_COLOR } from "../utils/venColor";
import { compareVenNames } from "../utils/venOrder";
import { objectiveMix } from "../utils/fleetObjective";

/** The series key the fleet total is drawn under. Prefixed so it can never
 *  collide with a VEN actually named "fleet". */
export const FLEET_KEY = "__fleet__";
/** The fleet forecast and its pre-change counterpart (openspec `opportunistic-objective` D12). */
export const FORECAST_KEY = "__forecast__";
export const BEFORE_KEY = "__before__";

/** One dashed overlay of the fleet total: the forecast, or the forecast from before the last
 *  objective change. Declared as data so the two differ only in what they say. */
function forecastSeries(
  key: string,
  label: string,
  look: Pick<TimeSeriesSeriesSpec, "strokeDasharray" | "strokeOpacity" | "strokeWidth">,
): TimeSeriesSeriesSpec {
  return {
    key,
    label,
    axisId: AXIS_ID,
    dataKey: (row) => row.values?.[key] ?? null,
    color: FLEET_SUM_COLOR,
    type: "stepAfter",
    connectNulls: false,
    formatter: (v: number) => `${v.toFixed(2)} kW`,
    ...look,
  };
}

function forecastSamples(key: string, slots: FleetForecastSlot[]): NamedSample[] {
  return slots.map((s) => ({ ts: Date.parse(s.start), key, value: s.kw }));
}

const AXIS_ID = "kw";

/** Three times the standard cell height. Twenty overlapping curves need the
 *  vertical room to be told apart at all — at cell height they are a band, and
 *  "which site moved" is unanswerable. */
const CHART_HEIGHT = CELL_CHART_HEIGHT * 3;

/** Narrowest kW span the axis will scale down to. A fleet sitting still should
 *  read as sitting still, not as amplified rounding noise. */
const MIN_SPAN_KW = 1;

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
  windowMinutes,
  tickMinutes,
  nowMs,
  reportingVens,
  forecast,
  aheadMinutes = 0,
}: {
  history: FleetHistory;
  /** The fleet forecast to draw ahead of now, dashed, with its pre-change counterpart. */
  forecast?: FleetForecast;
  /** Minutes of future both Fleet charts show (`fleetAheadMinutes`). */
  aheadMinutes?: number;
  windowMinutes: number;
  /** VENs reporting right now (the live feed's `contributingVens`). It is what separates a store
   * that is still filling from one nothing is writing to: both resample to zero rows. */
  reportingVens: number;
  /** Tick spacing, chosen per window by the page so both charts agree. */
  tickMinutes: number;
  nowMs: number;
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

  const { rows, series, values } = useMemo(() => {
    const samples: NamedSample[] = [];

    for (const ven of history.vens) {
      for (const s of ven.samples) {
        samples.push({ ts: Date.parse(s.ts), key: ven.venName, value: s.netPowerKw });
      }
    }
    for (const point of history.fleet) {
      samples.push({ ts: Date.parse(point.ts), key: FLEET_KEY, value: point.netPowerKw });
    }
    samples.push(...forecastSamples(FORECAST_KEY, forecast?.slots ?? []));
    samples.push(...forecastSamples(BEFORE_KEY, forecast?.before?.slots ?? []));

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

    if (forecast && forecast.slots.length > 0) {
      // The legend names the objectives behind the soonest slot, so a reader sees what the
      // dashed line assumes without hovering it.
      const mix = objectiveMix(forecast.slots[0].objectives);
      series.push(forecastSeries(FORECAST_KEY, `forecast (${mix})`, { strokeDasharray: "6 4", strokeWidth: 2 }));
    }
    if (forecast?.before && forecast.before.slots.length > 0) {
      const at = forecast.before.changed_at ? new Date(forecast.before.changed_at).toLocaleTimeString() : "";
      series.push(
        forecastSeries(BEFORE_KEY, `forecast before the change ${at}`.trim(), {
          strokeDasharray: "2 4",
          strokeOpacity: 0.45,
        }),
      );
    }

    const values = rows.flatMap((row) => Object.values(row.values ?? {}));
    return { rows, series, values };
  }, [history, forecast]);

  if (rows.length === 0) {
    // Two different causes of the same empty array, and only the second is "no telemetry": the
    // page opens on 24 h at 900 s steps, so a freshly started store holds less than one bucket.
    if (reportingVens > 0) {
      return (
        <EmptyState
          message={`${reportingVens} ${reportingVens === 1 ? "VEN is" : "VENs are"} reporting, but the store is still collecting - this window has no complete bucket yet.`}
          testId="fleet-chart-collecting"
        />
      );
    }
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
        referenceAxisId={AXIS_ID}
        backgroundAreas={dayNight}
        xAxisTicks={ticks}
        xAxisInterval={FLEET_TICK_INTERVAL}
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
      {/* `wire-contracts`: a VEN whose report declared no unit was read in the profile's kWh,
          and that assumption is shown, not silent. */}
      {forecast && forecast.assumedUnitVens.length > 0 && (
        <Typography variant="caption" color="warning.main" display="block" data-testid="fleet-forecast-assumed">
          Forecast unit assumed (kWh) for {forecast.assumedUnitVens.join(", ")}: their reports declare none.
        </Typography>
      )}
    </>
  );
}
