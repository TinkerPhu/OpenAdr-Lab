import { Box } from "@mui/material";
import { CELL_CHART_HEIGHT, CELL_CHART_MIN_WIDTH } from "@lab/charts/chartLayout";
import type { AssetId, AssetTimelinePoint } from "./types";
import { AssetTimelineChart } from "./charts/AssetTimelineChart";
import { assetChartSpec } from "./assetChartSpecs";
import type { ZoneDef } from "../../api/types";

interface AssetMidSectionProps {
  assetId: AssetId;
  timePoints: AssetTimelinePoint[];
  color: string;
  nowMs: number;
  hoursBack?: number;
  hoursForward?: number;
  zones?: ZoneDef[];
  xAxisTickIntervalMinutes?: number;
}

export function AssetMidSection({
  assetId,
  timePoints,
  color,
  nowMs,
  hoursBack = 1.0,
  hoursForward = 1.0,
  zones,
  xAxisTickIntervalMinutes,
}: AssetMidSectionProps) {
  const chartSpec = assetChartSpec(assetId);

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
          stateKey={chartSpec.stateKey}
          shadings={chartSpec.shadings}
          zones={zones}
          xAxisTickIntervalMinutes={xAxisTickIntervalMinutes}
        />
      </div>
    </Box>
  );
}
