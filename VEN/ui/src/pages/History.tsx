import { useMemo, useState } from "react";
import {
  Box, Button, Paper, Table, TableBody, TableCell, TableContainer, TableHead, TableRow, TextField, Typography,
} from "@mui/material";
import { useHistoryTicks, useHistoryGrid, useHistoryEvents, useHistoryReports, useHistoryPlans } from "../api/hooks";
import { AssetTimelineChart } from "../components/controller/charts/AssetTimelineChart";
import { TariffChart } from "../components/controller/charts/TariffChart";
import { ASSET_COLORS, ASSET_LABELS } from "../components/controller/types";
import type { AssetTimelinePoint, TariffTimePoint } from "../components/controller/types";
import type { PlanSnapshot } from "../api/types";
import { JsonDialog } from "../components/JsonDialog";

/** Yesterday's date (UTC), the sensible default — "today" barely has any
 * downsampled history to show, especially early in the day. */
function defaultHistoryDate(): string {
  const d = new Date();
  d.setUTCDate(d.getUTCDate() - 1);
  return d.toISOString().slice(0, 10);
}

/** [from, to) ISO bounds for the UTC calendar day `dateStr` ("YYYY-MM-DD"). */
export function dayRangeIso(dateStr: string): { fromIso: string; toIso: string } {
  const from = new Date(`${dateStr}T00:00:00.000Z`);
  const to = new Date(from.getTime() + 24 * 3600 * 1000);
  return { fromIso: from.toISOString(), toIso: to.toISOString() };
}

