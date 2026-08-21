import type { ReactElement } from "react";
import {
  Area,
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
import { seriesHasData, type TimestampedRow } from "./mergeSeries";
import { niceAxis, tickFormatterForStep } from "./axisDomain";
import { useLegendToggle } from "./useLegendToggle";
import { ChartLegend } from "./ChartLegend";

export interface TimeSeriesAxisSpec {
  id: string;
  orientation?: "left" | "right";
  unit?: string;
  /** The *data* domain (from `minSpanDomain`/`tightSpanDomain`). This component snaps it
   * outward to round tick values itself via `niceAxis` — there is deliberately no `ticks`
   * prop, so no chart can render un-rounded Y labels by forgetting to pass one. */
  domain: [number, number];
  /** Unit-specific label formatting (e.g. `formatPowerTick`). Omit to get the step-derived
   * default from `tickFormatterForStep`, which matches the tick spacing's own precision. */
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
  /** This series' own tooltip value formatter, looked up by series identity — takes
   * precedence over the chart-level `tooltipFormatter` fallback. Declaring formatting
   * alongside the series avoids a chart-level if-chain branching on the hovered series'
   * name (see `.claude/CLAUDE.md`'s `declare-dont-branch`). */
  formatter?: (value: number) => string;
}

/** A shaded min/max range around a value, e.g. a headroom/flexibility band —
 * generic per `.claude/CLAUDE.md`'s `generic-over-bespoke` rule so any future
 * band-style chart reuses this instead of a one-off `<Area>`. */
export interface TimeSeriesBandSpec {
  key: string;
  axisId: string;
  lower: (row: TimestampedRow) => number | null | undefined;
  upper: (row: TimestampedRow) => number | null | undefined;
  color: string;
  fillOpacity?: number;
  /** This band's own tooltip value formatter, given the hovered row's [lower, upper]
   * pair — takes precedence the same way a series' own `formatter` does. Without one,
   * the tooltip falls back to a plain dash-joined pair (not `String([lo, hi])`, which
   * recharts/JS render as a bare comma-joined "lo,hi" — unreadable and, worse, easy to
   * misread as a single long number). */
  formatter?: (lower: number, upper: number) => string;
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
  /** Shaded min/max bands, rendered under every `series` line. */
  bands?: TimeSeriesBandSpec[];
  /** Omit to render no NOW line — only meaningful for charts with a live "now" concept. */
  nowMs?: number;
  /** Which axis the NOW line and zone shading are drawn against — any axis id works,
   * they only need one to map x-coordinates. Required whenever `nowMs` or `zones` is set. */
  referenceAxisId?: string;
  zones?: ZoneDef[];
  /** Fallback used only for series that don't declare their own `formatter`. */
  tooltipFormatter?: (value: number, name: string) => [string, string];
  /** Chart-specific overlay `<ReferenceArea>` elements (e.g. AssetTimelineChart's PV
   * curtailment shading) that don't fit the generic zone-shading primitive — rendered
   * after zones, before the NOW line, same stacking order every consumer used before
   * migration. */
  extraReferenceAreas?: ReactElement[];
  height?: number;
  testId?: string;
  legend?: boolean;
  /** Opt-in: renders the legend with a checkbox per series, live-toggling that series'
   * visibility (and removing it from the tooltip) — see chart_diagrams.md's "Interactive
   * legend" section. Unset/false: legend is unchanged from before this capability
   * existed. Toggle state is local to this chart instance, not persisted. */
  interactiveLegend?: boolean;
  margin?: { top: number; right: number; left: number; bottom: number };
}

/**
 * Shared composition for every multi/single-series, time-X-axis chart in VEN/ui — see
 * docs/architecture/chart_diagrams.md for the duplication this replaces. Renders
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
  bands,
  nowMs,
  referenceAxisId,
  zones,
  tooltipFormatter,
  extraReferenceAreas,
  height = CELL_CHART_HEIGHT,
  testId,
  legend = true,
  interactiveLegend = false,
  margin = { top: 4, right: 4, left: 0, bottom: 0 },
}: TimeSeriesChartProps) {
  const { isHidden, toggle } = useLegendToggle();
  // The single Y-axis tick rule for every chart built on this composition: each axis' data
  // domain is snapped to round tick values here, so no caller can forget it (`niceAxis`).
  const resolvedAxes = axes.map((axis) => ({ axis, nice: axis.hidden ? null : niceAxis(axis.domain) }));
  const visibleSeries = series.filter((s) => seriesHasData(data, s.dataKey));
  const seriesByName = new Map(visibleSeries.map((s) => [s.key, s]));
  const bandsByName = new Map((bands ?? []).map((b) => [b.key, b]));
  const resolveTooltipValue = (value: unknown, name: string): [string, string] => {
    const band = bandsByName.get(name);
    if (band && Array.isArray(value)) {
      const [lower, upper] = value as [number, number];
      return [band.formatter ? band.formatter(lower, upper) : `${lower} – ${upper}`, name];
    }
    const own = seriesByName.get(name)?.formatter;
    if (own) return [own(value as number), name];
    if (tooltipFormatter) return tooltipFormatter(value as number, name);
    return [String(value), name];
  };
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
            allowDataOverflow
            ticks={xAxisTicks}
            tickFormatter={xAxisTickFormatter}
            tick={{ fontSize: 10 }}
          />
          {resolvedAxes.map(({ axis, nice }) =>
            nice === null ? (
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
                tickFormatter={axis.tickFormatter ?? tickFormatterForStep(nice.step)}
                domain={nice.domain}
                ticks={nice.ticks}
              />
            )
          )}
          <Tooltip
            contentStyle={TOOLTIP_CONTENT_STYLE}
            itemStyle={TOOLTIP_ITEM_STYLE}
            labelStyle={TOOLTIP_LABEL_STYLE}
            labelFormatter={(v) => new Date(v as number).toLocaleTimeString()}
            formatter={resolveTooltipValue}
          />
          {legend && interactiveLegend && (
            <Legend
              content={
                <ChartLegend
                  entries={visibleSeries.map((s) => ({ key: s.key, label: s.key, color: s.color }))}
                  isHidden={isHidden}
                  toggle={toggle}
                  interactive={true}
                />
              }
            />
          )}
          {legend && !interactiveLegend && <Legend iconSize={10} wrapperStyle={{ fontSize: 10 }} />}

          {bands?.map((b) => (
            <Area
              key={b.key}
              yAxisId={b.axisId}
              type="monotone"
              dataKey={(row: TimestampedRow) => {
                const lo = b.lower(row);
                const hi = b.upper(row);
                return lo == null || hi == null ? [null, null] : [lo, hi];
              }}
              name={b.key}
              stroke="none"
              fill={b.color}
              fillOpacity={b.fillOpacity ?? 0.15}
              isAnimationActive={false}
              connectNulls={false}
            />
          ))}

          {visibleSeries.map((s) => (
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
              hide={interactiveLegend && isHidden(s.key)}
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
