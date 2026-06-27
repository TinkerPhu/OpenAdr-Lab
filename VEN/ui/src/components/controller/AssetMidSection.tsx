import { Box } from "@mui/material";
import { CELL_CHART_HEIGHT, CELL_CHART_MIN_WIDTH } from "./chartLayout";
import type { AssetId, AssetTimelinePoint } from "./types";
import { AssetTimelineChart } from "./charts/AssetTimelineChart";
import type { ZoneDef } from "../../api/types";

interface AssetMidSectionProps {
  assetId: AssetId;
  timePoints: AssetTimelinePoint[];
  color: string;
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
  zones?: ZoneDef[];
}

export function AssetMidSection({
  assetId,
  timePoints,
  color,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  zones,
}: AssetMidSectionProps) {
  const stateKey =
    assetId === "ev" || assetId === "battery" ? "soc" :
    assetId === "heater" ? "temp_c" :
    undefined;

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
          stateKey={stateKey}
          zones={zones}
        />
      </div>
    </Box>
  );
}
