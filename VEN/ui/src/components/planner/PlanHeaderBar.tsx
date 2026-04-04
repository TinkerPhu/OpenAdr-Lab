import { useState } from "react";
import {
  Box, Chip, Collapse, IconButton, Stack, Tooltip, Typography,
} from "@mui/material";
import WarningAmberIcon from "@mui/icons-material/WarningAmber";
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

  return (
    <Box data-testid="plan-header">
      {/* Main summary row */}
      <Stack direction="row" alignItems="center" flexWrap="wrap" gap={1}>
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

        {/* FIRM horizon */}
        {plan.firm_slots.length > 0 && (() => {
          const firmMs = new Date(plan.firm_slots[plan.firm_slots.length - 1].end).getTime()
            - new Date(plan.firm_slots[0].start).getTime();
          const firmHours = (firmMs / 3_600_000).toFixed(1);
          return (
            <Typography data-testid="plan-firm-horizon" variant="caption" color="text.secondary">
              FIRM: {firmHours}h
            </Typography>
          );
        })()}

        {/* Cost */}
        <Typography data-testid="plan-cost" variant="caption">
          €{plan.firm_summary.total_cost_eur.toFixed(2)}
        </Typography>

        {/* Import kWh */}
        <Typography data-testid="plan-import-kwh" variant="caption">
          {plan.firm_summary.total_import_kwh.toFixed(1)} kWh
        </Typography>

        {/* CO₂ */}
        <Typography data-testid="plan-co2" variant="caption">
          {(plan.firm_summary.total_co2_g / 1000).toFixed(2)} kg CO₂
        </Typography>

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
