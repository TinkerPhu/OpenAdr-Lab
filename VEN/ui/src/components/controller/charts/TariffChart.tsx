import { CELL_CHART_HEIGHT } from "../../charts/chartLayout";
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
import type { TariffTimePoint } from "../types";
import { SERIES_COLORS } from "../types";
import type { ZoneDef } from "../../../api/types";
import {
  minSpanDomain,
  tightSpanDomain,
  MIN_CO2_RATE_SPAN_G_H,
  MIN_COST_RATE_SPAN_EUR_H,
  MIN_TARIFF_SPAN_EUR_KWH,
  roundedTimeTicks,
  zeroAnchoredTicks,
} from "../../charts/axisDomain";
import { formatCo2RateGH, formatCostRateEurH, formatTariffEurKwh } from "../../charts/unitFormat";
import { renderNowLine } from "../../charts/NowLine";
import { renderZoneShading } from "../../charts/ZoneShading";
import { TOOLTIP_CONTENT_STYLE, TOOLTIP_ITEM_STYLE, TOOLTIP_LABEL_STYLE } from "../../charts/tooltipStyle";

const COLOR_IMPORT_TARIFF = SERIES_COLORS.import_tariff;
const COLOR_EXPORT_TARIFF = SERIES_COLORS.export_tariff;
const COLOR_COST_RATE     = SERIES_COLORS.cost_rate;
const COLOR_CO2_RATE      = SERIES_COLORS.co2_rate;

interface TariffChartProps {
  data: TariffTimePoint[];
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
  height?: number;
  zones?: ZoneDef[];
  /** X-axis ticks every N minutes, snapped to the wall-clock (10:00, 10:30, ...) instead of
   * recharts' default "nice" ticks. History page only — Controller's real-time cells keep the
   * default behavior. */
  xAxisTickIntervalMinutes?: number;
}

