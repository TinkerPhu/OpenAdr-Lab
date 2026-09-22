import { useMemo } from "react";
import { Typography } from "@mui/material";
import { TimeSeriesChart } from "@lab/charts/TimeSeriesChart";
import type { TimeSeriesSeriesSpec } from "@lab/charts/TimeSeriesChart";
import { mergeTimestampedSeries } from "@lab/charts/mergeSeries";
import type { NamedSample } from "@lab/charts/mergeSeries";
import { tightSpanDomain, formatPowerTick } from "@lab/charts/axisDomain";
import { EmptyState } from "@lab/charts/EmptyState";
import type { FleetHistory } from "../api/types";
import { venColor, FLEET_SUM_COLOR } from "../utils/venColor";

/** The series key the fleet total is drawn under. Prefixed so it can never
 *  collide with a VEN actually named "fleet". */
export const FLEET_KEY = "__fleet__";

const AXIS_ID = "kw";

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
  windowMinutes,
  nowMs,
}: {
  history: FleetHistory;
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

    // Sorted by name so the legend order does not shuffle between refetches as
    // VENs come and go.
    const venNames = [...history.vens].map((v) => v.venName).sort();
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
        axes={[
          {
            id: AXIS_ID,
            unit: "kW",
            // Import above the line and export below is the shape of a fleet's
            // day, so the domain always spans zero even when every site is
            // importing.
            domain: tightSpanDomain([...values, 0], 1),
            tickFormatter: formatPowerTick,
          },
        ]}
        series={series}
        nowMs={nowMs}
        referenceAxisId={AXIS_ID}
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
