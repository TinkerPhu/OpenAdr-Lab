import { Box, Collapse, IconButton, Paper, Tooltip } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import type { AssetId, AssetSummary, AssetTimelinePoint } from "./types";
import { ASSET_COLORS, COLOR_ASSET_FALLBACK } from "./types";
import type { ZoneDef } from "../../api/types";
import { AssetLeftSection } from "./AssetLeftSection";
import { AssetMidSection } from "./AssetMidSection";
import { AssetRightSection } from "./AssetRightSection";
import type { SimSnapshot, SimInjectState } from "../../api/types";
import { DEFAULT_WINDOW, EXTENDED_WINDOW } from "./chartLayout";

interface AssetCellProps {
  assetId: AssetId;
  summary: AssetSummary;
  simSnapshot: SimSnapshot | undefined;
  simOverrides: SimInjectState | undefined;
  collapsed: { right: boolean };
  /** Timeline points from the shared useAllTimelines query on the page. */
  timePoints: AssetTimelinePoint[];
  /** Epoch ms — shared across all cells from the page for a consistent NOW line. */
  nowMs: number;
  /** Whether this cell's time window is expanded to 48h forward. */
  extended: boolean;
  pinned: boolean;
  zones?: ZoneDef[];
  onTogglePin: (cellId: string) => void;
  onToggleCollapse: (cellId: string, section: "left" | "right") => void;
  onOverrideChange: (patch: Partial<SimInjectState>) => void;
  onResetSoc: (assetId: string, soc: number, onDone: () => void) => void;
}

export function AssetCell({
  assetId,
  summary,
  simSnapshot,
  simOverrides,
  collapsed,
  timePoints,
  nowMs,
  extended,
  pinned,
  zones,
  onTogglePin,
  onToggleCollapse,
  onOverrideChange,
  onResetSoc,
}: AssetCellProps) {
  const cellId = `asset:${assetId}`;
  const color = ASSET_COLORS[assetId] ?? COLOR_ASSET_FALLBACK;

  // Display window: each cell clips its chart domain independently.
  // The shared useAllTimelines query on the page always fetches the widest
  // window needed (48h if any cell is expanded), so data is always available.
  const window = extended ? EXTENDED_WINDOW : DEFAULT_WINDOW;

  return (
    <Paper
      variant="outlined"
      data-testid={`asset-cell-${assetId}`}
      sx={{
        display: "flex",
        flexDirection: "row",
        borderLeft: `4px solid ${color}`,
        mb: 1,
        overflow: "hidden",
      }}
    >
      {/* Left section */}
      <AssetLeftSection summary={summary} />

      {/* Mid section — timeline graph */}
      <AssetMidSection
        assetId={assetId}
        timePoints={timePoints}
        color={color}
        nowMs={nowMs}
        hoursBack={window.hoursBack}
        hoursForward={window.hoursForward}
        zones={zones}
      />

      {/* Right section — simulation controls */}
      <Collapse in={!collapsed.right} orientation="horizontal" unmountOnExit>
        <Box data-testid={`asset-cell-${assetId}-right`}>
          <AssetRightSection
            assetId={assetId}
            simSnapshot={simSnapshot}
            overrides={simOverrides}
            onOverrideChange={onOverrideChange}
            onResetSoc={onResetSoc}
          />
        </Box>
      </Collapse>

      {/* Right column: pin button + settings expand button */}
      <Box sx={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
        <Tooltip title={pinned ? "Unpin cell" : "Pin cell to top"}>
          <IconButton
            size="small"
            data-testid={`asset-cell-${assetId}-pin-btn`}
            onClick={() => onTogglePin(cellId)}
            sx={{ m: 0.5 }}
          >
            {pinned ? <PushPinIcon fontSize="small" /> : <PushPinOutlinedIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
        <Tooltip title={collapsed.right ? "Expand settings" : "Collapse settings"}>
          <IconButton
            size="small"
            data-testid={`asset-cell-${assetId}-collapse-right`}
            aria-label={collapsed.right ? "Expand right" : "Collapse right"}
            onClick={() => onToggleCollapse(cellId, "right")}
            sx={{ m: 0.5 }}
          >
            {collapsed.right ? <ChevronLeftIcon fontSize="small" /> : <ChevronRightIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
      </Box>
    </Paper>
  );
}
