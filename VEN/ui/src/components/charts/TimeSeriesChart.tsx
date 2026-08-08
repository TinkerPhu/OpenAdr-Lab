import type { ReactElement } from "react";
import {
  ComposedChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from "recharts";
import type { ZoneDef } from "../../api/types";
import { renderNowLine } from "./NowLine";
import { renderZoneShading } from "./ZoneShading";
import { TOOLTIP_CONTENT_STYLE, TOOLTIP_ITEM_STYLE, TOOLTIP_LABEL_STYLE } from "./tooltipStyle";
import { CELL_CHART_HEIGHT } from "./chartLayout";
import type { TimestampedRow } from "./mergeSeries";

export interface TimeSeriesAxisSpec {
  id: string;
  orientation?: "left" | "right";
  unit?: string;
  domain: [number, number];
  /** From `zeroAnchoredTicks` — `undefined` defers to recharts' own tick generation. */
  ticks?: number[];
  tickFormatter?: (v: number) => string;
  width?: number;
  /** Renders with no visible axis line/ticks — carries a series (e.g. SoC, T_tank) whose
   * value should only ever appear in the tooltip, never as its own drawn axis. */
  hidden?: boolean;
}

export interface TimeSeriesSeriesSpec {
  /** React key AND the `name` recharts reports to the tooltip/legend for this series. */
  key: string;
  axisId: string;
  /** Reads from the single merged `data` row passed to the chart — see mergeSeries.ts.
   * A string dataKey works for plain (non-nested) row shapes; an accessor function is
   * required when the row's values live in a nested map (`row.values?.[k]`). Never point
   * this at any array/data source other than the one `data` prop every other series here
   * also reads from — that's the whole cursor-correctness invariant this component exists
   * to enforce structurally. */
  dataKey: string | ((row: TimestampedRow) => number | null | undefined);
  color: string;
  strokeWidth?: number;
  strokeDasharray?: string;
  strokeOpacity?: number;
  type?: "stepAfter" | "monotone" | "linear";
  connectNulls?: boolean;
}

interface TimeSeriesChartProps {
  /** Pre-merged, timestamp-keyed rows — build with `mergeTimestampedSeries`/`locfFillKeys`
   * before passing in. This component never accepts a second, independent data array for
   * any individual series (see `TimeSeriesSeriesSpec.dataKey`'s doc comment). */
  data: TimestampedRow[];
  /** Omit both to let recharts auto-fit the X domain to the data (for charts with no
   * live "now" concept, e.g. a planned-rates viewer) instead of a fixed window. */
  tMin?: number;
  tMax?: number;
  xAxisTickFormatter: (ts: number) => string;
  xAxisTicks?: number[];
  axes: TimeSeriesAxisSpec[];
  series: TimeSeriesSeriesSpec[];
  /** Omit to render no NOW line — only meaningful for charts with a live "now" concept. */
  nowMs?: number;
  /** Which axis the NOW line and zone shading are drawn against — any axis id works,
   * they only need one to map x-coordinates. Required whenever `nowMs` or `zones` is set. */
  referenceAxisId?: string;
  zones?: ZoneDef[];
  tooltipFormatter: (value: number, name: string) => [string, string];
  /** Chart-specific overlay `<ReferenceArea>` elements (e.g. AssetTimelineChart's PV
   * curtailment shading) that don't fit the generic zone-shading primitive — rendered
   * after zones, before the NOW line, same stacking order every consumer used before
   * migration. */
  extraReferenceAreas?: ReactElement[];
  height?: number;
  testId?: string;
  legend?: boolean;
  margin?: { top: number; right: number; left: number; bottom: number };
}

/**
 * Shared composition for every multi/single-series, time-X-axis chart in VEN/ui — see
 * openspec/changes/unified-chart-primitives/ for the duplication this replaces. Renders
 * purely from the declarative `axes`/`series` config plus the pre-merged `data`; holds no
 * chart-specific domain/formatting logic itself (that lives in each caller, using the
 * shared axisDomain.ts/unitFormat.ts/mergeSeries.ts primitives).
 */
export function TimeSeriesChart({
  data,
  tMin,
  tMax,
  xAxisTickFormatter,
  xAxisTicks,
  axes,
  series,
  nowMs,
  referenceAxisId,
  zones,
  tooltipFormatter,
  extraReferenceAreas,
  height = CELL_CHART_HEIGHT,
  testId,
  legend = true,
  margin = { top: 4, right: 4, left: 0, bottom: 0 },
}: TimeSeriesChartProps) {
  return (
    <div data-testid={testId} style={{ width: "100%", height }}>
      <ResponsiveContainer width="100%" height="100%">
        <ComposedChart data={data} margin={margin}>
          <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
          <XAxis
            dataKey="ts"
            scale="time"
            type="number"
            domain={tMin !== undefined && tMax !== undefined ? [tMin, tMax] : ["auto", "auto"]}
            ticks={xAxisTicks}
            tickFormatter={xAxisTickFormatter}
            tick={{ fontSize: 10 }}
          />
          {axes.map((axis) =>
            axis.hidden ? (
              <YAxis
                key={axis.id}
                yAxisId={axis.id}
                axisLine={false}
                tickLine={false}
                tick={false}
                width={0}
                domain={axis.domain}
              />
            ) : (
              <YAxis
                key={axis.id}
                yAxisId={axis.id}
                orientation={axis.orientation ?? "left"}
                tick={{ fontSize: 10 }}
                width={axis.width ?? 46}
                unit={axis.unit}
                tickFormatter={axis.tickFormatter}
                domain={axis.domain}
                ticks={axis.ticks}
              />
            )
          )}
          <Tooltip
            contentStyle={TOOLTIP_CONTENT_STYLE}
            itemStyle={TOOLTIP_ITEM_STYLE}
            labelStyle={TOOLTIP_LABEL_STYLE}
            labelFormatter={(v) => new Date(v as number).toLocaleTimeString()}
            formatter={tooltipFormatter}
          />
          {legend && <Legend iconSize={10} wrapperStyle={{ fontSize: 10 }} />}

          {series.map((s) => (
            <Line
              key={s.key}
              yAxisId={s.axisId}
              type={s.type ?? "stepAfter"}
              dataKey={s.dataKey}
              name={s.key}
              stroke={s.color}
              strokeWidth={s.strokeWidth ?? 1.5}
              strokeDasharray={s.strokeDasharray}
              strokeOpacity={s.strokeOpacity}
              dot={false}
              connectNulls={s.connectNulls ?? false}
              isAnimationActive={false}
            />
          ))}

          {referenceAxisId && renderZoneShading(referenceAxisId, zones)}
          {extraReferenceAreas}
          {referenceAxisId && nowMs !== undefined && renderNowLine(referenceAxisId, nowMs)}
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  );
}
