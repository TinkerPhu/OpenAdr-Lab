import { useMemo, useState } from "react";
import {
  Box, Chip, Paper, Stack, Table, TableBody, TableCell, TableHead, TableRow,
  TextField, Typography, TableContainer
} from "@mui/material";
import { Event } from "../api/types";
import { JsonDialog } from "../components/JsonDialog";

export function EventsPage(props: {
  events: Event[];
  lastUpdated?: Date | null;
}) {
  const [statusFilter, setStatusFilter] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Event | null>(null);

  const statuses = useMemo(() => {
    const s = new Set<string>();
    props.events.forEach(e => e.status && s.add(e.status));
    return Array.from(s).sort();
  }, [props.events]);

  const filtered = useMemo(() => {
    return props.events.filter(e => {
      if (statusFilter && e.status !== statusFilter) return false;
      const hay = `${e.id} ${e.program_id ?? ""} ${e.status ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [props.events, statusFilter, query]);

  return (
    <Stack spacing={2}>
      <Box>
        <Typography variant="h5">Events</Typography>
        <Typography variant="body2" color="text.secondary">
          Last updated: {props.lastUpdated ? props.lastUpdated.toLocaleString() : "—"}
        </Typography>
      </Box>

      <Paper sx={{ p: 2 }}>
        <Stack direction={{ xs: "column", sm: "row" }} spacing={2} alignItems="center">
          <TextField
            label="Search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            size="small"
            fullWidth
          />
          <Stack direction="row" spacing={1} flexWrap="wrap">
            <Chip
              label="All"
              clickable
              color={!statusFilter ? "primary" : "default"}
              onClick={() => setStatusFilter(null)}
            />
            {statuses.map(s => (
              <Chip
                key={s}
                label={s}
                clickable
                color={statusFilter === s ? "primary" : "default"}
                onClick={() => setStatusFilter(s)}
              />
            ))}
          </Stack>
        </Stack>
      </Paper>

      <TableContainer component={Paper}>
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>ID</TableCell>
              <TableCell>Program</TableCell>
              <TableCell>Status</TableCell>
              <TableCell>Created</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {filtered.map(e => (
              <TableRow key={e.id} hover sx={{ cursor: "pointer" }} onClick={() => setSelected(e)}>
                <TableCell sx={{ fontFamily: "monospace" }}>{e.id}</TableCell>
                <TableCell>{e.program_id ?? "—"}</TableCell>
                <TableCell>{e.status ?? "—"}</TableCell>
                <TableCell>{e.created_at ?? "—"}</TableCell>
              </TableRow>
            ))}
            {filtered.length === 0 && (
              <TableRow>
                <TableCell colSpan={4} align="center">No events</TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </TableContainer>

      <JsonDialog
        open={!!selected}
        title={selected ? `Event ${selected.id}` : "Event"}
        json={selected?.raw ?? {}}
        onClose={() => setSelected(null)}
      />
    </Stack>
  );
}
