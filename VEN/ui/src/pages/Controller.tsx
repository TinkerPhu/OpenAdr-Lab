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
  useCapacity,
  usePlan,
  usePackets,
  useLedger,
} from "../api/hooks";
import type {
  AssetLedger,
  EnergyPacket,
  OadrCapacityState,
  Plan,
} from "../api/types";

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
              <TableCell align="right">Energy kWh</TableCell>
              <TableCell align="right">Cost €</TableCell>
              <TableCell align="right">CO₂ g</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {ledger.map((l) => (
              <TableRow key={l.asset_id}>
                <TableCell>{l.asset_id}</TableCell>
                <TableCell align="right">
                  {l.energy_kwh.toFixed(3)}
                </TableCell>
                <TableCell align="right">
                  €{l.cost_eur.toFixed(4)}
                </TableCell>
                <TableCell align="right">
                  {l.co2_g.toFixed(1)}
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
  const packetsQuery = usePackets();
  const planQuery = usePlan();
  const capacityQuery = useCapacity();
  const ledgerQuery = useLedger();

  return (
    <Stack spacing={3}>
      <Typography variant="h5">Controller</Typography>

      <StatusBar
        capacity={capacityQuery.data}
        plan={planQuery.data}
        packets={packetsQuery.data}
      />

      <PacketsTable packets={packetsQuery.data} />

      <LedgerTable ledger={ledgerQuery.data} />
    </Stack>
  );
}