function formatTs(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function TariffChart({
  data,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  height,
  zones,
  xAxisTickIntervalMinutes,
}: TariffChartProps) {
  // Domain driven by hoursBack/hoursForward keeps the X-axis stable and ensures the
  // NOW reference line is always visible even when past tariff data is absent.
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;
  const xAxisTicks = xAxisTickIntervalMinutes
    ? roundedTimeTicks(tMin, tMax, xAxisTickIntervalMinutes)
    : undefined;

  // Clip data to [tMin, tMax]. recharts does not clip rendered data to the XAxis domain —
  // without this the chart auto-scales to the full data extent (e.g. 6×24h from /tariffs).
  // Keep the last point before tMin as a left anchor so stepAfter lines start at the
  // correct value at the left edge of the window.
  const clipped = (() => {
    const upToEnd = data.filter((p) => p.ts <= tMax);
    // Pin the left-anchor to tMin so recharts never sees a data point outside
    // [tMin, tMax]. Without this, recharts expands the X-axis domain to fit the
    // anchor's original timestamp, shifting the NOW line rightward relative to
    // all other charts that share the same domain.
    const lastBefore = upToEnd.filter((p) => p.ts < tMin).slice(-1)
      .map((p) => ({ ...p, ts: tMin }));
    const inWindow = upToEnd.filter((p) => p.ts >= tMin);
    const windowed = [...lastBefore, ...inWindow];

    // Carry-forward the last known tariff prices to tMax. The merged dataset contains
    // power points (gridTimeline) with null tariff fields after the last tariff snapshot.
    // connectNulls=false stops the stepAfter line at the last non-null value rather than
    // extending to the right edge — a sentinel at tMax prevents this gap.
    const lastTariff = [...windowed].reverse().find(
      (p) => p.importPriceEurKwh !== null || p.exportPriceEurKwh !== null || p.co2GKwh !== null
    );
    if (lastTariff) {
      windowed.push({
        ts: tMax,
        importPriceEurKwh: lastTariff.importPriceEurKwh,
        exportPriceEurKwh: lastTariff.exportPriceEurKwh,
        co2GKwh: lastTariff.co2GKwh,
        totalCostRateEurH: null,
        totalCo2RateGH: null,
        gridPowerKw: null,
      });
    }

    return windowed;
  })();

  const co2Domain = minSpanDomain(
    clipped.map((p) => p.totalCo2RateGH ?? null),
    MIN_CO2_RATE_SPAN_G_H
  );
  // Tariff (€/kWh) and cost rate (€/h) are different physical dimensions and must not
  // share a Y-axis — plotting them together previously let cost rate's range flatten the
  // tariff curves whenever the two magnitudes diverged. tightSpanDomain (not minSpanDomain)
  // — tariff is a strictly-positive price, so anchoring the domain at 0 would still
  // compress a narrow real range (e.g. 0.28-0.32) into a sliver of the axis, undoing the
  // point of splitting the axis out in the first place.
  const tariffDomain = tightSpanDomain(
    clipped.flatMap((p) => [p.importPriceEurKwh, p.exportPriceEurKwh]),
    MIN_TARIFF_SPAN_EUR_KWH
  );
  const costDomain = minSpanDomain(
    clipped.map((p) => p.totalCostRateEurH ?? null),
    MIN_COST_RATE_SPAN_EUR_H
  );

  // Ensure at least a 2-point range so recharts can render the NOW line when data is empty.
  const chartData: TariffTimePoint[] =
    clipped.length > 0
      ? clipped
      : [
          { ts: tMin, importPriceEurKwh: null, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: null, totalCo2RateGH: null, gridPowerKw: null },
          { ts: tMax, importPriceEurKwh: null, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: null, totalCo2RateGH: null, gridPowerKw: null },
        ];

  return (
    <div data-testid="tariff-chart" style={{ width: "100%", height: height ?? CELL_CHART_HEIGHT }}>
      <ResponsiveContainer width="100%" height="100%">
        <ComposedChart data={chartData} margin={{ top: 4, right: 40, left: 0, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
          <XAxis
            dataKey="ts"
            scale="time"
            type="number"
            domain={[tMin, tMax]}
            ticks={xAxisTicks}
            tickFormatter={formatTs}
            tick={{ fontSize: 10 }}
          />
          <YAxis
            yAxisId="tariff"
            tick={{ fontSize: 10 }}
            width={48}
            unit=" €/kWh"
            domain={tariffDomain}
            ticks={zeroAnchoredTicks(tariffDomain)}
          />
          <YAxis
            yAxisId="cost"
            orientation="right"
            tick={{ fontSize: 10 }}
            width={48}
            unit=" €/h"
            domain={costDomain}
            ticks={zeroAnchoredTicks(costDomain)}
          />
          <YAxis
            yAxisId="co2"
            orientation="right"
            tick={{ fontSize: 10 }}
            width={52}
            unit=" g/h"
            domain={co2Domain}
            ticks={zeroAnchoredTicks(co2Domain)}
          />
          <Tooltip
            contentStyle={TOOLTIP_CONTENT_STYLE}
            itemStyle={TOOLTIP_ITEM_STYLE}
            labelStyle={TOOLTIP_LABEL_STYLE}
            labelFormatter={(v) => new Date(v as number).toLocaleTimeString()}
            formatter={(value: number, name: string) => {
              if (name === "CO₂ rate [g/h]") return [formatCo2RateGH(value), name];
              if (name === "Cost rate [€/h]") return [formatCostRateEurH(value), name];
              return [formatTariffEurKwh(value), name];
            }}
          />
          <Legend iconSize={10} wrapperStyle={{ fontSize: 10 }} />

          {/* Import tariff [€/kWh] — red dashed */}
          <Line
            yAxisId="tariff"
            type="stepAfter"
            dataKey="importPriceEurKwh"
            name="Import tariff [€/kWh]"
            stroke={COLOR_IMPORT_TARIFF}
            strokeDasharray="5 5"
            strokeWidth={1.5}
            dot={false}
            connectNulls={true}
            isAnimationActive={false}
          />

          {/* Export tariff [€/kWh] — green dashed */}
          <Line
            yAxisId="tariff"
            type="stepAfter"
            dataKey="exportPriceEurKwh"
            name="Export tariff [€/kWh]"
            stroke={COLOR_EXPORT_TARIFF}
            strokeDasharray="5 5"
            strokeWidth={1.5}
            dot={false}
            connectNulls={true}
            isAnimationActive={false}
          />

          {/* Total cost rate [€/h] — near-black dashed, own axis (distinct dimension from tariff) */}
          <Line
            yAxisId="cost"
            type="stepAfter"
            dataKey="totalCostRateEurH"
            name="Cost rate [€/h]"
            stroke={COLOR_COST_RATE}
            strokeDasharray="5 5"
            strokeWidth={1.5}
            dot={false}
            connectNulls={true}
            isAnimationActive={false}
          />

          {/* CO₂ rate [g/h] — orange dotted, right axis; negative when exporting */}
          <Line
            yAxisId="co2"
            type="stepAfter"
            dataKey="totalCo2RateGH"
            name="CO₂ rate [g/h]"
            stroke={COLOR_CO2_RATE}
            strokeDasharray="2 2"
            strokeWidth={1.5}
            dot={false}
            connectNulls={true}
            isAnimationActive={false}
          />

          {/* Zone background shading — rendered before data lines so they sit behind */}
          {renderZoneShading("tariff", zones)}

          {/* NOW reference line */}
          {renderNowLine("tariff", nowMs)}
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  );
}
