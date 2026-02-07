import { useMemo, useState } from "react";
import {
  List, ListItem, ListItemButton, ListItemText, Paper, Stack, TextField, Typography,
} from "@mui/material";
import type { Program } from "../api/types";
import { usePrograms } from "../api/hooks";
import { JsonDialog } from "../components/JsonDialog";

export function ProgramsPage() {
  const { data: programs = [], dataUpdatedAt } = usePrograms();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Program | null>(null);

  const filtered = useMemo(() => {
    return programs.filter((p) => {
      const hay = `${p.id} ${p.programName ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [programs, query]);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="programs-heading">
          Programs
        </Typography>
        <Typography variant="body2" color="text.secondary" data-testid="programs-last-updated">
          Last updated: {lastUpdated}
        </Typography>
      </div>

      <Paper sx={{ p: 2 }}>
        <TextField
          label="Search"
          size="small"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          fullWidth
          inputProps={{
            "data-testid": "programs-search",
            "aria-label": "Search programs",
          }}
        />
      </Paper>

      <Paper>
        <List dense data-testid="programs-list">
          {filtered.map((p) => (
            <ListItem key={p.id} disablePadding data-testid={`program-item-${p.id}`}>
              <ListItemButton onClick={() => setSelected(p)}>
                <ListItemText
                  primary={p.programName ?? p.id}
                  secondary={`${p.id}${p.createdDateTime ? ` — ${p.createdDateTime}` : ""}`}
                />
              </ListItemButton>
            </ListItem>
          ))}
          {filtered.length === 0 && (
            <ListItem data-testid="programs-empty">
              <ListItemText primary="No programs" />
            </ListItem>
          )}
        </List>
      </Paper>

      <JsonDialog
        open={!!selected}
        title={selected ? `Program: ${selected.programName ?? selected.id}` : "Program"}
        json={selected ?? {}}
        onClose={() => setSelected(null)}
      />
    </Stack>
  );
}
