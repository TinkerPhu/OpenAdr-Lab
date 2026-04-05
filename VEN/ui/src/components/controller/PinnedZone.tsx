import { Box } from "@mui/material";
import type { ReactNode } from "react";

interface PinnedZoneProps {
  pinnedCellIds: string[];
  children: ReactNode;
}

export function PinnedZone({ pinnedCellIds, children }: PinnedZoneProps) {
  if (pinnedCellIds.length === 0) return null;

  return (
    <Box
      data-testid="pinned-zone"
      sx={{
        position: "sticky",
        top: 0,
        zIndex: 100,
        bgcolor: "background.paper",
        borderBottom: "1px solid",
        borderColor: "divider",
        pb: 0.5,
      }}
    >
      {children}
    </Box>
  );
}
