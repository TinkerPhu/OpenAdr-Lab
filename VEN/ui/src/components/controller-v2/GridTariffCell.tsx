import { useMemo } from "react";
import { Box, IconButton, Paper, Tooltip, Typography } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import ZoomOutMapIcon from "@mui/icons-material/ZoomOutMap";
import ZoomInMapIcon from "@mui/icons-material/ZoomInMap";
import type { TariffSnapshot } from "./types";
import { TariffChart } from "./charts/TariffChart";
import { useTariffs } from "../../api/hooks";
import type { AssetTimelinePoint } from "./types";
import { buildTariffPricePoints, buildPowerPoints } from "./tariffBuilders";

const DEFAULT_WINDOW = { hoursBack: 1.0, hoursForward: 1.0 };
const EXTENDED_WINDOW = { hoursBack: 0.0, hoursForward: 24.0 };

interface GridTariffCellProps {
  snapshot: TariffSnapshot;
  gridTimeline: AssetTimelinePoint[];
  extended: boolean;
  pinned: boolean;
  onTogglePin: () => void;
  onToggleExpand: () => void;
}

export function GridTariffCell({
  snapshot,
  gridTimeline,
  extended,
  pinned,
  onTogglePin,
  onToggleExpand,
}: GridTariffCellProps) {
  const window = extended ? EXTENDED_WINDOW : DEFAULT_WINDOW;

  const { data: tariffsData = [] } = useTariffs();
  // nowMs updates each time fresh data arrives, matching AssetCell's pattern.
  const nowMs = useMemo(() => Date.now(), [gridTimeline, tariffsData]);

  const tariffTimePoints = useMemo(() => {
    const pricePoints = buildTariffPricePoints(tariffsData);
    const powerPoints = buildPowerPoints(gridTimeline);
    return [...pricePoints, ...powerPoints].sort((a, b) => a.ts - b.ts);
  }, [gridTimeline, tariffsData]);

  const fmt = (v: number | null, decimals = 4) =>
    v === null ? "—" : v.toFixed(decimals);

  return (
    <Paper
      variant="outlined"
      data-testid="grid-tariff-cell"
      sx={{ display: "flex", flexDirection: "row", mb: 1, borderLeft: "4px solid #37474f" }}
    >
      {/* Left: tariff values */}
      <Box sx={{ minWidth: 180, px: 1.5, py: 1, display: "flex", flexDirection: "column", gap: 0.5 }}>
        <Typography variant="body2" fontWeight="bold">
          Tariff
        </Typography>
        <Typography variant="caption" color="text.secondary" data-testid="tariff-import-price">
          Import: {fmt(snapshot.importPriceEurKwh)} €/kWh
        </Typography>
        <Typography variant="caption" color="text.secondary" data-testid="tariff-export-price">
          Export: {fmt(snapshot.exportPriceEurKwh)} €/kWh
        </Typography>
        <Typography variant="caption" color="text.secondary" data-testid="tariff-co2">
          CO₂eq: {fmt(snapshot.co2GKwh, 1)} g/kWh
        </Typography>
        <Typography variant="caption" color="text.secondary" data-testid="tariff-total-cost-rate">
          Cost rate: {fmt(snapshot.totalCostRateEurH, 3)} €/h
        </Typography>
        <Typography variant="caption" color="text.secondary" data-testid="tariff-grid-power">
          Grid: {snapshot.gridPowerKw.toFixed(2)} kW
        </Typography>
      </Box>

      {/* Right: tariff chart */}
      <Box sx={{ flex: 1, minWidth: 200 }}>
        <TariffChart
          data={tariffTimePoints}
          nowMs={nowMs}
          hoursBack={window.hoursBack}
          hoursForward={window.hoursForward}
        />
      </Box>

      {/* Right column: pin button on top, expand toggle below */}
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
        <Tooltip title={extended ? "Collapse to ±1h view" : "Expand to 24h tariff horizon (no past)"}>
          <IconButton
            size="small"
            data-testid="grid-tariff-cell-extend-btn"
            onClick={onToggleExpand}
            sx={{ m: 0.5 }}
          >
            {extended ? <ZoomInMapIcon fontSize="small" /> : <ZoomOutMapIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
      </Box>
    </Paper>
  );
}
