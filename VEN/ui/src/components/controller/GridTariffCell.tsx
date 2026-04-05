import { useMemo } from "react";
import { Box, IconButton, Paper, Tooltip } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import { CELL_CHART_MIN_WIDTH } from "./chartLayout";
import { TariffChart } from "./charts/TariffChart";
import { useTariffs } from "../../api/hooks";
import type { AssetTimelinePoint } from "./types";
import { buildTariffPricePoints, buildPowerPoints, fillCostRateFromTariffs } from "./tariffBuilders";

const DEFAULT_WINDOW = { hoursBack: 1.0, hoursForward: 1.0 };
const EXTENDED_WINDOW = { hoursBack: 1.0, hoursForward: 24.0 };

interface GridTariffCellProps {
  gridTimeline: AssetTimelinePoint[];
  nowMs: number;
  extended: boolean;
  pinned: boolean;
  onTogglePin: () => void;
}

export function GridTariffCell({
  gridTimeline,
  nowMs,
  extended,
  pinned,
  onTogglePin,
}: GridTariffCellProps) {
  const window = extended ? EXTENDED_WINDOW : DEFAULT_WINDOW;

  const { data: tariffsData = [] } = useTariffs();

  const tariffTimePoints = useMemo(() => {
    const pricePoints = buildTariffPricePoints(tariffsData);
    const powerPoints = buildPowerPoints(gridTimeline);
    const merged = [...pricePoints, ...powerPoints].sort((a, b) => a.ts - b.ts);
    return fillCostRateFromTariffs(merged, tariffsData);
  }, [gridTimeline, tariffsData]);

  return (
    <Paper
      variant="outlined"
      data-testid="grid-tariff-cell"
      sx={{ display: "flex", flexDirection: "row", mb: 1, borderLeft: "4px solid #37474f" }}
    >
      {/* Chart */}
      <Box sx={{ flex: 1, minWidth: CELL_CHART_MIN_WIDTH }}>
        <TariffChart
          data={tariffTimePoints}
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
            data-testid="grid-tariff-cell-pin-btn"
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