export function HistoryPage() {
  const [date, setDate] = useState(defaultHistoryDate);
  const { fromIso, toIso } = useMemo(() => dayRangeIso(date), [date]);
  const toMs = useMemo(() => new Date(toIso).getTime(), [toIso]);

  const { data: ticks = [] } = useHistoryTicks(fromIso, toIso);
  const { data: grid = [] } = useHistoryGrid(fromIso, toIso);
  const { data: events = [] } = useHistoryEvents(fromIso, toIso);
  const { data: reports = [] } = useHistoryReports(fromIso, toIso);
  const { data: plans = [] } = useHistoryPlans(fromIso, toIso);
  const [selectedPlan, setSelectedPlan] = useState<PlanSnapshot | null>(null);

  const ticksByAsset = useMemo(() => {
    const map = new Map<string, AssetTimelinePoint[]>();
    for (const row of ticks) {
      const points = map.get(row.asset_id) ?? [];
      points.push({
        ts: row.ts,
        values: {
          power_kw: row.power_kw,
          ...(row.soc_pct !== null ? { soc: row.soc_pct / 100 } : {}),
          ...(row.temperature_c !== null ? { temp_c: row.temperature_c } : {}),
        },
      });
      map.set(row.asset_id, points);
    }
    return map;
  }, [ticks]);

  const tariffPoints: TariffTimePoint[] = useMemo(
    () =>
      grid.map((row) => ({
        ts: row.ts,
        importPriceEurKwh: row.import_tariff_eur_kwh,
        exportPriceEurKwh: row.export_tariff_eur_kwh,
        co2GKwh: row.co2_g_kwh,
        totalCostRateEurH:
          row.import_tariff_eur_kwh !== null
            ? row.import_kw * row.import_tariff_eur_kwh -
              row.export_kw * (row.export_tariff_eur_kwh ?? 0)
            : null,
        totalCo2RateGH: row.co2_g_kwh !== null ? row.import_kw * row.co2_g_kwh : null,
        gridPowerKw: row.import_kw - row.export_kw,
      })),
    [grid]
  );

  return (
    <Box sx={{ p: 2 }} data-testid="history-page">
      <Typography variant="h5" gutterBottom>History</Typography>
      <TextField
        label="Date (UTC)"
        type="date"
        size="small"
        value={date}
        onChange={(e) => setDate(e.target.value)}
        inputProps={{ "data-testid": "history-date-input" }}
        sx={{ mb: 2 }}
      />

      <Typography variant="h6">Grid</Typography>
      <TariffChart data={tariffPoints} nowMs={toMs} hoursBack={24} hoursForward={0} />

      {[...ticksByAsset.entries()].map(([assetId, points]) => {
        const hasSoc = points.some((p) => p.values?.soc !== undefined);
        const hasTemp = points.some((p) => p.values?.temp_c !== undefined);
        return (
          <Box key={assetId} sx={{ mt: 2 }} data-testid={`history-asset-chart-${assetId}`}>
            <Typography variant="subtitle1">{ASSET_LABELS[assetId] ?? assetId}</Typography>
            <AssetTimelineChart
              data={points}
              color={ASSET_COLORS[assetId] ?? "#888"}
              nowMs={toMs}
              hoursBack={24}
              hoursForward={0}
              stateKey={hasSoc ? "soc" : hasTemp ? "temp_c" : undefined}
            />
          </Box>
        );
      })}

      <Typography variant="h6" sx={{ mt: 3 }}>Events received</Typography>
      <TableContainer component={Paper}>
        <Table size="small" data-testid="history-events-table">
          <TableHead>
            <TableRow>
              <TableCell>Time</TableCell>
              <TableCell>Type</TableCell>
              <TableCell>Event ID</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {events.map((e) => (
              <TableRow key={`${e.event_id}-${e.received_at}`} data-testid={`history-event-row-${e.event_id}`}>
                <TableCell>{new Date(e.received_at).toLocaleString()}</TableCell>
                <TableCell>{e.event_type}</TableCell>
                <TableCell>{e.event_id}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableContainer>

      <Typography variant="h6" sx={{ mt: 3 }}>Reports sent</Typography>
      <TableContainer component={Paper}>
        <Table size="small" data-testid="history-reports-table">
          <TableHead>
            <TableRow>
              <TableCell>Time</TableCell>
              <TableCell>Type</TableCell>
              <TableCell>Event ID</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {reports.map((r) => (
              <TableRow key={`${r.event_id}-${r.sent_at}`} data-testid={`history-report-row-${r.event_id}`}>
                <TableCell>{new Date(r.sent_at).toLocaleString()}</TableCell>
                <TableCell>{r.report_type}</TableCell>
                <TableCell>{r.event_id}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableContainer>

      {/* WP-T6 (docs/history/project_journal.md, search "WP-T"): wires GET /history/plans,
          previously unused — plan snapshots archived for this day. */}
      <Typography variant="h6" sx={{ mt: 3 }}>Plans</Typography>
      <TableContainer component={Paper}>
        <Table size="small" data-testid="history-plans-table">
          <TableHead>
            <TableRow>
              <TableCell>Created</TableCell>
              <TableCell>Horizon start</TableCell>
              <TableCell>Horizon end</TableCell>
              <TableCell>Detail</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {plans.map((p) => (
              <TableRow key={p.created_at} data-testid={`history-plan-row-${p.created_at}`}>
                <TableCell>{new Date(p.created_at).toLocaleString()}</TableCell>
                <TableCell>{new Date(p.horizon_start).toLocaleString()}</TableCell>
                <TableCell>{new Date(p.horizon_end).toLocaleString()}</TableCell>
                <TableCell>
                  <Button
                    size="small"
                    data-testid={`history-plan-view-${p.created_at}`}
                    onClick={() => setSelectedPlan(p)}
                  >
                    View
                  </Button>
                </TableCell>
              </TableRow>
            ))}
            {plans.length === 0 && (
              <TableRow>
                <TableCell colSpan={4} align="center" data-testid="history-plans-empty">
                  No plan snapshots for this day
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>

      <JsonDialog
        open={selectedPlan !== null}
        title={selectedPlan ? `Plan snapshot: ${new Date(selectedPlan.created_at).toLocaleString()}` : "Plan"}
        json={selectedPlan ? JSON.parse(selectedPlan.plan_json) : {}}
        onClose={() => setSelectedPlan(null)}
      />
    </Box>
  );
}
