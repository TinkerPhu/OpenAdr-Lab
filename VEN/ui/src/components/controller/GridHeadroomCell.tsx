import { useState } from "react";
import { Box, IconButton, Paper, Tooltip, Typography } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import UnfoldMoreIcon from "@mui/icons-material/UnfoldMore";
import UnfoldLessIcon from "@mui/icons-material/UnfoldLess";
import {
  CELL_CHART_MIN_WIDTH, CELL_LEFT_SECTION_WIDTH, DEFAULT_WINDOW, EXTENDED_WINDOW, CELL_CHART_HEIGHT_TALL,
} from "../charts/chartLayout";
import type { AssetTimelinePoint } from "./types";
import type { SiteFlexibilityEnvelope, SiteFlexibilitySample, SiteFlexibilityForecastSlot } from "../../api/types";
import { SiteHeadroomChart } from "./charts/SiteHeadroomChart";

interface GridHeadroomCellProps {
  envelope: SiteFlexibilityEnvelope | null | undefined;
  history: SiteFlexibilitySample[];
  forecast: SiteFlexibilityForecastSlot[];
  gridTimeline: AssetTimelinePoint[];
  nowMs: number;
  extended: boolean;
  pinned: boolean;
  onTogglePin: () => void;
}

function fmtKw(v: number | undefined): string {
  return v === undefined ? "—" : `${v.toFixed(2)} kW`;
}

function fmtDuration(s: number | null | undefined): string {
  if (s === undefined || s === null) return "—";
  const mins = Math.round(s / 60);
  return mins >= 60 ? `${(mins / 60).toFixed(1)} h` : `${mins} min`;
}

/**
 * BL-43: live site-level flexibility headroom — the VEN's own instant-only
 * `up_kw`/`down_kw` (no forward schedule, unlike the Dynamic Operating Envelope
 * in `GridTariffCell`). Follows the same sibling-cell pattern (pin/tall-toggle).
 */
export function GridHeadroomCell({
  envelope,
  history,
  forecast,
  gridTimeline,
  nowMs,
  extended,
  pinned,
  onTogglePin,
}: GridHeadroomCellProps) {
  const [tall, setTall] = useState(false);
  const window = extended ? EXTENDED_WINDOW : DEFAULT_WINDOW;

  return (
    <Paper
      variant="outlined"
      data-testid="grid-headroom-cell"
      sx={{ display: "flex", flexDirection: "row", mb: 1, borderLeft: "4px solid #8BC34A" }}
    >
      {/* Left: current headroom values */}
      <Box sx={{ minWidth: CELL_LEFT_SECTION_WIDTH, px: 1.5, py: 1, display: "flex", flexDirection: "column", gap: 0.5 }}>
        <Typography variant="body2" fontWeight="bold">
          Site Headroom
        </Typography>
        <Typography variant="caption" color="text.secondary" data-testid="headroom-up-kw">
          Up: {fmtKw(envelope?.up_kw)} ({fmtDuration(envelope?.up_duration_s)})
        </Typography>
        <Typography variant="caption" color="text.secondary" data-testid="headroom-down-kw">
          Down: {fmtKw(envelope?.down_kw)} ({fmtDuration(envelope?.down_duration_s)})
        </Typography>
      </Box>

      {/* Right: live headroom band around the grid-power line */}
      <Box sx={{ flex: 1, minWidth: CELL_CHART_MIN_WIDTH }}>
        <SiteHeadroomChart
          gridTimeline={gridTimeline}
          history={history}
          forecast={forecast}
          nowMs={nowMs}
          hoursBack={window.hoursBack}
          hoursForward={window.hoursForward}
          height={tall ? CELL_CHART_HEIGHT_TALL : undefined}
        />
      </Box>

      {/* Right column: pin button + vertical expand button */}
      <Box sx={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
        <Tooltip title={pinned ? "Unpin" : "Pin to top"}>
          <IconButton
            size="small"
            data-testid="grid-headroom-cell-pin-btn"
            onClick={onTogglePin}
            sx={{ m: 0.5 }}
          >
            {pinned ? <PushPinIcon fontSize="small" /> : <PushPinOutlinedIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
        <Tooltip title={tall ? "Collapse chart" : "Expand chart"}>
          <IconButton
            size="small"
            data-testid="grid-headroom-cell-tall-expand-btn"
            onClick={() => setTall((v) => !v)}
            sx={{ m: 0.5 }}
          >
            {tall ? <UnfoldLessIcon fontSize="small" /> : <UnfoldMoreIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
      </Box>
    </Paper>
  );
}
