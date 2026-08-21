import { useMemo, useRef, useState } from "react";
import {
  Box, Button, Paper, Stack, Table, TableBody, TableCell, TableContainer, TableHead, TableRow, TextField,
  Typography,
} from "@mui/material";
import {
  useHistoryTicks, useHistoryGrid, useHistoryEvents, useHistoryReports, useHistoryForecastAccuracy,
  useHealth,
} from "../api/hooks";
import { AssetTimelineChart } from "../components/controller/charts/AssetTimelineChart";
import { TariffEnvelopeChart } from "../components/controller/charts/TariffEnvelopeChart";
import { GridRatesChart } from "../components/controller/charts/GridRatesChart";
import { SiteHeadroomChart } from "../components/controller/charts/SiteHeadroomChart";
import { ASSET_COLORS, ASSET_LABELS } from "../components/controller/types";
import type { AssetTimelinePoint, TariffTimePoint } from "../components/controller/types";
import type { ForecastAccuracySample, SiteFlexibilitySample } from "../api/types";

/** forecast-accuracy-tracking: only these three assets get near/far forecast samples
 * recorded (see design.md Decision 5) — same set `record_forecast_accuracy_samples` writes. */
const FORECAST_TRACKED_ASSETS = ["pv", "base_load", "site-residual"] as const;

/** Rows per page for the "Events received"/"Reports sent" tables — both can hold far more
 * rows than a [from, to) window alone bounds (e.g. many active events on a short
 * `report_interval_s`), so both are paginated the same way. */
const HISTORY_TABLE_PAGE_SIZE = 50;

/** Shared "X-Y of total" + prev/next footer for a paginated history table — one definition
 * so "Events received" and "Reports sent" (the same row-shape-of-problem) can't drift. */
function HistoryTablePager({
  testIdPrefix, total, limit, offset, onOffsetChange,
}: {
  testIdPrefix: string; total: number; limit: number; offset: number;
  onOffsetChange: (next: number) => void;
}) {
  // A single page needs no controls or count — only show once there's a second page.
  if (total <= limit) return null;
  const from = offset + 1;
  const to = Math.min(offset + limit, total);
  return (
    <Stack direction="row" spacing={1} alignItems="center" sx={{ mt: 1, mb: 2 }}>
      <Button
        size="small" variant="outlined" disabled={offset === 0}
        onClick={() => onOffsetChange(Math.max(0, offset - limit))}
        data-testid={`${testIdPrefix}-prev`}
      >
        Prev
      </Button>
      <Button
        size="small" variant="outlined" disabled={to >= total}
        onClick={() => onOffsetChange(offset + limit)}
        data-testid={`${testIdPrefix}-next`}
      >
        Next
      </Button>
      <Typography variant="body2" color="text.secondary" data-testid={`${testIdPrefix}-summary`}>
        {from}-{to} of {total}
      </Typography>
    </Stack>
  );
}

/** [from, to) ISO bounds for the UTC calendar day `dateStr` ("YYYY-MM-DD"). */
export function dayRangeIso(dateStr: string): { fromIso: string; toIso: string } {
  const from = new Date(`${dateStr}T00:00:00.000Z`);
  const to = new Date(from.getTime() + 24 * 3600 * 1000);
  return { fromIso: from.toISOString(), toIso: to.toISOString() };
}

/** [from, to) ISO bounds for the rolling 24h window ending at `nowMs` — the default view,
 * so the tab is useful the moment it's opened instead of showing an empty/mostly-empty
 * calendar day. `nowMs` is caller-supplied (the server clock, not the browser's) so a
 * client with a skewed OS clock still queries a window that actually contains data. */
function last24hRangeIso(nowMs: number): { fromIso: string; toIso: string } {
  const to = new Date(nowMs);
  return { fromIso: new Date(to.getTime() - 24 * 3600 * 1000).toISOString(), toIso: to.toISOString() };
}

