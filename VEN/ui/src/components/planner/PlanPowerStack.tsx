import {
  ComposedChart,
  Bar,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
  ReferenceLine,
} from "recharts";
import type { TooltipProps } from "recharts";
import { useRef, useState, useEffect } from "react";
import type { Plan } from "../../api/types";
import { Box, Typography } from "@mui/material";
import { ASSET_COLORS } from "../controller/types";

// ─── Color palette — uses canonical asset colors ─────────────────────────────

const COLORS: Record<string, string> = {
  baseline: ASSET_COLORS.base_load,  // blue-grey — forecast base load
  ev: ASSET_COLORS.ev,               // blue — planned
  wm: ASSET_COLORS.wm,               // orange — planned
  heater: ASSET_COLORS.heater,       // deep-orange — planned
  bat_charge: ASSET_COLORS.battery,  // purple — planned (charging)
  bat_dis: "#CE93D8",                // light purple — planned (discharging)
  pv: ASSET_COLORS.pv,               // amber — forecast
  net_import: "#212121",             // dark — net line
};

const LABELS: Record<string, string> = {
  baseline: "Base load (forecast)",
  ev: "EV (planned)",
  wm: "Washing machine (planned)",
  heater: "Heater (planned)",
  bat_charge: "Battery charge (planned)",
  bat_dis: "Battery discharge (planned)",
  pv: "PV (forecast)",
  net_import: "Net grid import",
};

// ─── Data point shape ───────────────────────────────────────────────────────

interface StackRow {
  time: string;   // HH:mm label
  ts: number;     // epoch ms for domain
  baseline: number;
  ev: number;
  wm: number;
  heater: number;
  bat_charge: number;
  bat_dis: number;  // negative
  pv: number;       // negative
  net_import: number;
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function buildRows(plan: Plan): StackRow[] {
  return plan.slots.map((slot) => {
    const m = slot.planned_kw_by_asset ?? {};
    const batPower = m["battery"] ?? 0;
    return {
      time: formatTime(new Date(slot.start).getTime()),
      ts: new Date(slot.start).getTime(),
      baseline: slot.baseline_kw,
      ev: m["ev"] ?? 0,
      wm: m["wm"] ?? 0,
      heater: m["heater"] ?? 0,
      bat_charge: Math.max(0, batPower),
      bat_dis: Math.min(0, batPower),      // negative when discharging
      pv: -slot.pv_forecast_kw,            // negative = generation
      net_import: slot.net_import_kw,
    };
  });
}

// ─── Custom tooltip ─────────────────────────────────────────────────────────

function PowerStackTooltip({ active, payload, label }: TooltipProps<number, string>) {
  if (!active || !payload || payload.length === 0) return null;

  const time = typeof label === "number" ? formatTime(label) : label;

  return (
    <div
      style={{
        background: "rgba(255,255,255,0.96)",
        border: "1px solid #ccc",
        borderRadius: 4,
        padding: "6px 10px",
        fontSize: 12,
      }}
    >
      <div style={{ marginBottom: 4, fontWeight: "bold" }}>{time}</div>
      {payload
        .filter((e) => e.value !== 0)
        .map((entry) => {
          const key = entry.dataKey as string;
          const color = COLORS[key] ?? "#888";
          const lbl = LABELS[key] ?? key;
          const v = entry.value as number;
          return (
            <div key={key} style={{ color }}>
              {lbl}: {v >= 0 ? "+" : ""}{v.toFixed(2)} kW
            </div>
          );
        })}
    </div>
  );
}

// ─── Component ──────────────────────────────────────────────────────────────

interface PlanPowerStackProps {
  plan: Plan | null | undefined;
}

export function PlanPowerStack({ plan }: PlanPowerStackProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [barSize, setBarSize] = useState<number>(3);

  const slotCount = plan?.slots.length ?? 0;
  useEffect(() => {
    const el = containerRef.current;
    if (!el || slotCount === 0) return;
    const observer = new ResizeObserver(([entry]) => {
      const w = entry.contentRect.width;
      setBarSize(Math.max(1, Math.floor(w / slotCount)));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [slotCount]);

  if (!plan || plan.slots.length === 0) {
    return (
      <Box sx={{ py: 2 }}>
        <Typography variant="body2" color="text.secondary">
          No plan data available.
        </Typography>
      </Box>
    );
  }

  const rows = buildRows(plan);
  const nowMs = Date.now();

  // Positive-side stacked bars (load / charging)
  const loadKeys = ["baseline", "ev", "wm", "heater", "bat_charge"] as const;
  // Negative-side stacked bars (generation / discharging)
  const genKeys = ["pv", "bat_dis"] as const;

  return (
    <Box ref={containerRef} data-testid="plan-power-stack" sx={{ width: "100%", height: 340 }}>
      <Typography variant="subtitle2" color="text.secondary" gutterBottom>
        Power Stack — Forecast vs Plan
      </Typography>
      <ResponsiveContainer width="100%" height="100%">
        <ComposedChart data={rows} margin={{ top: 4, right: 16, left: 0, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" opacity={0.3} />
          <XAxis
            dataKey="ts"
            scale="time"
            type="number"
            domain={["dataMin", "dataMax"]}
            tickFormatter={formatTime}
            tick={{ fontSize: 10 }}
          />
          <YAxis tick={{ fontSize: 10 }} width={44} label={{ value: "kW", angle: -90, position: "insideLeft", style: { fontSize: 10 } }} />
          <Tooltip content={<PowerStackTooltip />} />
          <Legend iconSize={10} wrapperStyle={{ fontSize: 10 }} />

          {/* Positive stack: loads / charging */}
          {loadKeys.map((key) => (
            <Bar
              key={key}
              dataKey={key}
              name={LABELS[key]}
              stackId="load"
              fill={COLORS[key]}
              fillOpacity={0.85}
              barSize={barSize}
              isAnimationActive={false}
            />
          ))}

          {/* Negative stack: generation / discharging */}
          {genKeys.map((key) => (
            <Bar
              key={key}
              dataKey={key}
              name={LABELS[key]}
              stackId="gen"
              fill={COLORS[key]}
              fillOpacity={0.85}
              barSize={barSize}
              isAnimationActive={false}
            />
          ))}

          {/* Net import line */}
          <Line
            type="stepAfter"
            dataKey="net_import"
            name={LABELS.net_import}
            stroke={COLORS.net_import}
            strokeWidth={2}
            dot={false}
            isAnimationActive={false}
          />

          {/* NOW marker */}
          <ReferenceLine
            x={nowMs}
            stroke="#f44336"
            strokeDasharray="3 3"
            label={{ value: "NOW", position: "top", fontSize: 9, fill: "#f44336" }}
          />
        </ComposedChart>
      </ResponsiveContainer>
    </Box>
  );
}
