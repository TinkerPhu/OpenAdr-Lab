import {
  ComposedChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ReferenceLine,
  Legend,
  ResponsiveContainer,
} from "recharts";
import type { TariffTimePoint } from "../types";

interface TariffChartProps {
  data: TariffTimePoint[];
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
}

function formatTs(ts: number) {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function TariffChart({ data, nowMs, hoursBack = 1.0, hoursForward = 1.0 }: TariffChartProps) {
  // Domain driven by hoursBack/hoursForward keeps the X-axis stable and ensures the
  // NOW reference line is always visible even when past tariff data is absent.
  const tMin = nowMs - hoursBack * 3_600_000;
  const tMax = nowMs + hoursForward * 3_600_000;

  // Clip data to [tMin, tMax]. recharts does not clip rendered data to the XAxis domain —
  // without this the chart auto-scales to the full data extent (e.g. 6×24h from /tariffs).
  // Keep the last point before tMin as a left anchor so stepAfter lines start at the
  // correct value at the left edge of the window.
  const clipped = (() => {
    const upToEnd = data.filter((p) => p.ts <= tMax);
    const lastBefore = upToEnd.filter((p) => p.ts < tMin).slice(-1);
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
        gridPowerKw: null,
      });
    }

    return windowed;
  })();

  // Ensure at least a 2-point range so recharts can render the NOW line when data is empty.
  const chartData: TariffTimePoint[] =
    clipped.length > 0
      ? clipped
      : [
          { ts: tMin, importPriceEurKwh: null, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: null, gridPowerKw: null },
          { ts: tMax, importPriceEurKwh: null, exportPriceEurKwh: null, co2GKwh: null, totalCostRateEurH: null, gridPowerKw: null },
        ];

  return (
    <div data-testid="tariff-chart" style={{ width: "100%", height: 160 }}>
      <ResponsiveContainer width="100%" height="100%">
        <ComposedChart data={chartData} margin={{ top: 4, right: 8, left: 0, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
          <XAxis
            dataKey="ts"
            scale="time"
            type="number"
            domain={[tMin, tMax]}
            tickFormatter={formatTs}
            tick={{ fontSize: 10 }}
          />
          <YAxis yAxisId="tariff" tick={{ fontSize: 10 }} width={40} />
          <YAxis yAxisId="power" orientation="right" tick={{ fontSize: 10 }} width={40} />
          <Tooltip
            labelFormatter={(v) => new Date(v as number).toLocaleTimeString()}
            formatter={(value: number, name: string) => [value?.toFixed(4), name]}
          />
          <Legend iconSize={10} wrapperStyle={{ fontSize: 10 }} />

          {/* Import tariff [€/kWh] — red dashed */}
          <Line
            yAxisId="tariff"
            type="stepAfter"
            dataKey="importPriceEurKwh"
            name="Import tariff [€/kWh]"
            stroke="#f44336"
            strokeDasharray="5 5"
            strokeWidth={1.5}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
          />

          {/* Import CO₂eq tariff [g CO₂eq/kWh] — red dotted */}
          <Line
            yAxisId="tariff"
            type="stepAfter"
            dataKey="co2GKwh"
            name="CO₂eq tariff [g/kWh]"
            stroke="#f44336"
            strokeDasharray="2 2"
            strokeWidth={1.5}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
          />

          {/* Export tariff [€/kWh] — green dashed */}
          <Line
            yAxisId="tariff"
            type="stepAfter"
            dataKey="exportPriceEurKwh"
            name="Export tariff [€/kWh]"
            stroke="#4caf50"
            strokeDasharray="5 5"
            strokeWidth={1.5}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
          />

          {/* Total cost rate [€/h] — black dashed */}
          <Line
            yAxisId="tariff"
            type="stepAfter"
            dataKey="totalCostRateEurH"
            name="Cost rate [€/h]"
            stroke="#212121"
            strokeDasharray="5 5"
            strokeWidth={1.5}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
          />

          {/* Grid power [kW] — black solid */}
          <Line
            yAxisId="power"
            type="stepAfter"
            dataKey="gridPowerKw"
            name="Grid power [kW]"
            stroke="#212121"
            strokeWidth={2}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
          />

          {/* NOW reference line */}
          <ReferenceLine
            yAxisId="tariff"
            x={nowMs}
            stroke="#f44336"
            strokeDasharray="3 3"
            label={{ value: "NOW", position: "top", fontSize: 9, fill: "#f44336" }}
          />
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  );
}
