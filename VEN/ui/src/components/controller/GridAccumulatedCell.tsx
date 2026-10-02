import { useMemo, useState } from "react";
import { Box, Divider, IconButton, Paper, Tooltip, Typography } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import UnfoldMoreIcon from "@mui/icons-material/UnfoldMore";
import UnfoldLessIcon from "@mui/icons-material/UnfoldLess";
import {
  CELL_CHART_MIN_WIDTH, CELL_LEFT_SECTION_WIDTH, DEFAULT_WINDOW, EXTENDED_WINDOW, CELL_CHART_HEIGHT_TALL,
  DEFAULT_TICK_INTERVAL_MINUTES, EXTENDED_TICK_INTERVAL_MINUTES,
} from "@lab/charts/chartLayout";
import type { AssetId, AssetSummary, AssetTimelinePoint } from "./types";
import { ASSET_COLORS, COLOR_ASSET_FALLBACK } from "./types";
import { assetIdsWithTimelineData, buildStackedFromAllTimelines } from "./stackedTimelines";
import { StackedTimeSeriesChart } from "../charts/StackedTimeSeriesChart";
import type { ZoneDef } from "../../api/types";
import { formatSignedPowerValue } from "@lab/charts/unitFormat";

interface GridAccumulatedCellProps {
  assetSummaries: AssetSummary[];
  /** Timeline data from the shared useAllTimelines query on the page. */
  allTimelines: Record<string, AssetTimelinePoint[]>;
  /** Epoch ms — shared across all cells from the page for a consistent NOW line. */
  nowMs: number;
  /** Whether this cell's time window is expanded to 48h forward. */
  extended: boolean;
  pinned: boolean;
  gridPowerKw: number;
  zones?: ZoneDef[];
  onTogglePin: () => void;
}

export function GridAccumulatedCell({
  assetSummaries,
  allTimelines,
  nowMs,
  extended,
  pinned,
  gridPowerKw,
  zones,
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

  // Configured roster narrowed to what actually has plotted data in this window —
  // see assetIdsWithTimelineData's doc comment for why (legend must match the graph).
  const assetIds = assetIdsWithTimelineData(
    assetSummaries.map((s) => s.assetId),
    allTimelines
  );

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
            sx={{ color: ASSET_COLORS[s.assetId] ?? COLOR_ASSET_FALLBACK }}
          >
            {s.label}: {formatSignedPowerValue(s.powerKw)}
          </Typography>
        ))}
        <Divider sx={{ my: 0.5 }} />
        <Typography variant="caption" color="text.secondary" data-testid="accumulated-grid-power">
          Grid: {formatSignedPowerValue(gridPowerKw)}
        </Typography>
      </Box>

      {/* Right: stacked area chart */}
      <Box sx={{ flex: 1, minWidth: CELL_CHART_MIN_WIDTH }}>
        <StackedTimeSeriesChart
          data={stackedAreaPoints}
          assetIds={assetIds as AssetId[]}
          colorMap={ASSET_COLORS}
          nowMs={nowMs}
          hoursBack={window.hoursBack}
          hoursForward={window.hoursForward}
          height={tall ? CELL_CHART_HEIGHT_TALL : undefined}
          zones={zones}
          xAxisTickIntervalMinutes={extended ? EXTENDED_TICK_INTERVAL_MINUTES : DEFAULT_TICK_INTERVAL_MINUTES}
          interactiveLegend
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
