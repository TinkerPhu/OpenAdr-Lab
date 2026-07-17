import { useState } from "react";
import {
  Box,
  Chip,
  List,
  ListItem,
  ListItemText,
  Stack,
  Typography,
} from "@mui/material";
import { useNotificationHistory } from "../api/hooks";
import type { UserNotification, UserNotificationSeverity } from "../api/types";

const SEVERITIES: UserNotificationSeverity[] = ["INFO", "WARN", "ALERT"];

function severityColor(s: UserNotificationSeverity): "default" | "warning" | "error" {
  switch (s) {
    case "ALERT":
      return "error";
    case "WARN":
      return "warning";
    default:
      return "default";
  }
}

/** 030: dedup-aware secondary line — single occurrences show one timestamp,
 * deduplicated rows show first-seen and last-seen. */
function occurrenceLine(n: UserNotification): string {
  const first = new Date(n.created_at).toLocaleString();
  if (n.count <= 1) return first;
  const last = new Date(n.last_seen_at).toLocaleString();
  return `first ${first} — last ${last}`;
}

/** 030: full persisted notification history with severity filter and
 * ×N rendering for deduplicated rows. The bell shows only the live ring;
 * this page serves everything the store kept. */
export function NotificationsPage() {
  const [severity, setSeverity] = useState<UserNotificationSeverity | undefined>();
  const { data: notifications = [], isLoading } = useNotificationHistory(severity);

  // The endpoint returns oldest-first; the page reads newest-first.
  const newestFirst = [...notifications].reverse();

  return (
    <Box>
      <Typography variant="h5" sx={{ mb: 2 }}>
        Notifications
      </Typography>
      <Stack direction="row" spacing={1} sx={{ mb: 2 }} aria-label="Severity filter">
        <Chip
          label="ALL"
          data-testid="severity-filter-all"
          color={severity === undefined ? "primary" : "default"}
          onClick={() => setSeverity(undefined)}
        />
        {SEVERITIES.map((s) => (
          <Chip
            key={s}
            label={s}
            data-testid={`severity-filter-${s.toLowerCase()}`}
            color={severity === s ? "primary" : "default"}
            onClick={() => setSeverity(s)}
          />
        ))}
      </Stack>
      {newestFirst.length === 0 ? (
        <Typography color="text.secondary" data-testid="notifications-history-empty">
          {isLoading ? "Loading…" : "No notifications"}
        </Typography>
      ) : (
        <List dense data-testid="notifications-history-list">
          {newestFirst.map((n) => (
            <ListItem
              key={n.id}
              data-testid={`notification-history-item-${n.id}`}
              alignItems="flex-start"
            >
              <Chip
                label={n.severity}
                size="small"
                color={severityColor(n.severity)}
                sx={{ mr: 1, mt: 0.5 }}
              />
              <ListItemText
                primary={n.count > 1 ? `${n.message} ×${n.count}` : n.message}
                secondary={occurrenceLine(n)}
              />
            </ListItem>
          ))}
        </List>
      )}
    </Box>
  );
}
