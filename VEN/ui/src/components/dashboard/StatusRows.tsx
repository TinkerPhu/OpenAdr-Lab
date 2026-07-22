import { useState } from "react";
import {
  Box, Collapse, IconButton, Paper, Stack, Typography,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import ExpandLessIcon from "@mui/icons-material/ExpandLess";
import FiberManualRecordIcon from "@mui/icons-material/FiberManualRecord";
import type { Plan, TaskStatusEntry, VtnStatus } from "../../api/types";

// WP-T8 (docs/history/project_journal.md, search "WP-T" §3.3): shared traffic-light row —
// a single-line healthy state, or a degraded state whose detail only shows
// once expanded. Same idiom PlanHeaderBar.tsx already uses for warnings.
type Severity = "ok" | "warn" | "neutral";

const SEVERITY_COLOR: Record<Severity, string> = {
  ok: "success.main",
  warn: "warning.main",
  neutral: "text.secondary",
};

function StatusRow({
  testId,
  severity,
  summary,
  detail,
}: {
  testId: string;
  severity: Severity;
  summary: React.ReactNode;
  detail?: React.ReactNode;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Box data-testid={testId}>
      <Stack direction="row" alignItems="center" spacing={1}>
        <FiberManualRecordIcon sx={{ color: SEVERITY_COLOR[severity], fontSize: 12 }} />
        <Typography data-testid={`${testId}-status`} variant="body2" sx={{ flex: 1 }}>
          {summary}
        </Typography>
        {detail && (
          <IconButton
            data-testid={`${testId}-expand`}
            size="small"
            onClick={() => setOpen((o) => !o)}
            aria-label={open ? "Collapse detail" : "Expand detail"}
          >
            {open ? <ExpandLessIcon fontSize="small" /> : <ExpandMoreIcon fontSize="small" />}
          </IconButton>
        )}
      </Stack>
      {detail && (
        <Collapse in={open} unmountOnExit>
          <Box data-testid={`${testId}-detail`} sx={{ pl: 3, pt: 0.5 }}>
            {detail}
          </Box>
        </Collapse>
      )}
    </Box>
  );
}

function formatAge(iso: string): string {
  const diffSec = Math.max(0, Math.floor((Date.now() - new Date(iso).getTime()) / 1000));
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  return `${Math.floor(diffMin / 60)}h ago`;
}

export function VtnConnectionRow({ vtnStatus }: { vtnStatus: VtnStatus | undefined }) {
  if (!vtnStatus) {
    return <StatusRow testId="dash-vtn" severity="neutral" summary="VTN Connection: unknown" />;
  }
  if (vtnStatus.connected) {
    const pollLabel = vtnStatus.last_success_ts ? `Last poll: ${formatAge(vtnStatus.last_success_ts)}` : "";
    return (
      <StatusRow
        testId="dash-vtn"
        severity="ok"
        summary={`VTN Connection: Connected${pollLabel ? `   ${pollLabel}` : ""}`}
      />
    );
  }
  return (
    <StatusRow
      testId="dash-vtn"
      severity="warn"
      summary="VTN Connection: Disconnected"
      detail={
        <Typography variant="caption" color="text.secondary">
          backoff {vtnStatus.current_backoff_s.toFixed(1)}s
          {vtnStatus.last_error ? `; last error: ${vtnStatus.last_error}` : ""}
        </Typography>
      }
    />
  );
}

export function PlanStatusRow({ plan }: { plan: Plan | null | undefined }) {
  if (!plan) {
    return <StatusRow testId="dash-plan" severity="neutral" summary="Plan status: waiting for planner to run" />;
  }
  if (plan.solve_status === "INFEASIBLE") {
    return (
      <StatusRow
        testId="dash-plan"
        severity="warn"
        summary={`Plan status: Infeasible (solved ${formatAge(plan.created_at)})`}
        detail={
          plan.warnings.length > 0 && (
            <Stack spacing={0.25}>
              {plan.warnings.map((w, i) => (
                <Typography key={i} variant="caption" color="text.secondary">
                  {w.severity}: {w.message}
                </Typography>
              ))}
            </Stack>
          )
        }
      />
    );
  }
  return (
    <StatusRow
      testId="dash-plan"
      severity="ok"
      summary={`Plan status: Optimal (solved ${formatAge(plan.created_at)})`}
    />
  );
}

export function TaskSummaryRow({ tasks }: { tasks: TaskStatusEntry[] }) {
  if (tasks.length === 0) {
    return <StatusRow testId="dash-tasks" severity="neutral" summary="Active tasks: no task status recorded yet" />;
  }
  // WP-T3's own rule (Tasks.tsx): healthy means restart_count === 0 — a
  // task's first still-running attempt has last_success === null and is not
  // degraded, so this must not key off last_success.
  const unhealthy = tasks.filter((t) => t.restart_count > 0);
  if (unhealthy.length === 0) {
    return (
      <StatusRow testId="dash-tasks" severity="ok" summary={`Active tasks: ${tasks.length}/${tasks.length} running`} />
    );
  }
  return (
    <StatusRow
      testId="dash-tasks"
      severity="warn"
      summary={`Active tasks: ${tasks.length - unhealthy.length}/${tasks.length} running`}
      detail={
        <Stack spacing={0.25}>
          {unhealthy.map((t) => (
            <Typography key={t.name} variant="caption" color="text.secondary">
              {t.name}: {t.restart_count} restart{t.restart_count === 1 ? "" : "s"}
            </Typography>
          ))}
        </Stack>
      }
    />
  );
}

export function DashboardStatusPanel({
  vtnStatus,
  plan,
  tasks,
}: {
  vtnStatus: VtnStatus | undefined;
  plan: Plan | null | undefined;
  tasks: TaskStatusEntry[];
}) {
  return (
    <Paper sx={{ p: 2 }} data-testid="dash-status-panel">
      <Stack spacing={1}>
        <VtnConnectionRow vtnStatus={vtnStatus} />
        <PlanStatusRow plan={plan} />
        <TaskSummaryRow tasks={tasks} />
      </Stack>
    </Paper>
  );
}
