import { useState } from "react";
import {
  Box, Chip, Collapse, IconButton, LinearProgress, Paper,
  Stack, Table, TableBody, TableCell, TableHead, TableRow, Typography,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import type { EnergyPacket, PacketStatus } from "../../api/types";

// ─── Status chip colors ───────────────────────────────────────────────────────

function statusColor(s: PacketStatus): "success" | "info" | "default" | "warning" | "error" {
  switch (s) {
    case "ACTIVE":           return "success";
    case "SCHEDULED":        return "info";
    case "PENDING":          return "default";
    case "PAUSED":           return "warning";
    case "COMPLETED":        return "success";
    case "PARTIAL_COMPLETED":return "warning";
    case "ABANDONED":        return "error";
    case "FAILED":           return "error";
  }
}

// ─── Fill gauge color ─────────────────────────────────────────────────────────

function fillColor(pct: number): "success" | "warning" | "error" {
  if (pct > 0.8) return "success";
  if (pct >= 0.4) return "warning";
  return "error";
}

// ─── Deadline countdown ───────────────────────────────────────────────────────

function DeadlineDisplay({ packet }: { packet: EnergyPacket }) {
  const tiers = packet.value_curve.deadline_tiers;
  const idx = packet.value_curve.active_tier_index;
  if (!tiers.length || idx >= tiers.length) return null;

  const deadline = new Date(tiers[idx].deadline);
  const diffMs = deadline.getTime() - Date.now();

  if (diffMs <= 0) {
    return (
      <Chip
        data-testid={`packet-deadline-${packet.id}`}
        label="OVERDUE"
        color="error"
        size="small"
        sx={{ fontWeight: "bold" }}
      />
    );
  }

  const totalMin = Math.floor(diffMs / 60000);
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  const label = h > 0 ? `T−${h}h ${m}m` : `T−${m}m`;

  return (
    <Typography
      data-testid={`packet-deadline-${packet.id}`}
      variant="caption"
      color="text.secondary"
    >
      {label}
    </Typography>
  );
}

// ─── Budget bar ───────────────────────────────────────────────────────────────

function BudgetBar({ packet }: { packet: EnergyPacket }) {
  const tiers = packet.value_curve.deadline_tiers;
  const idx = packet.value_curve.active_tier_index;
  if (!tiers.length || idx >= tiers.length) return null;
  const maxCost = tiers[idx].max_total_cost_eur;
  if (maxCost === null) return null;

  const pct = maxCost > 0 ? (packet.accumulated_cost_eur / maxCost) * 100 : 0;
  const clamped = Math.min(pct, 100);
  const color = pct > 90 ? "error" : "primary";

  return (
    <Box data-testid={`packet-budget-${packet.id}`}>
      <Stack direction="row" justifyContent="space-between">
        <Typography variant="caption" color="text.secondary">Budget</Typography>
        <Typography variant="caption" color="text.secondary">
          €{packet.accumulated_cost_eur.toFixed(2)} / €{maxCost.toFixed(2)}
        </Typography>
      </Stack>
      <LinearProgress variant="determinate" value={clamped} color={color} sx={{ height: 4, borderRadius: 2 }} />
    </Box>
  );
}

// ─── Packet card ──────────────────────────────────────────────────────────────

function PacketCard({ packet }: { packet: EnergyPacket }) {
  const [expanded, setExpanded] = useState(false);
  const fillPct = packet.estimated_completion;
  const color = fillColor(fillPct);

  return (
    <Paper
      data-testid={`packet-card-${packet.id}`}
      variant="outlined"
      sx={{ p: 1.5, mb: 1 }}
    >
      <Stack spacing={0.75}>
        {/* Header row */}
        <Stack direction="row" alignItems="center" justifyContent="space-between">
          <Typography variant="body2" fontWeight="medium">
            {packet.asset_id} <Typography component="span" variant="caption" color="text.secondary">…{packet.id.slice(-6)}</Typography>
          </Typography>
          <Stack direction="row" alignItems="center" spacing={0.5}>
            <Chip label={packet.status} color={statusColor(packet.status)} size="small" />
            <IconButton
              size="small"
              data-testid={`packet-expand-${packet.id}`}
              onClick={() => setExpanded((e) => !e)}
              aria-label={expanded ? "Collapse packet details" : "Expand packet details"}
            >
              {expanded ? <ExpandLessIcon fontSize="small" /> : <ExpandMoreIcon fontSize="small" />}
            </IconButton>
          </Stack>
        </Stack>

        {/* Fill gauge */}
        <Box>
          <Stack direction="row" justifyContent="space-between">
            <Typography variant="caption" color="text.secondary">Fill</Typography>
            <Typography variant="caption" color="text.secondary">{(fillPct * 100).toFixed(0)}%</Typography>
          </Stack>
          <LinearProgress
            data-testid={`packet-fill-${packet.id}`}
            data-color={color}
            variant="determinate"
            value={fillPct * 100}
            color={color}
            sx={{ height: 6, borderRadius: 3 }}
          />
        </Box>

        {/* Deadline + target row */}
        <Stack direction="row" alignItems="center" justifyContent="space-between">
          <DeadlineDisplay packet={packet} />
          <Typography variant="caption" color="text.secondary">
            {packet.target_energy_kwh} kWh @ {packet.desired_power_kw} kW
          </Typography>
        </Stack>

        {/* Budget bar */}
        <BudgetBar packet={packet} />

        {/* Expanded: deadline tiers */}
        <Collapse in={expanded} unmountOnExit>
          <Box data-testid={`packet-tiers-${packet.id}`} sx={{ mt: 1 }}>
            <Typography variant="caption" fontWeight="medium">Deadline Tiers</Typography>
            <Table size="small">
              <TableHead>
                <TableRow>
                  <TableCell sx={{ fontSize: "0.7rem" }}>#</TableCell>
                  <TableCell sx={{ fontSize: "0.7rem" }}>Deadline</TableCell>
                  <TableCell sx={{ fontSize: "0.7rem" }}>Min %</TableCell>
                  <TableCell sx={{ fontSize: "0.7rem" }}>Max €</TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {packet.value_curve.deadline_tiers.map((tier, i) => (
                  <TableRow
                    key={i}
                    sx={{ bgcolor: i === packet.value_curve.active_tier_index ? "action.selected" : undefined }}
                  >
                    <TableCell sx={{ fontSize: "0.7rem" }}>{i}</TableCell>
                    <TableCell sx={{ fontSize: "0.7rem" }}>{new Date(tier.deadline).toLocaleString()}</TableCell>
                    <TableCell sx={{ fontSize: "0.7rem" }}>{(tier.min_completion * 100).toFixed(0)}%</TableCell>
                    <TableCell sx={{ fontSize: "0.7rem" }}>{tier.max_total_cost_eur != null ? `€${tier.max_total_cost_eur.toFixed(2)}` : "—"}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            {/* For ABANDONED/FAILED: show termination details */}
            {(packet.status === "ABANDONED" || packet.status === "FAILED") && (
              <Typography variant="caption" color="error" sx={{ mt: 0.5, display: "block" }}>
                Terminated at tier {packet.value_curve.active_tier_index},
                final fill {(packet.estimated_completion * 100).toFixed(0)}%
              </Typography>
            )}
          </Box>
        </Collapse>
      </Stack>
    </Paper>
  );
}

// ─── Group section ────────────────────────────────────────────────────────────

function PacketGroup({
  testId, label, packets, defaultOpen,
}: { testId: string; label: string; packets: EnergyPacket[]; defaultOpen: boolean }) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <Box data-testid={testId}>
      <Stack
        direction="row"
        alignItems="center"
        spacing={1}
        sx={{ cursor: "pointer", mb: 0.5 }}
        onClick={() => setOpen((o) => !o)}
      >
        <Typography variant="subtitle2">{label}</Typography>
        <Chip label={packets.length} size="small" />
        {open ? <ExpandLessIcon fontSize="small" /> : <ExpandMoreIcon fontSize="small" />}
      </Stack>
      <Collapse in={open}>
        {packets.map((p) => <PacketCard key={p.id} packet={p} />)}
      </Collapse>
    </Box>
  );
}

// ─── Main component ───────────────────────────────────────────────────────────

type Props = { packets: EnergyPacket[] };

export function PacketProgressBoard({ packets }: Props) {
  if (!packets.length) {
    return (
      <Typography data-testid="packet-board-empty" color="text.secondary">
        No energy packets.
      </Typography>
    );
  }

  const active   = packets.filter((p) => p.status === "ACTIVE");
  const queued   = packets.filter((p) => ["PENDING", "SCHEDULED", "PAUSED"].includes(p.status));
  const done     = packets.filter((p) => ["COMPLETED", "PARTIAL_COMPLETED", "ABANDONED", "FAILED"].includes(p.status));

  return (
    <Box data-testid="packet-board">
      <Stack spacing={2}>
        <PacketGroup testId="packet-group-active" label="Active" packets={active} defaultOpen={true} />
        <PacketGroup testId="packet-group-queued" label="Queued" packets={queued} defaultOpen={true} />
        <PacketGroup testId="packet-group-done"   label="Done"   packets={done}   defaultOpen={false} />
      </Stack>
    </Box>
  );
}
