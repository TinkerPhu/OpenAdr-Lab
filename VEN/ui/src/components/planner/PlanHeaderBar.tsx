import { useState } from "react";
import {
  Box, Chip, Collapse, IconButton, Stack, Tooltip, Typography,
} from "@mui/material";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
import ErrorOutlineIcon from "@mui/icons-material/ErrorOutline";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import type { Plan } from "../../api/types";

// ─── Trigger badge color ──────────────────────────────────────────────────────

type MuiColor = "default" | "primary" | "secondary" | "info" | "success" | "warning" | "error";

function triggerColor(trigger: string): MuiColor {
  const map: Record<string, MuiColor> = {
    Periodic: "default",
    RateChange: "primary",
    CapacityChange: "warning",
    UserRequest: "secondary",
    Event: "success",
  };
  return map[trigger] ?? "default";
}

// ─── Age formatting ───────────────────────────────────────────────────────────

function formatAge(createdAt: string): string {
  const diffMs = Date.now() - new Date(createdAt).getTime();
  const diffSec = Math.floor(diffMs / 1000);
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  return `${diffHr}h ago`;
}

// ─── Severity chip color ──────────────────────────────────────────────────────

function severityColor(severity: string): MuiColor {
  if (severity === "CRITICAL") return "error";
  if (severity === "WARNING") return "warning";
  return "default";
}

// ─── Main component ───────────────────────────────────────────────────────────

type Props = { plan: Plan | null | undefined };

