import { useMemo, useState } from "react";
import {
  Box, Button, Chip, Paper, Stack, Table, TableBody, TableCell, TableContainer, TableHead, TableRow,
  TextField, Typography,
} from "@mui/material";
import {
  CartesianGrid, Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis,
} from "recharts";
import { useHistoryPlans } from "../api/hooks";
import { niceAxis, tickFormatterForStep } from "../components/charts/axisDomain";
import type { PlanHistorySample, WarningKind } from "../api/types";

/** [from, to) ISO bounds for the UTC calendar day `dateStr` ("YYYY-MM-DD"). */
export function dayRangeIso(dateStr: string): { fromIso: string; toIso: string } {
  const from = new Date(`${dateStr}T00:00:00.000Z`);
  const to = new Date(from.getTime() + 24 * 3600 * 1000);
  return { fromIso: from.toISOString(), toIso: to.toISOString() };
}

/** [from, to) ISO bounds for the rolling 24h window ending now — the default view. */
function last24hRangeIso(): { fromIso: string; toIso: string } {
  const to = new Date();
  return { fromIso: new Date(to.getTime() - 24 * 3600 * 1000).toISOString(), toIso: to.toISOString() };
}

const WARNING_KIND_LABELS: Record<WarningKind, string> = {
  SOLVER_INFEASIBLE: "Solver infeasible",
  STALE_RATE_ESTIMATE: "Stale rate estimate",
  BUDGET_SHORTFALL: "Budget shortfall",
  CAPACITY_VIOLATION: "Capacity violation",
  PEAK_PENALTY_EXCEEDED: "Peak penalty exceeded",
  EV_CORE_ENERGY_UNMET: "EV core energy unmet",
  OTHER: "Other",
};

const WARNING_KIND_COLOR: Record<WarningKind, "error" | "warning" | "default"> = {
  SOLVER_INFEASIBLE: "error",
  STALE_RATE_ESTIMATE: "warning",
  BUDGET_SHORTFALL: "warning",
  CAPACITY_VIOLATION: "warning",
  PEAK_PENALTY_EXCEEDED: "warning",
  EV_CORE_ENERGY_UNMET: "warning",
  OTHER: "default",
};

function formatTs(ts: number): string {
  return new Date(ts).toLocaleString();
}

export function PlanHistoryPage() {
  const [date, setDate] = useState<string | null>(null);
  const { fromIso, toIso } = useMemo(() => (date ? dayRangeIso(date) : last24hRangeIso()), [date]);
  const displayDate = date ?? toIso.slice(0, 10);

  const plansQuery = useHistoryPlans(fromIso, toIso);
  const { data: plans = [] } = plansQuery;

  const solverMsSeries = useMemo(
    () =>
      plans
        .filter((p): p is PlanHistorySample & { solver_ms: number } => p.solver_ms !== null)
        .map((p) => ({ ts: p.created_at, solver_ms: p.solver_ms })),
    [plans]
  );

  // Solve time is a strictly-positive millisecond count; anchor the axis at 0 (a solve taking
  // "no time" is the meaningful baseline) and let niceAxis pick round ticks.
  const solverMsAxis = useMemo(
    () => niceAxis([0, Math.max(...solverMsSeries.map((p) => p.solver_ms), 0)]),
    [solverMsSeries]
  );

  const handleDateControlClick = () => {
    const alreadyOnThisDay = date === displayDate;
    setDate(displayDate);
    if (alreadyOnThisDay) plansQuery.refetch();
  };

  return (
    <Box sx={{ p: 2 }} data-testid="plan-history-page">
      <Typography variant="h5" gutterBottom>Plan History</Typography>
      <Typography variant="body2" color="text.secondary" sx={{ mb: 2 }}>
        Solve time, solver outcome, and warning trend across past plan cycles (GB-25). The MIP-gap
        column is the solver's configured tolerance for that cycle, not the achieved gap.
      </Typography>
      <Stack direction="row" spacing={1} alignItems="center" sx={{ mb: 1 }}>
        <TextField
          label="Date (UTC)"
          type="date"
          size="small"
          value={displayDate}
          onChange={(e) => setDate(e.target.value || null)}
          inputProps={{ "data-testid": "plan-history-date-input", onClick: handleDateControlClick }}
          InputLabelProps={{ shrink: true }}
        />
        <Button
          variant="outlined"
          onClick={() => {
            const alreadyOnLast24h = date === null;
            setDate(null);
            if (alreadyOnLast24h) plansQuery.refetch();
          }}
          data-testid="plan-history-last-24h-btn"
          sx={{ height: 40 }}
        >
          Last 24h
        </Button>
      </Stack>

      <Typography variant="h6" sx={{ mt: 2 }}>Solve time trend</Typography>
      {solverMsSeries.length === 0 ? (
        <Typography variant="body2" color="text.secondary" data-testid="plan-history-no-solver-ms">
          No plan cycles with a recorded solve time in this window.
        </Typography>
      ) : (
        <Box data-testid="plan-history-solver-ms-chart" sx={{ height: 220 }}>
          <ResponsiveContainer width="100%" height="100%">
            <LineChart data={solverMsSeries}>
              <CartesianGrid strokeDasharray="3 3" />
              <XAxis dataKey="ts" tickFormatter={formatTs} minTickGap={40} />
              {/* Round ticks via the shared niceAxis rule (axisDomain.ts) — this chart owns
                  its own <YAxis> rather than going through TimeSeriesChart. */}
              <YAxis
                unit=" ms"
                width={70}
                domain={solverMsAxis.domain}
                ticks={solverMsAxis.ticks}
                tickFormatter={tickFormatterForStep(solverMsAxis.step)}
              />
              <Tooltip labelFormatter={formatTs} formatter={(v: number) => [`${v} ms`, "Solve time"]} />
              <Line type="monotone" dataKey="solver_ms" stroke="#1976d2" dot={false} isAnimationActive={false} />
            </LineChart>
          </ResponsiveContainer>
        </Box>
      )}

      <Typography variant="h6" sx={{ mt: 3 }}>Plan cycles</Typography>
      <TableContainer component={Paper}>
        <Table size="small" data-testid="plan-history-table">
          <TableHead>
            <TableRow>
              <TableCell>Time</TableCell>
              <TableCell>Trigger</TableCell>
              <TableCell>Solve status</TableCell>
              <TableCell>Solver ms</TableCell>
              <TableCell>MIP gap target</TableCell>
              <TableCell>Warnings</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {plans.map((p) => (
              <TableRow key={p.plan_id} data-testid={`plan-history-row-${p.plan_id}`}>
                <TableCell>{formatTs(p.created_at)}</TableCell>
                <TableCell>{p.trigger}</TableCell>
                <TableCell>{p.solve_status}</TableCell>
                <TableCell>{p.solver_ms ?? "—"}</TableCell>
                <TableCell>{p.mip_gap_target ?? "—"}</TableCell>
                <TableCell>
                  {p.warning_kinds.length === 0 ? (
                    "—"
                  ) : (
                    <Stack direction="row" spacing={0.5} flexWrap="wrap" useFlexGap>
                      {p.warning_kinds.map((k, i) => (
                        <Chip
                          key={`${k}-${i}`}
                          size="small"
                          label={WARNING_KIND_LABELS[k] ?? k}
                          color={WARNING_KIND_COLOR[k] ?? "default"}
                          data-testid={`plan-history-warning-kind-${k}`}
                        />
                      ))}
                    </Stack>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </TableContainer>
    </Box>
  );
}
