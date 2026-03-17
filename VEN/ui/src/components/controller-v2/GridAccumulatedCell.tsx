import { useMemo } from "react";
import { Box, IconButton, Paper, Tooltip, Typography } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import type { AssetId, AssetSummary, StackedAreaPoint } from "./types";
import { ASSET_COLORS } from "./types";
import { StackedAreaChart } from "./charts/StackedAreaChart";
import { useAllTimelines } from "../../api/hooks";
import type { AssetTimelinePoint } from "./types";

const KNOWN_ASSETS: AssetId[] = ["ev", "heater", "pv", "battery", "base_load"];

function buildStackedFromAllTimelines(
  allTimelines: Record<string, AssetTimelinePoint[]>
): StackedAreaPoint[] {
  // Collect all unique timestamps across all asset timelines.
  const tsSet = new Set<number>();
  for (const points of Object.values(allTimelines)) {
    for (const p of points) tsSet.add(p.ts);
  }
  const sortedTs = [...tsSet].sort((a, b) => a - b);

  const emptyPt = (): Omit<StackedAreaPoint, "ts"> => ({
    ev_pos: 0, ev_neg: 0,
    heater_pos: 0, heater_neg: 0,
    pv_pos: 0, pv_neg: 0,
    battery_pos: 0, battery_neg: 0,
    base_load_pos: 0, base_load_neg: 0,
  });

  return sortedTs.map((ts) => {
    const pt: StackedAreaPoint = { ts, ...emptyPt() };
    for (const assetId of KNOWN_ASSETS) {
      const points = allTimelines[assetId];
      if (!points) continue;
      const match = points.find((p) => p.ts === ts);
      const kw = match?.values["power_kw"] ?? 0;
      const key = assetId as AssetId;
      pt[`${key}_pos` as keyof StackedAreaPoint] = Math.max(0, kw) as never;
      pt[`${key}_neg` as keyof StackedAreaPoint] = Math.min(0, kw) as never;
    }
    return pt;
  });
}

interface GridAccumulatedCellProps {
  assetSummaries: AssetSummary[];
  nowMs: number;
  pinned: boolean;
  onTogglePin: () => void;
}

export function GridAccumulatedCell({
  assetSummaries,
  nowMs,
  pinned,
  onTogglePin,
}: GridAccumulatedCellProps) {
  const { data: allTimelines = {} } = useAllTimelines();
  const stackedAreaPoints = useMemo(
    () => buildStackedFromAllTimelines(allTimelines),
    [allTimelines]
  );

  const assetIds = assetSummaries.map((s) => s.assetId);

  return (
    <Paper
      variant="outlined"
      data-testid="grid-accumulated-cell"
      sx={{ display: "flex", flexDirection: "row", mb: 1, borderLeft: "4px solid #546e7a" }}
    >
      {/* Left: per-asset current power list */}
      <Box sx={{ minWidth: 160, px: 1.5, py: 1, display: "flex", flexDirection: "column", gap: 0.5 }}>
        <Typography variant="body2" fontWeight="bold">
          Accumulated Power
        </Typography>
        {assetSummaries.map((s) => (
          <Typography
            key={s.assetId}
            variant="caption"
            data-testid={`accumulated-power-${s.assetId}`}
            sx={{ color: ASSET_COLORS[s.assetId] }}
          >
            {s.label}: {s.powerKw >= 0 ? "+" : ""}
            {s.powerKw.toFixed(2)} kW
          </Typography>
        ))}
      </Box>

      {/* Right: stacked area chart */}
      <Box sx={{ flex: 1, minWidth: 200 }}>
        <StackedAreaChart
          data={stackedAreaPoints}
          assetIds={assetIds as AssetId[]}
          colorMap={ASSET_COLORS}
          nowMs={nowMs}
        />
      </Box>

      {/* Pin button */}
      <Tooltip title={pinned ? "Unpin" : "Pin to top"}>
        <IconButton
          size="small"
          data-testid="grid-accumulated-cell-pin-btn"
          onClick={onTogglePin}
          sx={{ alignSelf: "flex-start", m: 0.5 }}
        >
          {pinned ? <PushPinIcon fontSize="small" /> : <PushPinOutlinedIcon fontSize="small" />}
        </IconButton>
      </Tooltip>
    </Paper>
  );
}
