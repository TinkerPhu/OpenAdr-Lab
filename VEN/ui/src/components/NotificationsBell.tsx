import { useState } from "react";
import {
  Badge,
  Box,
  Chip,
  IconButton,
  List,
  ListItem,
  ListItemText,
  Popover,
  Typography,
} from "@mui/material";
import NotificationsIcon from "@mui/icons-material/Notifications";
import { useNotifications } from "../api/hooks";
import type { UserNotificationSeverity } from "../api/types";

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

/** WP4.3 (BL-20): app-bar bell with badge + notification feed panel. */
export function NotificationsBell() {
  const { data: notifications = [] } = useNotifications();
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);

  // The ring arrives oldest-first; show newest first in the panel.
  const newestFirst = [...notifications].reverse();

  return (
    <>
      <IconButton
        color="inherit"
        data-testid="notifications-bell"
        aria-label={`Notifications: ${notifications.length}`}
        onClick={(e) => setAnchor(e.currentTarget)}
      >
        <Badge badgeContent={notifications.length} color="error">
          <NotificationsIcon />
        </Badge>
      </IconButton>
      <Popover
        open={anchor !== null}
        anchorEl={anchor}
        onClose={() => setAnchor(null)}
        anchorOrigin={{ vertical: "bottom", horizontal: "right" }}
        transformOrigin={{ vertical: "top", horizontal: "right" }}
        data-testid="notifications-panel"
      >
        {newestFirst.length === 0 ? (
          <Box sx={{ p: 2 }}>
            <Typography color="text.secondary" data-testid="notifications-empty">
              No notifications
            </Typography>
          </Box>
        ) : (
          <List dense sx={{ width: 380, maxHeight: 420, overflow: "auto" }}>
            {newestFirst.map((n) => (
              <ListItem key={n.id} data-testid={`notification-item-${n.id}`} alignItems="flex-start">
                <Chip
                  label={n.severity}
                  size="small"
                  color={severityColor(n.severity)}
                  sx={{ mr: 1, mt: 0.5 }}
                />
                <ListItemText
                  primary={n.message}
                  secondary={new Date(n.created_at).toLocaleString()}
                />
              </ListItem>
            ))}
          </List>
        )}
      </Popover>
    </>
  );
}
