import {
  Chip, Paper, Stack, Table, TableBody, TableCell, TableContainer,
  TableHead, TableRow, Typography,
} from "@mui/material";
import { useEventLog } from "../api/hooks";

// WP-T4 (docs/plans/ven-ui-transparency.md): VEN-operational diagnostic
// trail — deliberately separate from the Notifications page/feed (different
// backend store, different route, no dedup). Newest entries render first
// since this is a log to scan, not a feed to read in order.

function categoryColor(category: string): "warning" | "error" | "default" {
  if (category === "task_supervisor") return "error";
  if (category === "vtn_connection" || category === "storage") return "warning";
  return "default";
}

export function EventLogPage() {
  const { data: entries = [], dataUpdatedAt } = useEventLog();
  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";
  const newestFirst = [...entries].reverse();

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="event-log-heading">
          Event Log
        </Typography>
        <Typography variant="body2" color="text.secondary" data-testid="event-log-last-updated">
          Last updated: {lastUpdated} (auto-refresh 10s)
        </Typography>
      </div>

      {newestFirst.length === 0 && (
        <Paper sx={{ p: 2 }}>
          <Typography color="text.secondary" data-testid="event-log-empty">
            No events recorded yet
          </Typography>
        </Paper>
      )}

      {newestFirst.length > 0 && (
        <TableContainer component={Paper}>
          <Table size="small" data-testid="event-log-table">
            <TableHead>
              <TableRow>
                <TableCell>Category</TableCell>
                <TableCell>Message</TableCell>
                <TableCell>Time</TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              {newestFirst.map((entry) => (
                <TableRow key={entry.id} data-testid={`event-log-row-${entry.id}`}>
                  <TableCell>
                    <Chip
                      data-testid={`event-log-category-${entry.id}`}
                      label={entry.category}
                      color={categoryColor(entry.category)}
                      size="small"
                    />
                  </TableCell>
                  <TableCell sx={{ fontFamily: "monospace", fontSize: "0.85rem" }}>
                    {entry.message}
                  </TableCell>
                  <TableCell>{new Date(entry.created_at).toLocaleString()}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableContainer>
      )}
    </Stack>
  );
}
