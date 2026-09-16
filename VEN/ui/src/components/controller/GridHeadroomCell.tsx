import { useState } from "react";
import { Box, IconButton, Paper, ToggleButton, ToggleButtonGroup, Tooltip, Typography } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import UnfoldMoreIcon from "@mui/icons-material/UnfoldMore";
import UnfoldLessIcon from "@mui/icons-material/UnfoldLess";
import {
  CELL_CHART_MIN_WIDTH, CELL_LEFT_SECTION_WIDTH, DEFAULT_WINDOW, EXTENDED_WINDOW, CELL_CHART_HEIGHT_TALL,
  DEFAULT_TICK_INTERVAL_MINUTES, EXTENDED_TICK_INTERVAL_MINUTES,
} from "../charts/chartLayout";
import type { AssetTimelinePoint } from "./types";
import type {
  CapacityCurvesResponse,
  SiteFlexibilityEnvelope,
  SiteFlexibilitySample,
  SiteFlexibilityForecastSlot,
} from "../../api/types";
import { SiteHeadroomChart } from "./charts/SiteHeadroomChart";
import { formatTs } from "./charts/tariffChartShared";
import { useCapacityCurvesAt } from "../../api/hooks";

/** What the chart cursor does: show values (tooltip only), or move the commitment curves'
 * start to the clicked plan slot. */
type CursorMode = "values" | "move-commitment-start";

interface GridHeadroomCellProps {
  envelope: SiteFlexibilityEnvelope | null | undefined;
  history: SiteFlexibilitySample[];
  forecast: SiteFlexibilityForecastSlot[];
  capacity?: CapacityCurvesResponse | null;
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
 *
 * "Move commitment start" cursor mode: clicking a future time anchors the dashed
 * Import/Export commitment curves at the plan slot it falls in
 * (`GET /flexibility/capacity?start=`, one request per click; the server snaps
 * the time to a slot and names the one it used). Clicking again moves it; clicking at or
 * before now puts the curves back at now. The caption names the start the server actually
 * used, so a tap works the same on touch screens.
 */
export function GridHeadroomCell({
  envelope,
  history,
  forecast,
  capacity = null,
  gridTimeline,
  nowMs,
  extended,
  pinned,
  onTogglePin,
}: GridHeadroomCellProps) {
  const [tall, setTall] = useState(false);
  const window = extended ? EXTENDED_WINDOW : DEFAULT_WINDOW;

  const [cursorMode, setCursorMode] = useState<CursorMode>("values");
  const [clickedStartMs, setClickedStartMs] = useState<number | null>(null);
  const moveMode = cursorMode === "move-commitment-start";
  // The clicked time itself is sent; the server alone decides which plan slot it falls in
  // (`plan_state_boundary_at`) and answers with the `start` it used.
  const requestedStartMs = moveMode ? clickedStartMs : null;
  const { data: curvesAtStart } = useCapacityCurvesAt(requestedStartMs);
  const anchored = requestedStartMs !== null && curvesAtStart ? curvesAtStart : null;
  const anchoredStartMs = anchored ? Date.parse(anchored.start) : null;
  // The server answers with the now-anchored curves when there's no plan slot to anchor at.
  const anchoredAtNow = anchoredStartMs !== null && anchoredStartMs <= nowMs;
  const commitmentStartMs = anchored && !anchoredAtNow ? anchoredStartMs : null;

  const selectCursorMode = (mode: CursorMode | null) => {
    if (mode === null) return; // exclusive group: re-clicking the active mode keeps it
    setCursorMode(mode);
    setClickedStartMs(null);
  };
  /** Click a future time to anchor the curves there; click at or before now to put them
   * back at now. */
  const moveCommitmentStart = (tsMs: number) => setClickedStartMs(tsMs > nowMs ? tsMs : null);

  const caption = !moveMode
    ? null
    : anchoredAtNow || forecast.length === 0
      ? "No active plan — commitment curves start now"
      : commitmentStartMs !== null
        ? `Commitment start: ${formatTs(commitmentStartMs)} (plan slot) · click again to move it, or click the past to reset`
        : "Click a future time to move the commitment start";

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
        {/* Next to the values, not under the chart: below the plot the switch reads as a
            legend caption and goes unnoticed (reported from the live page). */}
        <ToggleButtonGroup
          size="small"
          exclusive
          value={cursorMode}
          onChange={(_, mode: CursorMode | null) => selectCursorMode(mode)}
          aria-label="Chart cursor"
          sx={{ mt: 0.5 }}
        >
          <ToggleButton value="values" sx={{ py: 0, fontSize: 11, textTransform: "none" }}>
            Values
          </ToggleButton>
          <ToggleButton
            value="move-commitment-start"
            aria-label="Move commitment start"
            sx={{ py: 0, fontSize: 11, textTransform: "none" }}
          >
            Move start
          </ToggleButton>
        </ToggleButtonGroup>
      </Box>

      {/* Right: live headroom band around the grid-power line */}
      <Box sx={{ flex: 1, minWidth: CELL_CHART_MIN_WIDTH }}>
        <SiteHeadroomChart
          gridTimeline={gridTimeline}
          history={history}
          forecast={forecast}
          capacity={commitmentStartMs !== null ? anchored : capacity}
          nowMs={nowMs}
          hoursBack={window.hoursBack}
          hoursForward={window.hoursForward}
          height={tall ? CELL_CHART_HEIGHT_TALL : undefined}
          xAxisTickIntervalMinutes={extended ? EXTENDED_TICK_INTERVAL_MINUTES : DEFAULT_TICK_INTERVAL_MINUTES}
          commitmentStartMs={commitmentStartMs}
          onCursorClick={moveMode ? moveCommitmentStart : undefined}
        />
        {caption && (
          <Box sx={{ px: 1, pb: 0.5 }}>
            <Typography variant="caption" color="text.secondary" data-testid="commitment-start-caption">
              {caption}
            </Typography>
          </Box>
        )}
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
