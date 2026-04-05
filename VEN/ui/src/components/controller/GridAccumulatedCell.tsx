import { useMemo } from "react";
import { Box, IconButton, Paper, Tooltip } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import { CELL_CHART_MIN_WIDTH } from "./chartLayout";
import type { AssetId, AssetSummary, AssetTimelinePoint, StackedAreaPoint } from "./types";
import { ASSET_COLORS } from "./types";
import { StackedAreaChart } from "./charts/StackedAreaChart";

const DEFAULT_WINDOW = { hoursBack: 1.0, hoursForward: 1.0 };
const EXTENDED_WINDOW = { hoursBack: 1.0, hoursForward: 24.0 };

const KNOWN_ASSETS: AssetId[] = ["ev", "heater", "pv", "battery", "base_load"];

/** Build stacked-area data by positional zip across grid-aligned asset arrays. */
export function buildStackedFromAllTimelines(
  allTimelines: Record<string, AssetTimelinePoint[]>
): StackedAreaPoint[] {
  // Use the first known asset's array to determine length and timestamps.
  // RF-05c guarantees all assets share the same ts at each index.
  const refAsset = KNOWN_ASSETS.find((id) => (allTimelines[id]?.length ?? 0) > 0);
  const refPoints = refAsset ? allTimelines[refAsset] : [];
  if (!refPoints || refPoints.length === 0) return [];

  return refPoints.map((ref, i) => {
    const pt: StackedAreaPoint = {
      ts: ref.ts,
      ev_pos: 0, ev_neg: 0,
      heater_pos: 0, heater_neg: 0,
      pv_pos: 0, pv_neg: 0,
      battery_pos: 0, battery_neg: 0,
      base_load_pos: 0, base_load_neg: 0,
      gridPowerKw: null,
    };
    for (const assetId of KNOWN_ASSETS) {
      const kw = allTimelines[assetId]?.[i]?.values?.["power_kw"] ?? 0;
      pt[`${assetId}_pos` as keyof StackedAreaPoint] = Math.max(0, kw) as never;
      pt[`${assetId}_neg` as keyof StackedAreaPoint] = Math.min(0, kw) as never;
    }
    pt.gridPowerKw = allTimelines["grid"]?.[i]?.values?.["power_kw"] ?? null;
    return pt;
  });
}

interface GridAccumulatedCellProps {
  assetSummaries: AssetSummary[];
  /** Timeline data from the shared useAllTimelines query on the page. */
  allTimelines: Record<string, AssetTimelinePoint[]>;
  /** Epoch ms — shared across all cells from the page for a consistent NOW line. */
  nowMs: number;
  /** Whether this cell's time window is expanded to 24h forward. */
  extended: boolean;
  pinned: boolean;
  onTogglePin: () => void;
}

export function GridAccumulatedCell({
  assetSummaries,
  allTimelines,
  nowMs,
  extended,
  pinned,
  onTogglePin,
}: GridAccumulatedCellProps) {
  const window = extended ? EXTENDED_WINDOW : DEFAULT_WINDOW;
  const tMin = nowMs - window.hoursBack * 3_600_000;
  const tMax = nowMs + window.hoursForward * 3_600_000;

  const stackedAreaPoints = useMemo(() => {
    const all = buildStackedFromAllTimelines(allTimelines);
    return all.filter((p) => p.ts >= tMin && p.ts <= tMax);
  }, [allTimelines, tMin, tMax]);

  const assetIds = assetSummaries.map((s) => s.assetId);

  return (
    <Paper
      variant="outlined"
      data-testid="grid-accumulated-cell"
      sx={{ display: "flex", flexDirection: "row", mb: 1, borderLeft: "4px solid #546e7a" }}
    >
      {/* Chart */}
      <Box sx={{ flex: 1, minWidth: CELL_CHART_MIN_WIDTH }}>
        <StackedAreaChart
          data={stackedAreaPoints}
          assetIds={assetIds as AssetId[]}
          colorMap={ASSET_COLORS}
          nowMs={nowMs}
          hoursBack={window.hoursBack}
          hoursForward={window.hoursForward}
        />
      </Box>

      {/* Right column: pin button */}
      <Box sx={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
        <Tooltip title={pinned ? "Unpin" : "Pin to top"}>
          <IconButton
            size="small"
            data-testid="grid-accumulated-cell-pin-btn"
            onClick={onTogglePin}
            sx={{ m: 0.5 }}
          >
            {pinned ? <PushPinIcon fontSize="small" /> : <PushPinOutlinedIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
      </Box>
    </Paper>
  );
}
