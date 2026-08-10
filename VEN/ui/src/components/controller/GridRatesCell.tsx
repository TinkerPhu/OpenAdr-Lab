import { useMemo, useState } from "react";
import { Box, IconButton, Paper, Tooltip, Typography } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import UnfoldMoreIcon from "@mui/icons-material/UnfoldMore";
import UnfoldLessIcon from "@mui/icons-material/UnfoldLess";
import {
  CELL_CHART_MIN_WIDTH, CELL_LEFT_SECTION_WIDTH, DEFAULT_WINDOW, EXTENDED_WINDOW, CELL_CHART_HEIGHT_TALL,
  DEFAULT_TICK_INTERVAL_MINUTES, EXTENDED_TICK_INTERVAL_MINUTES,
} from "../charts/chartLayout";
import type { TariffSnapshot, AssetTimelinePoint } from "./types";
import { GridRatesChart } from "./charts/GridRatesChart";
import type { ZoneDef } from "../../api/types";
import { useTariffs } from "../../api/hooks";
import { buildTariffPricePoints, buildPowerPoints, fillCostRateFromTariffs } from "./tariffBuilders";

interface GridRatesCellProps {
  snapshot: TariffSnapshot;
  gridTimeline: AssetTimelinePoint[];
  nowMs: number;
  extended: boolean;
  pinned: boolean;
  zones?: ZoneDef[];
  onTogglePin: () => void;
}

/**
 * Derived signals — cost rate and CO₂ rate, both computed by the VEN as
 * (tariff × grid power). Split out from the combined tariff/rates cell so direct
 * VTN signals (GridTariffCell: tariff + capacity-limit envelope) and VEN-derived
 * ones don't share a diagram — see GridRatesChart's doc comment.
 */
export function GridRatesCell({
  snapshot,
  gridTimeline,
  nowMs,
  extended,
  pinned,
  zones,
  onTogglePin,
}: GridRatesCellProps) {
  const [tall, setTall] = useState(false);
  const window = extended ? EXTENDED_WINDOW : DEFAULT_WINDOW;

  const { data: tariffsData = [] } = useTariffs();

  const tariffTimePoints = useMemo(() => {
    const pricePoints = buildTariffPricePoints(tariffsData);
    const powerPoints = buildPowerPoints(gridTimeline);
    const merged = [...pricePoints, ...powerPoints].sort((a, b) => a.ts - b.ts);
    return fillCostRateFromTariffs(merged, tariffsData);
  }, [gridTimeline, tariffsData]);

  const fmt = (v: number | null, decimals = 4) =>
    v === null ? "—" : v.toFixed(decimals);

  return (
    <Paper
      variant="outlined"
      data-testid="grid-rates-cell"
      sx={{ display: "flex", flexDirection: "row", mb: 1, borderLeft: "4px solid #4e342e" }}
    >
      {/* Left: rate values */}
      <Box sx={{ minWidth: CELL_LEFT_SECTION_WIDTH, px: 1.5, py: 1, display: "flex", flexDirection: "column", gap: 0.5 }}>
        <Typography variant="body2" fontWeight="bold">
          Grid Rates
        </Typography>
        <Typography variant="caption" color="text.secondary" data-testid="rates-total-cost-rate">
          Cost rate: {fmt(snapshot.totalCostRateEurH, 3)} €/h
        </Typography>
        <Typography variant="caption" color="text.secondary" data-testid="rates-total-co2-rate">
          CO₂ rate: {fmt(snapshot.totalCo2RateGH, 1)} g/h
        </Typography>
      </Box>

      {/* Right: derived signals — cost rate + CO2 rate */}
      <Box sx={{ flex: 1, minWidth: CELL_CHART_MIN_WIDTH }}>
        <GridRatesChart
          data={tariffTimePoints}
          nowMs={nowMs}
          hoursBack={window.hoursBack}
          hoursForward={window.hoursForward}
          height={tall ? CELL_CHART_HEIGHT_TALL : undefined}
          zones={zones}
          xAxisTickIntervalMinutes={extended ? EXTENDED_TICK_INTERVAL_MINUTES : DEFAULT_TICK_INTERVAL_MINUTES}
        />
      </Box>

      {/* Right column: pin button + vertical expand button */}
      <Box sx={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
        <Tooltip title={pinned ? "Unpin" : "Pin to top"}>
          <IconButton
            size="small"
            data-testid="grid-rates-cell-pin-btn"
            onClick={onTogglePin}
            sx={{ m: 0.5 }}
          >
            {pinned ? <PushPinIcon fontSize="small" /> : <PushPinOutlinedIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
        <Tooltip title={tall ? "Collapse chart" : "Expand chart"}>
          <IconButton
            size="small"
            data-testid="grid-rates-cell-tall-expand-btn"
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
