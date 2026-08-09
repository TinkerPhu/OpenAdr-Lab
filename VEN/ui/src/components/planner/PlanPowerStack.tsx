import { useMemo } from "react";
import { StackedTimeSeriesChart } from "../charts/StackedTimeSeriesChart";
import type { AssetId } from "../controller/types";
import { ASSET_COLORS } from "../controller/types";
import { buildStackedFromAllTimelines } from "../controller/GridAccumulatedCell";
import type { Plan } from "../../api/types";
import { useAllTimelines } from "../../api/hooks";
import { Box, Typography } from "@mui/material";

const RENDER_ORDER: AssetId[] = ["base_load", "ev", "wm", "heater", "battery", "pv"];

interface PlanPowerStackProps {
  plan: Plan | null | undefined;
}

export function PlanPowerStack({ plan }: PlanPowerStackProps) {
  // eslint-disable-next-line react-hooks/purity -- intentional: snapshot current time for the NOW line; component re-renders on poll
  const nowMs = Date.now();
  const lastEnd = plan?.slots[plan.slots.length - 1]?.end;
  // Memoized on the plan's own horizon (lastEnd), not recomputed from Date.now() on
  // every render: this value feeds useAllTimelines()'s query key below, so letting it
  // drift by milliseconds on every render (as it harmlessly did when it was only a
  // chart-sizing prop) minted a new query key — and triggered a new fetch — on every
  // render, including the frequent re-renders SSE solving_progress events cause during
  // a solve. Recomputing only when the plan horizon actually changes fixes that without
  // losing correctness: the window is always >= the remaining plan horizon.
  const hoursForward = useMemo(() => {
    // Recomputed only when the plan horizon (lastEnd) changes, not on every
    // render; see comment above.
    // eslint-disable-next-line react-hooks/purity -- intentional, see above
    const now = Date.now();
    const tMax = lastEnd ? new Date(lastEnd).getTime() : now + 12 * 3_600_000;
    return Math.max(0.5, (tMax - now) / 3_600_000);
  }, [lastEnd]);

  // Same backend-computed timeline the Controller tab's Accumulated Power chart
  // uses (net_import_kw - net_export_kw, correctly signed) — hoursBack: 0 keeps
  // this chart forecast-only, the one intentional difference from Controller's.
  const { data: allTimelinesResponse } = useAllTimelines(0, hoursForward);
  const allTimelines = useMemo(
    () => allTimelinesResponse?.timelines ?? {},
    [allTimelinesResponse]
  );

  if (!plan || plan.slots.length === 0) {
    return (
      <Box sx={{ py: 2 }}>
        <Typography variant="body2" color="text.secondary">
          No plan data available.
        </Typography>
      </Box>
    );
  }

  const points = buildStackedFromAllTimelines(allTimelines);
  const assetIds = RENDER_ORDER.filter((id) => (allTimelines[id]?.length ?? 0) > 0);

  const curtailedSlots = plan.slots.filter(
    (slot) => (slot.pv_forecast_kw - (slot.pv_used_kw ?? slot.pv_forecast_kw)) > 0.05,
  );

  return (
    <Box data-testid="plan-power-stack" sx={{ width: "100%", height: 340 }}>
      <Typography variant="subtitle2" color="text.secondary" gutterBottom>
        Power Stack — Forecast vs Plan
      </Typography>
      {curtailedSlots.length > 0 && (
        <Typography
          data-testid="pv-curtailment-indicator"
          variant="caption"
          color="warning.main"
          display="block"
        >
          PV curtailed in {curtailedSlots.length} upcoming slot
          {curtailedSlots.length === 1 ? "" : "s"} (peak −
          {Math.max(
            ...curtailedSlots.map((s) => s.pv_forecast_kw - (s.pv_used_kw ?? s.pv_forecast_kw)),
          ).toFixed(2)}{" "}
          kW)
        </Typography>
      )}
      <StackedTimeSeriesChart
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
