import {
  Chip, Paper, Table, TableBody, TableCell, TableContainer, TableHead, TableRow, Typography,
} from "@mui/material";
import { useAssetCapabilities, useAssetForecasts } from "../../api/hooks";
import { ASSET_LABELS } from "./types";

// WP-T6 (docs/plans/ven-ui-transparency.md): wires GET /capability/:asset_id
// and GET /forecast, both previously unused — "how much can this device flex
// right now" and "what does the planner expect it to do next", side by side
// per asset. Deliberately a standalone panel rather than folded into the
// already-complex AssetCell component (lower integration risk for a WP whose
// only goal is surfacing existing data, not redesigning the cell).

function formatSource(source: string): string {
  return source
    .toLowerCase()
    .split("_")
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");
}

export function FlexibilityForecastPanel({ assetIds }: { assetIds: string[] }) {
  const capabilityResults = useAssetCapabilities(assetIds);
  const { data: forecasts = [] } = useAssetForecasts();
  const forecastByAsset = new Map(forecasts.map((f) => [f.asset_id, f]));

  if (assetIds.length === 0) return null;

  return (
    <TableContainer component={Paper} data-testid="flexibility-forecast-panel" sx={{ mb: 2 }}>
      <Table size="small">
        <TableHead>
          <TableRow>
            <TableCell colSpan={5}>
              <Typography variant="subtitle1">Flexibility &amp; Forecast</Typography>
            </TableCell>
          </TableRow>
          <TableRow>
            <TableCell>Asset</TableCell>
            <TableCell align="right">Max import</TableCell>
            <TableCell align="right">Max export</TableCell>
            <TableCell>Next predicted power</TableCell>
            <TableCell>Forecast source</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {assetIds.map((assetId, i) => {
            const cap = capabilityResults[i]?.data;
            const forecast = forecastByAsset.get(assetId);
            return (
              <TableRow key={assetId} data-testid={`flexibility-row-${assetId}`}>
                <TableCell>{ASSET_LABELS[assetId] ?? assetId}</TableCell>
                <TableCell align="right">
                  {cap ? `${cap.max_import_kw.toFixed(2)} kW${cap.is_fixed ? " (fixed)" : ""}` : "—"}
                </TableCell>
                <TableCell align="right">{cap ? `${cap.max_export_kw.toFixed(2)} kW` : "—"}</TableCell>
                <TableCell>
                  {forecast && forecast.power_kw.length > 0
                    ? `${forecast.power_kw[0].toFixed(2)} kW (${Math.round(forecast.confidence * 100)}% confidence)`
                    : "—"}
                </TableCell>
                <TableCell>
                  {forecast ? (
                    <Chip
                      size="small"
                      label={formatSource(forecast.source)}
                      data-testid={`forecast-source-${assetId}`}
                    />
                  ) : (
                    "—"
                  )}
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
