import { Box, Typography } from "@mui/material";
import { CELL_LEFT_SECTION_WIDTH } from "./chartLayout";
import type { AssetSummary } from "./types";

interface AssetLeftSectionProps {
  summary: AssetSummary;
}

export function AssetLeftSection({ summary }: AssetLeftSectionProps) {
  const { assetId, powerKw, costRateEurH, co2RateGH, socPct, tempC, forecastEnergyKwh, activeRequest } =
    summary;

  const sign = (n: number) => (n >= 0 ? "+" : "");

  return (
    <Box
      data-testid={`asset-cell-${assetId}-left`}
      sx={{ minWidth: CELL_LEFT_SECTION_WIDTH, px: 1.5, py: 1, display: "flex", flexDirection: "column", gap: 0.5 }}
    >
      <Typography variant="body2" fontWeight="bold" data-testid={`asset-name-${assetId}`}>
        {summary.label}
      </Typography>
      <Typography variant="caption" color="text.secondary" data-testid={`asset-power-${assetId}`}>
        {sign(powerKw)}
        {powerKw.toFixed(2)} kW
      </Typography>
      <Typography variant="caption" color="text.secondary" data-testid={`asset-cost-rate-${assetId}`}>
        {sign(costRateEurH)}
        {costRateEurH.toFixed(3)} €/h
      </Typography>
      <Typography variant="caption" color="text.secondary" data-testid={`asset-co2-rate-${assetId}`}>
        {sign(co2RateGH)}
        {co2RateGH.toFixed(0)} g CO₂eq/h
      </Typography>

      {socPct !== null && (
        <Typography variant="caption" color="text.secondary" data-testid={`asset-soc-${assetId}`}>
          SoC: {socPct.toFixed(1)}%
        </Typography>
      )}

      {tempC !== null && (
        <Typography variant="caption" color="text.secondary" data-testid={`asset-temp-${assetId}`}>
          T_tank: {tempC.toFixed(1)} °C
        </Typography>
      )}

      {forecastEnergyKwh !== null && (
        <Typography
          variant="caption"
          color="text.secondary"
          data-testid={`asset-forecast-energy-${assetId}`}
        >
          Forecast: {forecastEnergyKwh.toFixed(2)} kWh
        </Typography>
      )}

      {activeRequest && (
        <Typography variant="caption" color="primary" data-testid={`asset-request-${assetId}`}>
          Request: {activeRequest.requestedEnergyKwh.toFixed(1)} kWh
        </Typography>
      )}
    </Box>
  );
}
