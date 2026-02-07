import { useMemo, useState } from "react";
import {
  List, ListItem, ListItemButton, ListItemText, Paper, Stack, TextField, Typography,
} from "@mui/material";
import type { Ven } from "../api/types";
import { useVens } from "../api/hooks";
import { JsonDialog } from "../components/JsonDialog";

export function VensPage() {
  const { data: vens = [], dataUpdatedAt } = useVens();
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Ven | null>(null);

  const filtered = useMemo(() => {
    return vens.filter((v) => {
      const hay = `${v.id} ${v.venName ?? ""}`.toLowerCase();
      return hay.includes(query.toLowerCase());
    });
  }, [vens, query]);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5" data-testid="vens-heading">
          VENs
        </Typography>
        <Typography variant="body2" color="text.secondary" data-testid="vens-last-updated">
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
            "data-testid": "vens-search",
            "aria-label": "Search VENs",
          }}
        />
      </Paper>

      <Paper>
        <List dense data-testid="vens-list">
          {filtered.map((v) => (
            <ListItem key={v.id} disablePadding data-testid={`ven-item-${v.id}`}>
              <ListItemButton onClick={() => setSelected(v)}>
                <ListItemText
                  primary={v.venName ?? v.id}
                  secondary={`${v.id}${v.createdDateTime ? ` — ${v.createdDateTime}` : ""}`}
                />
              </ListItemButton>
            </ListItem>
          ))}
          {filtered.length === 0 && (
            <ListItem data-testid="vens-empty">
              <ListItemText primary="No VENs" />
            </ListItem>
          )}
        </List>
      </Paper>

      <JsonDialog
        open={!!selected}
        title={selected ? `VEN: ${selected.venName ?? selected.id}` : "VEN"}
        json={selected ?? {}}
        onClose={() => setSelected(null)}
      />
    </Stack>
  );
}
