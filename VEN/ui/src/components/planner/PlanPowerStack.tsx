import { StackedAreaChart } from "../controller/charts/StackedAreaChart";
import type { StackedAreaPoint, AssetId } from "../controller/types";
import { ASSET_COLORS } from "../controller/types";
import type { Plan } from "../../api/types";
import { Box, Typography } from "@mui/material";

const RENDER_ORDER: AssetId[] = ["base_load", "ev", "wm", "heater", "battery", "pv"];

function buildStackedFromPlan(plan: Plan): { points: StackedAreaPoint[]; assetIds: AssetId[] } {
  const present = new Set<string>(["base_load", "pv"]);
  for (const slot of plan.slots) {
    for (const key of Object.keys(slot.planned_kw_by_asset ?? {})) present.add(key);
  }
  const assetIds = RENDER_ORDER.filter((id) => present.has(id));

  const points: StackedAreaPoint[] = plan.slots.map((slot) => {
    const m = slot.planned_kw_by_asset ?? {};
    const pt: StackedAreaPoint = {
      ts: new Date(slot.start).getTime(),
      base_load_pos: slot.baseline_kw,
      base_load_neg: 0,
      pv_pos: 0,
      pv_neg: -(slot.pv_forecast_kw),
      ev_pos: 0, ev_neg: 0,
      heater_pos: 0, heater_neg: 0,
      battery_pos: 0, battery_neg: 0,
      gridPowerKw: slot.net_import_kw,
    };
    for (const id of assetIds.filter((x) => x !== "base_load" && x !== "pv")) {
      const kw = m[id] ?? 0;
      pt[`${id}_pos`] = Math.max(0, kw);
      pt[`${id}_neg`] = Math.min(0, kw);
    }
    return pt;
  });

  return { points, assetIds };
}

interface PlanPowerStackProps {
  plan: Plan | null | undefined;
}

export function PlanPowerStack({ plan }: PlanPowerStackProps) {
  if (!plan || plan.slots.length === 0) {
    return (
      <Box sx={{ py: 2 }}>
        <Typography variant="body2" color="text.secondary">
          No plan data available.
        </Typography>
      </Box>
    );
  }

  // eslint-disable-next-line react-hooks/purity -- intentional: snapshot current time relative to plan horizon; component re-renders on poll
  const nowMs = Date.now();
  const { points, assetIds } = buildStackedFromPlan(plan);
  const lastEnd = plan.slots[plan.slots.length - 1]?.end;
  const tMax = lastEnd ? new Date(lastEnd).getTime() : nowMs + 12 * 3_600_000;
  const hoursForward = Math.max(0.5, (tMax - nowMs) / 3_600_000);

  return (
    <Box data-testid="plan-power-stack" sx={{ width: "100%", height: 340 }}>
      <Typography variant="subtitle2" color="text.secondary" gutterBottom>
        Power Stack — Forecast vs Plan
      </Typography>
      <StackedAreaChart
        data={points}
        assetIds={assetIds}
        colorMap={ASSET_COLORS}
        nowMs={nowMs}
        hoursBack={0}
        hoursForward={hoursForward}
        height={300}
      />
    </Box>
  );
}
