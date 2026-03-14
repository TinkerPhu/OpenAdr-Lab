import { Box } from "@mui/material";
import type { AssetId, AssetTimePoint } from "./types";
import { AssetTimelineChart } from "./charts/AssetTimelineChart";

interface AssetMidSectionProps {
  assetId: AssetId;
  timePoints: AssetTimePoint[];
  color: string;
  nowMs: number;
}

export function AssetMidSection({ assetId, timePoints, color, nowMs }: AssetMidSectionProps) {
  return (
    <Box
      data-testid={`asset-cell-${assetId}-mid`}
      sx={{ flex: 1, minWidth: 200, height: 140 }}
    >
      <div data-testid={`asset-timeline-chart-${assetId}`} style={{ width: "100%", height: "100%" }}>
        <AssetTimelineChart data={timePoints} color={color} nowMs={nowMs} />
      </div>
    </Box>
  );
}
