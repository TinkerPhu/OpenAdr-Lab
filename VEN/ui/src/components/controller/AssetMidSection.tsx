import { Box } from "@mui/material";
import { CELL_CHART_HEIGHT, CELL_CHART_MIN_WIDTH } from "./chartLayout";
import type { AssetId, AssetTimelinePoint } from "./types";
import { AssetTimelineChart } from "./charts/AssetTimelineChart";

interface AssetMidSectionProps {
  assetId: AssetId;
  timePoints: AssetTimelinePoint[];
  color: string;
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
}

export function AssetMidSection({
  assetId,
  timePoints,
  color,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
}: AssetMidSectionProps) {
  return (
    <Box
      data-testid={`asset-cell-${assetId}-mid`}
      sx={{ flex: 1, minWidth: CELL_CHART_MIN_WIDTH, height: CELL_CHART_HEIGHT }}
    >
      <div data-testid={`asset-timeline-chart-${assetId}`} style={{ width: "100%", height: "100%" }}>
        <AssetTimelineChart
          data={timePoints}
          color={color}
          nowMs={nowMs}
          hoursBack={hoursBack}
          hoursForward={hoursForward}
        />
      </div>
    </Box>
  );
}
