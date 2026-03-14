import { Box, IconButton, Paper, Tooltip, Typography } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import type { AssetId, AssetSummary, StackedAreaPoint } from "./types";
import { ASSET_COLORS } from "./types";
import { StackedAreaChart } from "./charts/StackedAreaChart";

interface GridAccumulatedCellProps {
  assetSummaries: AssetSummary[];
  stackedAreaPoints: StackedAreaPoint[];
  nowMs: number;
  pinned: boolean;
  onTogglePin: () => void;
}

export function GridAccumulatedCell({
  assetSummaries,
  stackedAreaPoints,
  nowMs,
  pinned,
  onTogglePin,
}: GridAccumulatedCellProps) {
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
