import { Box, Typography } from "@mui/material";
import { CELL_CHART_HEIGHT } from "./chartLayout";

interface EmptyStateProps {
  /** Context-specific message — kept distinct per caller (e.g. "Add points to preview
   * the curve" vs "No tariff data"); only the layout/styling is shared here, not the
   * wording, since the message text carries real, chart-specific information. */
  message: string;
  height?: number;
  testId?: string;
}

/** Shared "no data" treatment for charts whose empty state is a centered message rather
 * than a 2-point placeholder that still renders axes/NOW-line machinery (e.g.
 * `TimeSeriesChart` compositions) — those two are different mechanisms serving different
 * needs, not duplication of the same one; this component only covers the message-based
 * case (single-series diagnostic/editor-preview charts with no meaningful empty axis to
 * show at all). */
export function EmptyState({ message, height = CELL_CHART_HEIGHT, testId }: EmptyStateProps) {
  return (
    <Box
      data-testid={testId}
      sx={{ height, display: "flex", alignItems: "center", justifyContent: "center" }}
    >
      <Typography color="text.secondary" variant="body2">
        {message}
      </Typography>
    </Box>
  );
}
