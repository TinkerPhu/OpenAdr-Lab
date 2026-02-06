import { useMemo, useState } from "react";
import { Paper, Stack, TextField, Typography, List, ListItem, ListItemText } from "@mui/material";
import { Program } from "../api/types";

export function ProgramsPage(props: { programs: Program[]; lastUpdated?: Date | null }) {
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    return props.programs.filter(p => {
      const hay = `${p.id} ${p.name ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [props.programs, query]);

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5">Programs</Typography>
        <Typography variant="body2" color="text.secondary">
          Last updated: {props.lastUpdated ? props.lastUpdated.toLocaleString() : "—"}
        </Typography>
      </div>

      <Paper sx={{ p: 2 }}>
        <TextField
          label="Search"
          size="small"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          fullWidth
        />
      </Paper>

      <Paper>
        <List dense>
          {filtered.map(p => (
            <ListItem key={p.id}>
              <ListItemText
                primary={p.name ?? p.id}
                secondary={p.name ? p.id : undefined}
              />
            </ListItem>
          ))}
          {filtered.length === 0 && (
            <ListItem>
              <ListItemText primary="No programs" />
            </ListItem>
          )}
        </List>
      </Paper>
    </Stack>
  );
}
