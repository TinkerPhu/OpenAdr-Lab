import { useMemo, useState } from "react";
import {
  List, ListItem, ListItemText, Paper, Stack, TextField, Typography,
} from "@mui/material";
import { usePrograms } from "../api/hooks";

export function ProgramsPage() {
  const { data: programs = [], dataUpdatedAt } = usePrograms();
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    return programs.filter((p) => {
      const hay = `${p.id} ${p.name ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [programs, query]);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  return (
    <Stack spacing={2}>
      <div>
        <Typography
          variant="h5"
          data-testid="programs-heading"
        >
          Programs
        </Typography>
        <Typography
          variant="body2"
          color="text.secondary"
          data-testid="programs-last-updated"
        >
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
            <ListItem key={p.id} data-testid={`program-item-${p.id}`}>
              <ListItemText
                primary={p.name ?? p.id}
                secondary={p.name ? p.id : undefined}
              />
            </ListItem>
          ))}
          {filtered.length === 0 && (
            <ListItem data-testid="programs-empty">
              <ListItemText primary="No programs" />
            </ListItem>
          )}
        </List>
      </Paper>
    </Stack>
  );
}