export function PlanHeaderBar({ plan }: Props) {
  const [warningsOpen, setWarningsOpen] = useState(false);

  if (!plan) {
    return (
      <Typography data-testid="plan-no-plan" color="text.secondary">
        No plan available — waiting for planner to run.
      </Typography>
    );
  }

  const hasWarnings = plan.warnings.length > 0;
  const isInfeasible = plan.solve_status === "INFEASIBLE";
  // GB-31 — a feasible plan the solver couldn't certify optimal (cut off by
  // the timeout or by hitting the configured MIP-gap tolerance): distinct
  // from both a clean Optimal solve and the no-plan-at-all Infeasible case.
  const isSuboptimal = plan.solve_status === "TIME_LIMIT" || plan.solve_status === "GAP_LIMIT";

  // R-97 — which phase stopped early, when the plan carries the split. This is the
  // distinction `solve_status` alone cannot make, and it is not cosmetic: a phase-1
  // TIME_LIMIT means an incumbent at an unknown gap (the achieved gap is not
  // observable through good_lp, R-65), so the plan may be materially suboptimal. A
  // phase-2 TIME_LIMIT is routine — phase 2 runs on a deliberately short budget and
  // its result is capped so it can never cost more than phase 1 — and on heater
  // sites it happens on nearly every cycle by design. Showing one warning for both
  // trained operators to ignore the one that matters.
  const phases = plan.phase_report;
  const phase1Capped = phases?.phase1_status === "TIME_LIMIT";
  const phase2Capped = phases?.phase2_status === "TIME_LIMIT";

  return (
    <Box data-testid="plan-header">
      {/* Main summary row */}
      <Stack direction="row" alignItems="center" flexWrap="wrap" gap={1}>
        {/* Infeasible-solve chip — distinct from the generic warnings badge below;
            a solver failure must not read the same as a minor plan caveat. */}
        {isInfeasible && (
          <Tooltip title="The MILP solver could not find a feasible plan; this is the infeasibility fallback, not an optimised schedule.">
            <Chip
              data-testid="plan-infeasible-chip"
              icon={<ErrorOutlineIcon fontSize="small" />}
              label="Infeasible"
              color="error"
              size="small"
            />
          </Tooltip>
        )}
        {phase1Capped && (
          <Tooltip title="Phase 1 (cost minimisation) ran out of time, so this plan is the best incumbent found at an unknown optimality gap and may be materially more expensive than necessary. Unlike a phase-2 time limit, this one affects plan quality.">
            <Chip
              data-testid="plan-phase1-capped-chip"
              icon={<WarningAmberIcon fontSize="small" />}
              label="Phase 1 hit time limit"
              color="warning"
              size="small"
            />
          </Tooltip>
        )}
        {isSuboptimal && (
          <Tooltip
            title={
              phase2Capped && !phase1Capped
                ? "Phase 2 (friction smoothing) used its whole budget, which is routine: it runs on a deliberately short time limit and its result is capped so it can never cost more than phase 1. Plan cost is unaffected."
                : plan.solve_status === "TIME_LIMIT"
                  ? "The solver found a feasible plan but was cut off by its time limit before certifying optimality."
                  : "The solver found a feasible plan within the configured MIP-gap tolerance but stopped before certifying full optimality."
            }
          >
            <Chip
              data-testid="plan-suboptimal-chip"
              icon={<WarningAmberIcon fontSize="small" />}
              label="Not certified optimal"
              color="warning"
              size="small"
            />
          </Tooltip>
        )}

        {/* Trigger badge */}
        <Chip
          data-testid="plan-trigger-badge"
          data-color={triggerColor(plan.trigger)}
          label={plan.trigger}
          color={triggerColor(plan.trigger)}
          size="small"
        />

        {/* Age */}
        <Typography data-testid="plan-age" variant="caption" color="text.secondary">
          {formatAge(plan.created_at)}
        </Typography>

        {/* Horizon */}
        {plan.slots.length > 0 && (() => {
          const horizonMs = new Date(plan.slots[plan.slots.length - 1].end).getTime()
            - new Date(plan.slots[0].start).getTime();
          const horizonHours = (horizonMs / 3_600_000).toFixed(1);
          return (
            <Typography data-testid="plan-firm-horizon" variant="caption" color="text.secondary">
              {horizonHours}h
            </Typography>
          );
        })()}

        {/* Cost */}
        <Typography data-testid="plan-cost" variant="caption">
          €{plan.summary.total_cost_eur.toFixed(2)}
        </Typography>

        {/* Import kWh */}
        <Typography data-testid="plan-import-kwh" variant="caption">
          {plan.summary.total_import_kwh.toFixed(1)} kWh
        </Typography>

        {/* CO₂ */}
        <Typography data-testid="plan-co2" variant="caption">
          {(plan.summary.total_co2_g / 1000).toFixed(2)} kg CO₂
        </Typography>

        {/* GB-25: solver diagnostics persisted on the plan itself, not just the transient
            PlanReady SSE event. Omitted (not shown as "—") when absent, same convention as
            the other optional chips on this row. */}
        {plan.solver_ms != null && (
          <Typography data-testid="plan-solver-ms" variant="caption" color="text.secondary">
            solve: {plan.solver_ms}ms
          </Typography>
        )}
        {/* R-97: the per-phase split, so "which half spent the time" is answerable
            without shell access to a container — the question the 2026-10 solve-cost
            investigation had to answer from docker logs. */}
        {phases != null && (
          <Tooltip title={`Phase 1 (cost): ${phases.phase1_ms}ms, ${phases.phase1_status}. Phase 2 (friction): ${phases.phase2_ms}ms, ${phases.phase2_status ?? "did not run"}.`}>
            <Typography data-testid="plan-phase-split" variant="caption" color="text.secondary">
              p1 {phases.phase1_ms}ms · p2 {phases.phase2_ms}ms
            </Typography>
          </Tooltip>
        )}
        {plan.mip_gap_target != null && (
          <Tooltip title="Solver's configured MIP-gap tolerance for this cycle — a proxy, not the achieved gap.">
            <Typography data-testid="plan-mip-gap-target" variant="caption" color="text.secondary">
              gap target: {(plan.mip_gap_target * 100).toFixed(1)}%
            </Typography>
          </Tooltip>
        )}

        {/* Warnings badge + expand button */}
        {hasWarnings && (
          <Tooltip title={`${plan.warnings.length} warning${plan.warnings.length > 1 ? "s" : ""}`}>
            <Stack direction="row" alignItems="center" spacing={0.25}>
              <Chip
                data-testid="plan-warnings-badge"
                icon={<WarningAmberIcon fontSize="small" />}
                label={plan.warnings.length}
                color="warning"
                size="small"
              />
              <IconButton
                data-testid="plan-warnings-expand"
                size="small"
                onClick={() => setWarningsOpen((o) => !o)}
                aria-label={warningsOpen ? "Collapse warnings" : "Expand warnings"}
              >
                {warningsOpen ? <ExpandLessIcon fontSize="small" /> : <ExpandMoreIcon fontSize="small" />}
              </IconButton>
            </Stack>
          </Tooltip>
        )}
      </Stack>

      {/* Warnings list */}
      {hasWarnings && (
        <Collapse in={warningsOpen} unmountOnExit>
          <Box sx={{ mt: 1, pl: 1 }}>
            {plan.warnings.map((w, i) => (
              <Stack
                key={i}
                data-testid={`plan-warning-${i}`}
                direction="row"
                alignItems="flex-start"
                spacing={1}
                sx={{ mb: 0.5 }}
              >
                <Chip label={w.severity} color={severityColor(w.severity)} size="small" sx={{ mt: 0.25 }} />
                {w.kind && (
                  <Chip
                    data-testid={`plan-warning-${i}-kind`}
                    label={w.kind}
                    variant="outlined"
                    size="small"
                    sx={{ mt: 0.25 }}
                  />
                )}
                <Box>
                  <Typography variant="caption" display="block">{w.message}</Typography>
                  {w.suggested_action && (
                    <Typography variant="caption" color="text.secondary" display="block">
                      → {w.suggested_action}
                    </Typography>
                  )}
                </Box>
              </Stack>
            ))}
          </Box>
        </Collapse>
      )}
    </Box>
  );
}