export function HistoryPage() {
  // null = default rolling last-24h window; a "YYYY-MM-DD" string once the user picks a
  // specific UTC calendar day to inspect instead.
  const [date, setDate] = useState<string | null>(null);
  // Server clock (see Controller.tsx's nowMs for the full rationale), captured once and
  // then frozen — like `date`, the rolling window is meant to be computed once (not
  // re-anchored on every /health poll), so it doesn't reset the Events/Reports table
  // pagination underneath a user mid-page. `serverNowMsRef` is set at most once, the
  // first time /health resolves; until then it falls back to a Date.now() snapshot
  // taken once at mount (not recomputed every render, which could otherwise change
  // fromIso/toIso every render and loop the state-reset block below).
  const health = useHealth();
  // eslint-disable-next-line react-hooks/purity -- intentional: one-time Date.now() snapshot at mount, not recomputed per render
  const [mountNowMs] = useState(() => Date.now());
  const serverNowMsRef = useRef<number | null>(null);
  if (serverNowMsRef.current === null && health.data) {
    serverNowMsRef.current = new Date(health.data.server_time).getTime();
  }
  const serverNowMs = serverNowMsRef.current ?? mountNowMs;
  const { fromIso, toIso } = useMemo(
    () => (date ? dayRangeIso(date) : last24hRangeIso(serverNowMs)),
    [date, serverNowMs]
  );
  const toMs = useMemo(() => new Date(toIso).getTime(), [toIso]);
  // Rolling 24h mode has no single "the" date (it spans two UTC calendar days) — show the
  // newest displayed day so the field is never blank, without treating that as a selection.
  const displayDate = date ?? toIso.slice(0, 10);

  // Paged independently of each other; reset to page 1 whenever the date/range changes,
  // since an offset from a previous window is meaningless against a new one. Adjusted during
  // render (React's documented pattern for resetting state on a prop change) rather than in a
  // useEffect, which would cause an extra commit/cascading render for the same outcome.
  const [eventsOffset, setEventsOffset] = useState(0);
  const [reportsOffset, setReportsOffset] = useState(0);
  const [pagedRangeIso, setPagedRangeIso] = useState({ fromIso, toIso });
  if (pagedRangeIso.fromIso !== fromIso || pagedRangeIso.toIso !== toIso) {
    setPagedRangeIso({ fromIso, toIso });
    setEventsOffset(0);
    setReportsOffset(0);
  }

  const ticksQuery = useHistoryTicks(fromIso, toIso);
  const gridQuery = useHistoryGrid(fromIso, toIso);
  const eventsQuery = useHistoryEvents(fromIso, toIso, HISTORY_TABLE_PAGE_SIZE, eventsOffset);
  const reportsQuery = useHistoryReports(fromIso, toIso, HISTORY_TABLE_PAGE_SIZE, reportsOffset);
  // Rules of hooks — fixed set, so called unconditionally rather than in the render loop below.
  const pvForecastQuery = useHistoryForecastAccuracy(fromIso, toIso, "pv");
  const baseLoadForecastQuery = useHistoryForecastAccuracy(fromIso, toIso, "base_load");
  const siteResidualForecastQuery = useHistoryForecastAccuracy(fromIso, toIso, "site-residual");

  const { data: ticks = [] } = ticksQuery;
  const { data: grid = [] } = gridQuery;
  const { data: eventsPage = { rows: [], total: 0 } } = eventsQuery;
  const { data: reportsPage = { rows: [], total: 0 } } = reportsQuery;
  const events = eventsPage.rows;
  const reports = reportsPage.rows;
  const { data: pvForecast = [] } = pvForecastQuery;
  const { data: baseLoadForecast = [] } = baseLoadForecastQuery;
  const { data: siteResidualForecast = [] } = siteResidualForecastQuery;
  const forecastByAsset: Record<string, ForecastAccuracySample[]> = {
    pv: pvForecast,
    base_load: baseLoadForecast,
    "site-residual": siteResidualForecast,
  };

  // The query keys are derived from `date`, so react-query already refetches whenever the
  // date actually changes. This covers the cases where the user wants a refresh without a
  // key change: reopening the date picker (stale last-24h view, same day re-confirmed) and
  // re-clicking "Last 24h" while already in that mode (the rolling window has drifted).
  const refetchAll = () => {
    // A stale page offset from the previous view could be past the end of a freshly
    // refetched window (fromIso/toIso didn't change identity here, so the effect above
    // won't fire) — reset both tables back to page 1 alongside the refetch.
    setEventsOffset(0);
    setReportsOffset(0);
    ticksQuery.refetch();
    gridQuery.refetch();
    eventsQuery.refetch();
    reportsQuery.refetch();
    pvForecastQuery.refetch();
    baseLoadForecastQuery.refetch();
    siteResidualForecastQuery.refetch();
  };

  // Clicking the date control means "show this day" even if the displayed value doesn't
  // change (native date inputs only fire onChange on a genuine value change, e.g. re-picking
  // today while the rolling last-24h view is showing the same date). Treat the click as if
  // `displayDate` had just been selected: switch out of rolling mode into the fixed calendar
  // day it's showing, and force a refetch only when that's a no-op (already on that exact day).
  const handleDateControlClick = () => {
    const alreadyOnThisDay = date === displayDate;
    setDate(displayDate);
    if (alreadyOnThisDay) refetchAll();
  };

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
        importLimitKw: row.import_limit_kw,
        exportLimitKw: row.export_limit_kw,
      })),
    [grid]
  );

  // Site-headroom-forecast Piece 3/4: persisted mean up_kw/down_kw, null before that
  // migration landed — filtered out rather than plotted as a fake zero band.
  const gridPowerTimeline: AssetTimelinePoint[] = useMemo(
    () => grid.map((row) => ({ ts: row.ts, values: { power_kw: row.import_kw - row.export_kw } })),
    [grid]
  );
  const headroomHistory: SiteFlexibilitySample[] = useMemo(
    () =>
      grid
        .filter((row) => row.up_kw !== null && row.down_kw !== null)
        .map((row) => ({
          ts: new Date(row.ts).toISOString(),
          up_kw: row.up_kw as number,
          down_kw: row.down_kw as number,
        })),
    [grid]
  );

  return (
    <Box sx={{ p: 2 }} data-testid="history-page">
      <Typography variant="h5" gutterBottom>History</Typography>
      <Stack direction="row" spacing={1} alignItems="center" sx={{ mb: 1 }}>
        <TextField
          label="Date (UTC)"
          type="date"
          size="small"
          value={displayDate}
          onChange={(e) => setDate(e.target.value || null)}
          inputProps={{ "data-testid": "history-date-input", onClick: handleDateControlClick }}
          InputLabelProps={{ shrink: true }}
        />
        <Button
          variant="outlined"
          onClick={() => {
            const alreadyOnLast24h = date === null;
            setDate(null);
            if (alreadyOnLast24h) refetchAll();
          }}
          data-testid="history-last-24h-btn"
          sx={{ height: 40 }}
        >
          Last 24h
        </Button>
      </Stack>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
        {date ? `Showing ${date} (UTC)` : "Showing the last 24 hours — pick a date above to view a specific UTC day"}
      </Typography>

      <Typography variant="h6">Grid</Typography>
      <TariffEnvelopeChart
        data={tariffPoints}
        nowMs={toMs}
        hoursBack={24}
        hoursForward={0}
        xAxisTickIntervalMinutes={30}
      />
      <GridRatesChart
        data={tariffPoints}
        nowMs={toMs}
        hoursBack={24}
        hoursForward={0}
        xAxisTickIntervalMinutes={30}
      />

      <Typography variant="h6" sx={{ mt: 3 }}>Site Headroom</Typography>
      <SiteHeadroomChart
        gridTimeline={gridPowerTimeline}
        history={headroomHistory}
        nowMs={toMs}
        hoursBack={24}
        hoursForward={0}
        xAxisTickIntervalMinutes={30}
      />

      {[...ticksByAsset.entries()].map(([assetId, points]) => {
        const hasSoc = points.some((p) => p.values?.soc !== undefined);
        const hasTemp = points.some((p) => p.values?.temp_c !== undefined);
        const isForecastTracked = (FORECAST_TRACKED_ASSETS as readonly string[]).includes(assetId);
        const forecast = isForecastTracked ? forecastByAsset[assetId] : undefined;
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
              nearForecast={forecast?.filter((s) => s.lead_kind === "near")}
              farForecast={forecast?.filter((s) => s.lead_kind === "far")}
              xAxisTickIntervalMinutes={30}
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
      <HistoryTablePager
        testIdPrefix="history-events-pager"
        total={eventsPage.total}
        limit={HISTORY_TABLE_PAGE_SIZE}
        offset={eventsOffset}
        onOffsetChange={setEventsOffset}
      />

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
      <HistoryTablePager
        testIdPrefix="history-reports-pager"
        total={reportsPage.total}
        limit={HISTORY_TABLE_PAGE_SIZE}
        offset={reportsOffset}
        onOffsetChange={setReportsOffset}
      />

    </Box>
  );
}
