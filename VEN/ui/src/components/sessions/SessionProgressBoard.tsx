import { useState } from "react";
import {
  Box, Chip, Collapse, IconButton, LinearProgress, Paper,
  Stack, Table, TableBody, TableCell, TableHead, TableRow, Typography,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import type {
  Plan, SimSnapshot, UserRequestStatus, UserRequestWithSession,
} from "../../api/types";
import { deviceIcon, sessionDeadline, sessionSummary } from "./sessionSummary";

// ─── Derivation helpers ───────────────────────────────────────────────────────

function statusColor(s: UserRequestStatus): "success" | "default" | "error" {
  switch (s) {
    case "ACTIVE":    return "success";
    case "COMPLETED": return "success";
    case "CANCELLED": return "default";
    case "FAILED":    return "error";
  }
}

function fillColor(pct: number): "success" | "warning" | "error" {
  if (pct > 0.8) return "success";
  if (pct >= 0.4) return "warning";
  return "error";
}

/** EV fill fraction from the live sim snapshot: soc / target_soc, clamped to [0, 1]. */
function evFill(req: UserRequestWithSession, sim?: SimSnapshot): number | null {
  if (req.session?.type !== "ev" || !sim || !(req.asset_id in sim.assets)) return null;
  const soc = sim.assets[req.asset_id].soc;
  const target = req.session.target_soc;
  if (soc == null || !(target > 0)) return null;
  return Math.min(soc / target, 1);
}

/**
 * Energy the current plan schedules for the asset before the deadline (kWh).
 * Sums from the next slot boundary (slots already started are skipped so the
 * live fill value and the plan don't double-count the in-progress slot).
 */
function plannedEnergyKwh(plan: Plan, assetId: string, deadline: Date, now: Date): number {
  let kwh = 0;
  for (const slot of plan.slots ?? []) {
    const start = new Date(slot.start);
    if (start < now || start >= deadline) continue;
    const kw = slot.planned_kw_by_asset?.[assetId]
      ?? slot.allocations.find((a) => a.asset_id === assetId)?.power_kw
      ?? 0;
    kwh += kw * (new Date(slot.end).getTime() - start.getTime()) / 3_600_000;
  }
  return Math.max(kwh, 0);
}

/**
 * On-track verdict, only where the energy arithmetic is well-defined:
 * EV — plan envelope remainder vs planned energy to departure;
 * shiftable — requested runtime energy vs planned energy in its window;
 * heater — null (current → target temperature speaks for itself).
 */
function onTrack(req: UserRequestWithSession, plan?: Plan): boolean | null {
  if (!plan || req.status !== "ACTIVE") return null;
  const deadline = sessionDeadline(req);
  if (!deadline) return null;
  const now = new Date();
  if (req.session?.type === "ev") {
    const envelope = plan.envelopes?.find((e) => e.asset_id === req.asset_id);
    if (!envelope) return null;
    return plannedEnergyKwh(plan, req.asset_id, deadline, now) + 1e-6 >= envelope.energy_needed_kwh;
  }
  if (req.session?.type === "shiftable_load") {
    const targetKwh = req.session.power_kw * req.session.duration_min / 60;
    return plannedEnergyKwh(plan, req.asset_id, deadline, now) + 1e-6 >= targetKwh * 0.999;
  }
  return null;
}

// ─── Deadline countdown ───────────────────────────────────────────────────────

function DeadlineDisplay({ req }: { req: UserRequestWithSession }) {
  const deadline = sessionDeadline(req);
  if (!deadline) return null;

  // eslint-disable-next-line react-hooks/purity -- intentional: snapshot current time for deadline display; component re-renders on data poll
  const diffMs = deadline.getTime() - Date.now();

  if (diffMs <= 0 && req.status === "ACTIVE") {
    return (
      <Chip
        data-testid={`session-deadline-${req.id}`}
        label="OVERDUE"
        color="error"
        size="small"
        sx={{ fontWeight: "bold" }}
      />
    );
  }

  const totalMin = Math.floor(Math.max(diffMs, 0) / 60_000);
  const h = Math.floor(totalMin / 60);
  const m = totalMin % 60;
  const label = diffMs <= 0 ? "done" : h > 0 ? `T−${h}h ${m}m` : `T−${m}m`;

  return (
    <Typography
      data-testid={`session-deadline-${req.id}`}
      variant="caption"
      color="text.secondary"
    >
      {label}
    </Typography>
  );
}

// ─── Budget line (estimated cost vs user budget — no accumulated-cost source) ─

function BudgetLine({ req }: { req: UserRequestWithSession }) {
  const budget = req.budget_eur ?? req.max_total_cost_eur;
  if (budget == null) return null;

  const pct = budget > 0 ? (req.estimated_cost_eur / budget) * 100 : 0;
  const color = pct > 90 ? "error" : "primary";

  return (
    <Box data-testid={`session-budget-${req.id}`}>
      <Stack direction="row" justifyContent="space-between">
        <Typography variant="caption" color="text.secondary">Budget (est.)</Typography>
        <Typography variant="caption" color="text.secondary">
          €{req.estimated_cost_eur.toFixed(2)} / €{budget.toFixed(2)}
        </Typography>
      </Stack>
      <LinearProgress variant="determinate" value={Math.min(pct, 100)} color={color} sx={{ height: 4, borderRadius: 2 }} />
    </Box>
  );
}

// ─── Session card ─────────────────────────────────────────────────────────────

function SessionCard({ req, plan, sim }: { req: UserRequestWithSession; plan?: Plan; sim?: SimSnapshot }) {
  const [expanded, setExpanded] = useState(false);
  const fill = evFill(req, sim);
  const track = onTrack(req, plan);
  const heaterTemp = req.session?.type === "heater" && sim && req.asset_id in sim.assets
    ? sim.assets[req.asset_id].temp_c
    : null;
  const envelope = plan?.envelopes?.find((e) => e.asset_id === req.asset_id);

  return (
    <Paper
      data-testid={`session-card-${req.id}`}
      variant="outlined"
      sx={{ p: 1.5, mb: 1 }}
    >
      <Stack spacing={0.75}>
        {/* Header row */}
        <Stack direction="row" alignItems="center" justifyContent="space-between">
          <Typography variant="body2" fontWeight="medium">
            {deviceIcon(req)} {req.asset_id} <Typography component="span" variant="caption" color="text.secondary">…{req.id.slice(-6)}</Typography>
          </Typography>
          <Stack direction="row" alignItems="center" spacing={0.5}>
            {track != null && (
              <Chip
                data-testid={`session-ontrack-${req.id}`}
                label={track ? "on track" : "at risk"}
                color={track ? "success" : "warning"}
                size="small"
                variant="outlined"
              />
            )}
            <Chip label={req.status} color={statusColor(req.status)} size="small" />
            <IconButton
              size="small"
              data-testid={`session-expand-${req.id}`}
              onClick={() => setExpanded((e) => !e)}
              aria-label={expanded ? "Collapse session details" : "Expand session details"}
            >
              {expanded ? <ExpandLessIcon fontSize="small" /> : <ExpandMoreIcon fontSize="small" />}
            </IconButton>
          </Stack>
        </Stack>

        {/* Summary line */}
        <Typography variant="caption" color="text.secondary">
          {sessionSummary(req)}{req.mode !== "BY_DEADLINE" ? ` · ${req.mode}` : ""}
        </Typography>

        {/* EV fill gauge (live SoC vs target) */}
        {fill != null && (
          <Box>
            <Stack direction="row" justifyContent="space-between">
              <Typography variant="caption" color="text.secondary">Fill (live SoC)</Typography>
              <Typography variant="caption" color="text.secondary">{(fill * 100).toFixed(0)}%</Typography>
            </Stack>
            <LinearProgress
              data-testid={`session-fill-${req.id}`}
              data-color={fillColor(fill)}
              variant="determinate"
              value={fill * 100}
              color={fillColor(fill)}
              sx={{ height: 6, borderRadius: 3 }}
            />
          </Box>
        )}

        {/* Heater: current → target temperature (a % gauge would mislead) */}
        {heaterTemp != null && req.session?.type === "heater" && (
          <Typography data-testid={`session-temp-${req.id}`} variant="body2">
            {heaterTemp.toFixed(1)}°C → {req.session.target_temp_c}°C
          </Typography>
        )}

        {/* Deadline + remaining-energy row */}
        <Stack direction="row" alignItems="center" justifyContent="space-between">
          <DeadlineDisplay req={req} />
          <Typography variant="caption" color="text.secondary">
            {envelope
              ? `needs ${envelope.energy_needed_kwh.toFixed(1)} kWh · est €${envelope.estimated_cost_eur.toFixed(2)}`
              : `${req.target_energy_kwh} kWh @ ${req.desired_power_kw} kW`}
          </Typography>
        </Stack>

        {/* Budget line */}
        <BudgetLine req={req} />

        {/* Expanded: deadline tiers */}
        <Collapse in={expanded} unmountOnExit>
          <Box data-testid={`session-tiers-${req.id}`} sx={{ mt: 1 }}>
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
                {req.deadlines.map((tier, i) => (
                  <TableRow key={i}>
                    <TableCell sx={{ fontSize: "0.7rem" }}>{i}</TableCell>
                    <TableCell sx={{ fontSize: "0.7rem" }}>{new Date(tier.latest_end).toLocaleString()}</TableCell>
                    <TableCell sx={{ fontSize: "0.7rem" }}>{(tier.min_completion * 100).toFixed(0)}%</TableCell>
                    <TableCell sx={{ fontSize: "0.7rem" }}>{tier.max_total_cost_eur != null ? `€${tier.max_total_cost_eur.toFixed(2)}` : "—"}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </Box>
        </Collapse>
      </Stack>
    </Paper>
  );
}

// ─── Group section ────────────────────────────────────────────────────────────

function SessionGroup({
  testId, label, requests, plan, sim, defaultOpen,
}: {
  testId: string; label: string; requests: UserRequestWithSession[];
  plan?: Plan; sim?: SimSnapshot; defaultOpen: boolean;
}) {
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
        <Chip label={requests.length} size="small" />
        {open ? <ExpandLessIcon fontSize="small" /> : <ExpandMoreIcon fontSize="small" />}
      </Stack>
      <Collapse in={open}>
        {requests.map((r) => <SessionCard key={r.id} req={r} plan={plan} sim={sim} />)}
      </Collapse>
    </Box>
  );
}

// ─── Condensed chips (Dashboard strip, BL-36) ─────────────────────────────────

function CondensedChips({ requests, plan, sim }: { requests: UserRequestWithSession[]; plan?: Plan; sim?: SimSnapshot }) {
  return (
    <Stack direction="row" flexWrap="wrap" gap={0.5}>
      {requests.map((req) => {
        const fill = evFill(req, sim);
        const track = onTrack(req, plan);
        const deadline = sessionDeadline(req);
        // eslint-disable-next-line react-hooks/purity -- intentional: snapshot current time for countdown; re-renders on data poll
        const nowMs = Date.now();
        const overdue = deadline != null && deadline.getTime() < nowMs;
        const progress = fill != null
          ? `${(fill * 100).toFixed(0)}%`
          : req.session?.type === "heater" && sim && req.asset_id in sim.assets && sim.assets[req.asset_id].temp_c != null
            ? `${sim.assets[req.asset_id].temp_c!.toFixed(1)}°C→${req.session.target_temp_c}°C`
            : `${req.target_energy_kwh} kWh`;
        const countdown = deadline == null
          ? ""
          : overdue
            ? " · OVERDUE"
            : ` · T−${Math.floor((deadline.getTime() - nowMs) / 60_000)}m`;
        return (
          <Chip
            key={req.id}
            data-testid={`session-chip-${req.id}`}
            size="small"
            color={overdue ? "error" : track === false ? "warning" : "default"}
            label={`${deviceIcon(req)} ${req.asset_id} ${progress}${countdown}`}
          />
        );
      })}
    </Stack>
  );
}

// ─── Main component ───────────────────────────────────────────────────────────

type Props = {
  requests: UserRequestWithSession[];
  plan?: Plan;
  sim?: SimSnapshot;
  variant?: "full" | "condensed";
};

export function SessionProgressBoard({ requests, plan, sim, variant = "full" }: Props) {
  const active = requests.filter((r) => r.status === "ACTIVE");
  const done = requests.filter((r) => r.status !== "ACTIVE");

  if (variant === "condensed") {
    if (!active.length) {
      return (
        <Typography data-testid="session-board-empty" variant="caption" color="text.secondary">
          No active sessions.
        </Typography>
      );
    }
    return <CondensedChips requests={active} plan={plan} sim={sim} />;
  }

  if (!requests.length) {
    return (
      <Typography data-testid="session-board-empty" color="text.secondary">
        No active sessions.
      </Typography>
    );
  }

  return (
    <Box data-testid="session-board">
      <Stack spacing={2}>
        <SessionGroup testId="session-group-active" label="Active" requests={active} plan={plan} sim={sim} defaultOpen={true} />
        <SessionGroup testId="session-group-done" label="Done" requests={done} plan={plan} sim={sim} defaultOpen={false} />
      </Stack>
    </Box>
  );
}
