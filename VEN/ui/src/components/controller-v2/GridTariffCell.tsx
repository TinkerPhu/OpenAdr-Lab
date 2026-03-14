import { Box, IconButton, Paper, Tooltip, Typography } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import type { TariffSnapshot, TariffTimePoint } from "./types";
import { TariffChart } from "./charts/TariffChart";

interface GridTariffCellProps {
  snapshot: TariffSnapshot;
  timePoints: TariffTimePoint[];
  nowMs: number;
  pinned: boolean;
  onTogglePin: () => void;
}

export function GridTariffCell({
  snapshot,
  timePoints,
  nowMs,
  pinned,
  onTogglePin,
}: GridTariffCellProps) {
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
        <TariffChart data={timePoints} nowMs={nowMs} />
      </Box>

      {/* Pin button */}
      <Tooltip title={pinned ? "Unpin" : "Pin to top"}>
        <IconButton
          size="small"
          data-testid="grid-tariff-cell-pin-btn"
          onClick={onTogglePin}
          sx={{ alignSelf: "flex-start", m: 0.5 }}
        >
          {pinned ? <PushPinIcon fontSize="small" /> : <PushPinOutlinedIcon fontSize="small" />}
        </IconButton>
      </Tooltip>
    </Paper>
  );
}
