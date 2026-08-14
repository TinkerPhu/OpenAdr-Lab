import {
  Chip, Paper, Table, TableBody, TableCell, TableContainer, TableHead, TableRow, Typography,
} from "@mui/material";
import { useAssetCapabilities, useAssetForecasts } from "../../api/hooks";
import { ASSET_LABELS } from "./types";
import type { AssetCapability } from "../../api/types";

// WP-T6 (docs/history/project_journal.md, search "WP-T"): wires GET /capability/:asset_id
// and GET /forecast, both previously unused. Shows "how much can this device
// flex right now" as a Max/Min band per direction (ceiling and the emergency-
// controllable floor — see AssetFlexibilityFloor), plus the forecast source
// chip for provenance. The forecast's power_kw number was dropped: for
// plan-controlled assets it duplicated the Controller V2 timeline charts
// (same Plan, different field), and for PV/base_load it was a second,
// independently-computed estimate the plan itself never used. Deliberately a
// standalone panel rather than folded into the already-complex AssetCell
// component.

function formatSource(source: string): string {
  return source
    .toLowerCase()
    .split("_")
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");
}

/** BL-27: "Stepped (0/1.5/3 kW)" for Stepped assets (the levels are the whole
 * point — a bare "Stepped" chip wouldn't tell the operator what the tiers
 * are); plain title-case label for every other classification. */
function formatAdjustability(cap: AssetCapability): string {
  const label = formatSource(cap.adjustability);
  if (cap.adjustability !== "STEPPED" || cap.power_steps_kw.length === 0) {
    return label;
  }
  const levels = cap.power_steps_kw.map((kw) => kw.toString()).join("/");
  return `${label} (${levels} kW)`;
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
            <TableCell colSpan={7}>
              <Typography variant="subtitle1">Flexibility &amp; Forecast</Typography>
            </TableCell>
          </TableRow>
          <TableRow>
            <TableCell>Asset</TableCell>
            <TableCell>Adjustability</TableCell>
            <TableCell align="right">Max import</TableCell>
            <TableCell align="right">Min import</TableCell>
            <TableCell align="right">Max export</TableCell>
            <TableCell align="right">Min export</TableCell>
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
                <TableCell data-testid={`adjustability-${assetId}`}>
                  {cap ? (
                    <Chip size="small" label={formatAdjustability(cap)} />
                  ) : (
                    "—"
                  )}
                </TableCell>
                <TableCell align="right">
                  {cap ? `${cap.max_import_kw.toFixed(2)} kW${cap.is_fixed ? " (fixed)" : ""}` : "—"}
                </TableCell>
                <TableCell align="right">{cap ? `${cap.min_import_kw.toFixed(2)} kW` : "—"}</TableCell>
                <TableCell align="right">{cap ? `${cap.max_export_kw.toFixed(2)} kW` : "—"}</TableCell>
                <TableCell align="right">{cap ? `${cap.min_export_kw.toFixed(2)} kW` : "—"}</TableCell>
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
