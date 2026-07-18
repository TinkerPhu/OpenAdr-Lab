import {
  Chip, Paper, Stack, Table, TableBody, TableCell, TableContainer,
  TableHead, TableRow, Typography,
} from "@mui/material";
import { useTasksStatus } from "../api/hooks";

// WP-T3 (docs/plans/ven-ui-transparency.md): the "healthy" signal is
// restart_count === 0, not last_success — last_success is legitimately null
// for a task on its first, still-running attempt (these tasks loop forever
// by design and only return to the supervisor when they panic).
function formatTimestamp(ts: string | null): string {
  return ts ? new Date(ts).toLocaleString() : "—";
}

export function TasksPage() {
  const { data: tasks = [], dataUpdatedAt } = useTasksStatus();
  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="tasks-heading">
          Background Tasks
        </Typography>
        <Typography variant="body2" color="text.secondary" data-testid="tasks-last-updated">
          Last updated: {lastUpdated} (auto-refresh 10s)
        </Typography>
      </div>

      {tasks.length === 0 && (
        <Paper sx={{ p: 2 }}>
          <Typography color="text.secondary" data-testid="tasks-empty">
            No task status recorded yet
          </Typography>
        </Paper>
      )}

      {tasks.length > 0 && (
        <TableContainer component={Paper}>
          <Table size="small" data-testid="tasks-table">
            <TableHead>
              <TableRow>
                <TableCell>Task</TableCell>
                <TableCell>Last run</TableCell>
                <TableCell>Last outcome</TableCell>
                <TableCell align="right">Restarts</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {tasks.map((task) => {
                const healthy = task.restart_count === 0;
                return (
                  <TableRow key={task.name} data-testid={`task-row-${task.name}`}>
                    <TableCell sx={{ fontFamily: "monospace" }}>{task.name}</TableCell>
                    <TableCell>{formatTimestamp(task.last_run_ts)}</TableCell>
                    <TableCell>
                      {task.last_success === null
                        ? "running"
                        : task.last_success
                          ? "exited normally"
                          : "panicked"}
                    </TableCell>
                    <TableCell align="right">
                      <Chip
                        data-testid={`task-restart-chip-${task.name}`}
                        label={task.restart_count}
                        color={healthy ? "success" : "warning"}
                        size="small"
                      />
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </TableContainer>
      )}
    </Stack>
  );
}
