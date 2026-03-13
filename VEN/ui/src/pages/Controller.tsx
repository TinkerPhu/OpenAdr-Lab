import { useMemo } from "react";
import {
  Box,
  Chip,
  Grid,
  Paper,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import {
  Area,
  CartesianGrid,
  ComposedChart,
  Legend,
  Line,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  useTrace,
  useCapacity,
  usePlan,
  useRates,
  usePackets,
  useLedger,
} from "../api/hooks";
import type {
  AssetLedger,
  EnergyPacket,
  OadrCapacityState,
  Plan,
  PlannedRates,
  TraceEntry,
} from "../api/types";

// ─── Chart data types ────────────────────────────────────────────────────────

type ControllerPowerPoint = {
  ts: number;
  // Past — solid
  trace_ev: number | null;
  trace_heater: number | null;
  trace_pv: number | null;
  trace_net: number | null;
  // Future — dashed
  plan_ev: number | null;
  plan_heater: number | null;
  plan_pv: number | null;
  plan_net: number | null;
  // Capacity step lines (future only)
  import_cap: number | null;
  export_cap: number | null;
};

type RateChartPoint = {
  ts: number;
  import_price: number | null;
  export_price: number | null;
  co2: number | null;
};

// ─── Pure data builders ──────────────────────────────────────────────────────

function buildPowerChartData(
  traceEntries: TraceEntry[],
  plan: Plan | null
): ControllerPowerPoint[] {
  const pastPoints: ControllerPowerPoint[] = traceEntries.map((e) => {
    const ev = e.setpoints.ev_charge_kw;
    const heater = e.setpoints.heater_kw;
    const pv = e.setpoints.pv_export_limit_kw ?? 0;
    return {
      ts: new Date(e.ts).getTime(),
      trace_ev: ev,
      trace_heater: heater,
      trace_pv: pv,
      trace_net: ev + heater - pv,
      plan_ev: null,
      plan_heater: null,
      plan_pv: null,
      plan_net: null,
      import_cap: null,
      export_cap: null,
    };
  });

  const futurePoints: ControllerPowerPoint[] = [];
  if (plan) {
    const allSlots = [...(plan.firm_slots ?? []), ...(plan.flexible_slots ?? [])];
    for (const slot of allSlots) {
      const ts = new Date(slot.start).getTime();
      const allocs = slot.allocations ?? [];
      const planEv = allocs
        .filter((a) => a.asset_id.toLowerCase().includes("ev"))
        .reduce((s, a) => s + a.power_kw, 0);
      const planHeater = allocs
        .filter((a) => a.asset_id.toLowerCase().includes("heater"))
        .reduce((s, a) => s + a.power_kw, 0);
      const planPv = allocs
        .filter((a) => a.asset_id.toLowerCase().includes("pv"))
        .reduce((s, a) => s + a.power_kw, 0);
      futurePoints.push({
        ts,
        trace_ev: null,
        trace_heater: null,
        trace_pv: null,
        trace_net: null,
        plan_ev: planEv,
        plan_heater: planHeater,
        plan_pv: planPv,
        plan_net: slot.net_import_kw,
        import_cap: slot.import_cap_kw,
        export_cap: slot.export_cap_kw,
      });
    }
  }

  return [...pastPoints, ...futurePoints].sort((a, b) => a.ts - b.ts);
}

function buildRateChartData(rates: PlannedRates | undefined): RateChartPoint[] {
  if (!rates || rates.length === 0) return [];
  return rates.map((s) => ({
    ts: new Date(s.interval_start).getTime(),
    import_price: s.import_price_eur_kwh,
    export_price: s.export_price_eur_kwh,
    co2: s.co2_g_kwh,
  }));
}

// ─── Status bar cards ────────────────────────────────────────────────────────

function CapacityCard({ capacity }: { capacity: OadrCapacityState | undefined }) {
  const fmt = (v: number | null | undefined) =>
    v != null ? `${v.toFixed(1)} kW` : "—";
  return (
    <Paper sx={{ p: 2, height: "100%" }}>
      <Typography variant="subtitle2" color="text.secondary" mb={1}>
        Capacity Limits
      </Typography>
      <Stack spacing={0.5}>
        <Typography variant="body2">Import limit: {fmt(capacity?.import_limit_kw)}</Typography>
        <Typography variant="body2">Export limit: {fmt(capacity?.export_limit_kw)}</Typography>
        <Typography variant="body2">Subscribed: {fmt(capacity?.import_subscription_kw)}</Typography>
      </Stack>
    </Paper>
  );
}

function PlanCard({ plan }: { plan: Plan | null | undefined }) {
  return (
    <Paper sx={{ p: 2, height: "100%" }}>
      <Typography variant="subtitle2" color="text.secondary" mb={1}>
        Active Plan
      </Typography>
      {!plan ? (
        <Typography variant="body2" color="text.secondary">No plan yet</Typography>
      ) : (
        <Stack spacing={0.5}>
          <Typography variant="body2">Trigger: {plan.trigger}</Typography>
          <Typography variant="body2">
            Firm cost: €{plan.firm_summary?.total_cost_eur?.toFixed(3) ?? "—"}
          </Typography>
          <Typography variant="body2">
            Import: {plan.firm_summary?.total_import_kwh?.toFixed(2) ?? "—"} kWh
          </Typography>
          <Stack direction="row" spacing={1} alignItems="center">
            <Typography variant="body2">Warnings:</Typography>
            {(plan.warnings ?? []).length > 0 ? (
              <Chip label={(plan.warnings ?? []).length} size="small" color="warning" />
            ) : (
              <Typography variant="body2">0</Typography>
            )}
          </Stack>
          <Typography variant="caption" color="text.secondary">
            {new Date(plan.created_at).toLocaleTimeString()}
          </Typography>
        </Stack>
      )}
    </Paper>
  );
}

const TERMINAL_STATUSES = new Set([
  "COMPLETED", "PARTIAL_COMPLETED", "ABANDONED", "FAILED",
]);

function PacketsSummaryCard({ packets }: { packets: EnergyPacket[] | undefined }) {
  const counts = useMemo(() => {
    const p = packets ?? [];
    return {
      active: p.filter((pk) => pk.status === "ACTIVE").length,
      pending: p.filter(
        (pk) => pk.status === "PENDING" || pk.status === "SCHEDULED"
      ).length,
      done: p.filter((pk) => TERMINAL_STATUSES.has(pk.status)).length,
    };
  }, [packets]);
  return (
    <Paper sx={{ p: 2, height: "100%" }}>
      <Typography variant="subtitle2" color="text.secondary" mb={1}>
        Packets
      </Typography>
      <Stack direction="row" spacing={1} flexWrap="wrap" mb={0.5}>
        <Chip label={`Active ${counts.active}`} size="small" color="success" />
        <Chip label={`Pending ${counts.pending}`} size="small" color="info" />
        <Chip label={`Done ${counts.done}`} size="small" />
      </Stack>
      <Typography variant="caption" color="text.secondary">
        Total: {(packets ?? []).length}
      </Typography>
    </Paper>
  );
}

function StatusBar({
  capacity,
  plan,
  packets,
}: {
  capacity: OadrCapacityState | undefined;
  plan: Plan | null | undefined;
  packets: EnergyPacket[] | undefined;
}) {
  return (
    <Grid container spacing={2}>
      <Grid item xs={12} sm={4}>
        <CapacityCard capacity={capacity} />
      </Grid>
      <Grid item xs={12} sm={4}>
        <PlanCard plan={plan} />
      </Grid>
      <Grid item xs={12} sm={4}>
        <PacketsSummaryCard packets={packets} />
      </Grid>
    </Grid>
  );
}

// ─── Power chart ─────────────────────────────────────────────────────────────

const timeFmt = (v: number) => new Date(v).toLocaleTimeString();

function PowerChart({ data, nowTs }: { data: ControllerPowerPoint[]; nowTs: number }) {
  if (data.length === 0) {
    return (
      <Paper sx={{ p: 2 }} data-testid="controller-power-chart-empty">
        <Typography color="text.secondary">No power data yet — waiting for trace…</Typography>
      </Paper>
    );
  }
  return (
    <Paper sx={{ p: 2 }} data-testid="controller-power-chart">
      <Typography variant="subtitle1" fontWeight="bold" mb={1}>
        Power — History (solid) + Plan (dashed) [kW]
      </Typography>
      <ResponsiveContainer width="100%" height={300}>
        <ComposedChart
          data={data}
          syncId="ctrl"
          margin={{ top: 4, right: 16, left: 0, bottom: 4 }}
        >
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis
            dataKey="ts"
            type="number"
            scale="time"
            domain={["dataMin", "dataMax"]}
            tickFormatter={timeFmt}
            tick={{ fontSize: 10 }}
          />
          <YAxis tick={{ fontSize: 10 }} unit=" kW" />
          <Tooltip
            labelFormatter={(v: number) => new Date(v).toLocaleTimeString()}
            formatter={(v: number) => [`${v.toFixed(2)} kW`]}
          />
          <Legend />
          {/* Past — solid */}
          <Line type="monotone" dataKey="trace_ev" name="EV (actual)" stroke="#1976d2" strokeWidth={1.5} dot={false} connectNulls={false} isAnimationActive={false} />
          <Line type="monotone" dataKey="trace_heater" name="Heater (actual)" stroke="#ed6c02" strokeWidth={1.5} dot={false} connectNulls={false} isAnimationActive={false} />
          <Line type="monotone" dataKey="trace_pv" name="PV (actual)" stroke="#f5c518" strokeWidth={1.5} dot={false} connectNulls={false} isAnimationActive={false} />
          <Line type="monotone" dataKey="trace_net" name="Net (actual)" stroke="#616161" strokeWidth={2.5} dot={false} connectNulls={false} isAnimationActive={false} />
          {/* Future — dashed */}
          <Line type="monotone" dataKey="plan_ev" name="EV (plan)" stroke="#1976d2" strokeDasharray="6 4" strokeWidth={1.5} dot={false} connectNulls={false} isAnimationActive={false} />
          <Line type="monotone" dataKey="plan_heater" name="Heater (plan)" stroke="#ed6c02" strokeDasharray="6 4" strokeWidth={1.5} dot={false} connectNulls={false} isAnimationActive={false} />
          <Line type="monotone" dataKey="plan_pv" name="PV (plan)" stroke="#f5c518" strokeDasharray="6 4" strokeWidth={1.5} dot={false} connectNulls={false} isAnimationActive={false} />
          <Line type="monotone" dataKey="plan_net" name="Net (plan)" stroke="#616161" strokeDasharray="6 4" strokeWidth={2} dot={false} connectNulls={false} isAnimationActive={false} />
          {/* Capacity limits — step */}
          <Line type="stepAfter" dataKey="import_cap" name="Import cap" stroke="#7b1fa2" strokeWidth={1} dot={false} connectNulls={false} isAnimationActive={false} />
          <Line type="stepAfter" dataKey="export_cap" name="Export cap" stroke="#388e3c" strokeWidth={1} dot={false} connectNulls={false} isAnimationActive={false} />
          <ReferenceLine
            x={nowTs}
            stroke="#f44336"
            strokeDasharray="5 5"
            label={{ value: "NOW", fill: "#f44336", fontSize: 10 }}
          />
        </ComposedChart>
      </ResponsiveContainer>
    </Paper>
  );
}

// ─── Rate chart ──────────────────────────────────────────────────────────────

function RateChart({ data, nowTs }: { data: RateChartPoint[]; nowTs: number }) {
  if (data.length === 0) {
    return (
      <Paper sx={{ p: 2 }} data-testid="controller-rate-chart-empty">
        <Typography color="text.secondary">
          No rate data — no active price events
        </Typography>
      </Paper>
    );
  }
  return (
    <Paper sx={{ p: 2 }} data-testid="controller-rate-chart">
      <Typography variant="subtitle1" fontWeight="bold" mb={1}>
        Rates — Prices + CO₂
      </Typography>
      <ResponsiveContainer width="100%" height={220}>
        <ComposedChart
          data={data}
          syncId="ctrl"
          margin={{ top: 4, right: 60, left: 0, bottom: 4 }}
        >
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis
            dataKey="ts"
            type="number"
            scale="time"
            domain={["dataMin", "dataMax"]}
            tickFormatter={timeFmt}
            tick={{ fontSize: 10 }}
          />
          <YAxis
            yAxisId="price"
            tick={{ fontSize: 10 }}
            unit=" €/kWh"
            width={72}
          />
          <YAxis
            yAxisId="co2"
            orientation="right"
            tick={{ fontSize: 10 }}
            unit=" g/kWh"
            width={64}
          />
          <Tooltip
            labelFormatter={(v: number) => new Date(v).toLocaleTimeString()}
          />
          <Legend />
          <Area
            yAxisId="price"
            type="stepAfter"
            dataKey="import_price"
            name="Import €/kWh"
            stroke="#0288d1"
            fill="#0288d1"
            fillOpacity={0.15}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
          />
          <Area
            yAxisId="price"
            type="stepAfter"
            dataKey="export_price"
            name="Export €/kWh"
            stroke="#00897b"
            fill="#00897b"
            fillOpacity={0.15}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
          />
          <Line
            yAxisId="co2"
            type="stepAfter"
            dataKey="co2"
            name="CO₂ g/kWh"
            stroke="#9e9e9e"
            strokeWidth={1.5}
            dot={false}
            connectNulls={false}
            isAnimationActive={false}
          />
          <ReferenceLine
            yAxisId="price"
            x={nowTs}
            stroke="#f44336"
            strokeDasharray="5 5"
          />
        </ComposedChart>
      </ResponsiveContainer>
    </Paper>
  );
}

// ─── Packets table ────────────────────────────────────────────────────────────

function fillBarColor(pct: number): string {
  if (pct >= 80) return "#2e7d32";
  if (pct >= 40) return "#e65100";
  return "#c62828";
}

function PacketsTable({ packets }: { packets: EnergyPacket[] | undefined }) {
  const rows = (packets ?? []).filter((p) => !TERMINAL_STATUSES.has(p.status));
  return (
    <Paper sx={{ p: 2 }} data-testid="controller-packets-table">
      <Typography variant="subtitle1" fontWeight="bold" mb={1}>
        Active Packets
      </Typography>
      {rows.length === 0 ? (
        <Typography color="text.secondary">No active or pending packets</Typography>
      ) : (
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>Asset</TableCell>
              <TableCell>Status</TableCell>
              <TableCell sx={{ minWidth: 140 }}>Fill %</TableCell>
              <TableCell>Deadline</TableCell>
              <TableCell align="right">Est. Cost €</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {rows.map((p) => {
              const fillPct = Math.round(p.estimated_completion * 100);
              const tier =
                p.value_curve.deadline_tiers[p.value_curve.active_tier_index];
              const deadline = tier?.deadline
                ? new Date(tier.deadline).toLocaleString()
                : "—";
              return (
                <TableRow key={p.id}>
                  <TableCell>{p.asset_id}</TableCell>
                  <TableCell>
                    <Chip label={p.status} size="small" />
                  </TableCell>
                  <TableCell>
                    <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                      <Box
                        sx={{
                          flex: 1,
                          height: 8,
                          borderRadius: 1,
                          bgcolor: "grey.200",
                          position: "relative",
                          overflow: "hidden",
                        }}
                      >
                        <Box
                          sx={{
                            position: "absolute",
                            left: 0,
                            top: 0,
                            bottom: 0,
                            width: `${fillPct}%`,
                            bgcolor: fillBarColor(fillPct),
                            transition: "width 0.3s",
                          }}
                        />
                      </Box>
                      <Typography variant="caption" sx={{ minWidth: 30 }}>
                        {fillPct}%
                      </Typography>
                    </Box>
                  </TableCell>
                  <TableCell>{deadline}</TableCell>
                  <TableCell align="right">
                    €{p.estimated_cost_eur.toFixed(3)}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
    </Paper>
  );
}

// ─── Ledger table ─────────────────────────────────────────────────────────────

function LedgerTable({ ledger }: { ledger: AssetLedger[] | undefined }) {
  return (
    <Paper sx={{ p: 2 }} data-testid="controller-ledger">
      <Typography variant="subtitle1" fontWeight="bold" mb={1}>
        Energy Ledger
      </Typography>
      {!ledger || ledger.length === 0 ? (
        <Typography color="text.secondary">No ledger data yet</Typography>
      ) : (
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>Asset</TableCell>
              <TableCell align="right">Import kWh</TableCell>
              <TableCell align="right">Export kWh</TableCell>
              <TableCell align="right">Cost €</TableCell>
              <TableCell align="right">CO₂ g</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {ledger.map((l) => (
              <TableRow key={l.asset_id}>
                <TableCell>{l.asset_id}</TableCell>
                <TableCell align="right">
                  {l.total_consumption_kwh.toFixed(3)}
                </TableCell>
                <TableCell align="right">
                  {l.total_production_kwh.toFixed(3)}
                </TableCell>
                <TableCell align="right">
                  €{l.total_import_cost_eur.toFixed(4)}
                </TableCell>
                <TableCell align="right">
                  {l.total_co2_g.toFixed(1)}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </Paper>
  );
}

// ─── Main page ────────────────────────────────────────────────────────────────

export function ControllerPage() {
  const traceQuery = useTrace(500);
  const packetsQuery = usePackets();
  const planQuery = usePlan();
  const ratesQuery = useRates();
  const capacityQuery = useCapacity();
  const ledgerQuery = useLedger();

  const nowTs = Date.now();

  const traceEntries = useMemo(
    () => [...(traceQuery.data ?? [])].reverse(),
    [traceQuery.data]
  );

  const powerData = useMemo(
    () => buildPowerChartData(traceEntries, planQuery.data ?? null),
    [traceEntries, planQuery.data]
  );

  const rateData = useMemo(
    () => buildRateChartData(ratesQuery.data),
    [ratesQuery.data]
  );

  return (
    <Stack spacing={3}>
      <Typography variant="h5">Controller</Typography>

      <StatusBar
        capacity={capacityQuery.data}
        plan={planQuery.data}
        packets={packetsQuery.data}
      />

      <PowerChart data={powerData} nowTs={nowTs} />

      <RateChart data={rateData} nowTs={nowTs} />

      <PacketsTable packets={packetsQuery.data} />

      <LedgerTable ledger={ledgerQuery.data} />
    </Stack>
  );
}
