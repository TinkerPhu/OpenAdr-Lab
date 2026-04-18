import { useMemo, useState } from "react";
import { Box, Divider, IconButton, Paper, Tooltip, Typography } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import UnfoldMoreIcon from "@mui/icons-material/UnfoldMore";
import UnfoldLessIcon from "@mui/icons-material/UnfoldLess";
import { CELL_CHART_MIN_WIDTH, CELL_LEFT_SECTION_WIDTH, DEFAULT_WINDOW, EXTENDED_WINDOW, CELL_CHART_HEIGHT_TALL } from "./chartLayout";
import type { AssetId, AssetSummary, AssetTimelinePoint, StackedAreaPoint } from "./types";
import { ASSET_COLORS } from "./types";
import { StackedAreaChart } from "./charts/StackedAreaChart";

/** Discover all asset IDs present in the timelines (everything except "grid"). */
function discoverAssetIds(allTimelines: Record<string, AssetTimelinePoint[]>): AssetId[] {
  return Object.keys(allTimelines).filter((id) => id !== "grid");
}

/** Build stacked-area data by positional zip across grid-aligned asset arrays. */
export function buildStackedFromAllTimelines(
  allTimelines: Record<string, AssetTimelinePoint[]>
): StackedAreaPoint[] {
  const assetIds = discoverAssetIds(allTimelines);
  // Use the first non-empty asset's array to determine length and timestamps.
  // RF-05c guarantees all assets share the same ts at each index.
  const refAsset = assetIds.find((id) => (allTimelines[id]?.length ?? 0) > 0);
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
    for (const assetId of assetIds) {
      const kw = allTimelines[assetId]?.[i]?.values?.["power_kw"] ?? 0;
      pt[`${assetId}_pos`] = Math.max(0, kw);
      pt[`${assetId}_neg`] = Math.min(0, kw);
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
  gridPowerKw: number;
  onTogglePin: () => void;
}

export function GridAccumulatedCell({
  assetSummaries,
  allTimelines,
  nowMs,
  extended,
  pinned,
  gridPowerKw,
  onTogglePin,
}: GridAccumulatedCellProps) {
  const [tall, setTall] = useState(false);
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
      {/* Left: per-asset current power list */}
      <Box sx={{ minWidth: CELL_LEFT_SECTION_WIDTH, px: 1.5, py: 1, display: "flex", flexDirection: "column", gap: 0.5 }}>
        <Typography variant="body2" fontWeight="bold">
          Accumulated Power
        </Typography>
        {assetSummaries.map((s) => (
          <Typography
            key={s.assetId}
            variant="caption"
            data-testid={`accumulated-power-${s.assetId}`}
            sx={{ color: ASSET_COLORS[s.assetId] ?? "#888" }}
          >
            {s.label}: {s.powerKw >= 0 ? "+" : ""}
            {s.powerKw.toFixed(2)} kW
          </Typography>
        ))}
        <Divider sx={{ my: 0.5 }} />
        <Typography variant="caption" color="text.secondary" data-testid="accumulated-grid-power">
          Grid: {gridPowerKw >= 0 ? "+" : ""}{gridPowerKw.toFixed(2)} kW
        </Typography>
      </Box>

      {/* Right: stacked area chart */}
      <Box sx={{ flex: 1, minWidth: CELL_CHART_MIN_WIDTH }}>
        <StackedAreaChart
          data={stackedAreaPoints}
          assetIds={assetIds as AssetId[]}
          colorMap={ASSET_COLORS}
          nowMs={nowMs}
          hoursBack={window.hoursBack}
          hoursForward={window.hoursForward}
          height={tall ? CELL_CHART_HEIGHT_TALL : undefined}
        />
      </Box>

      {/* Right column: pin button + vertical expand button */}
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
        <Tooltip title={tall ? "Collapse chart" : "Expand chart"}>
          <IconButton
            size="small"
            data-testid="grid-accumulated-cell-tall-expand-btn"
            onClick={() => setTall((v) => !v)}
            sx={{ m: 0.5 }}
          >
            {tall ? <UnfoldLessIcon fontSize="small" /> : <UnfoldMoreIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
      </Box>
    </Paper>
  );
}
