import { useState } from "react";
import { Box, Collapse, IconButton, Paper, Tooltip } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import ZoomOutMapIcon from "@mui/icons-material/ZoomOutMap";
import ZoomInMapIcon from "@mui/icons-material/ZoomInMap";
import type { AssetId, AssetSummary, AssetTimelinePoint } from "./types";
import { ASSET_COLORS } from "./types";
import { AssetLeftSection } from "./AssetLeftSection";
import { AssetMidSection } from "./AssetMidSection";
import { AssetRightSection } from "./AssetRightSection";
import type { SimSnapshot, UserOverrides } from "../../api/types";

const DEFAULT_WINDOW = { hoursBack: 1.0, hoursForward: 1.0 };
const EXTENDED_WINDOW = { hoursBack: 1.0, hoursForward: 24.0 };

interface AssetCellProps {
  assetId: AssetId;
  summary: AssetSummary;
  simSnapshot: SimSnapshot | undefined;
  simOverrides: UserOverrides | undefined;
  collapsed: { left: boolean; right: boolean };
  /** Timeline points from the shared useAllTimelines query on the page. */
  timePoints: AssetTimelinePoint[];
  /** Epoch ms — shared across all cells from the page for a consistent NOW line. */
  nowMs: number;
  /** Whether this cell's time window is expanded to 24h forward. */
  extended: boolean;
  pinned: boolean;
  onTogglePin: (cellId: string) => void;
  onToggleCollapse: (cellId: string, section: "left" | "right") => void;
  onToggleExpand: (cellId: string) => void;
  onOverrideChange: (patch: Partial<UserOverrides>) => void;
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
  onTogglePin,
  onToggleCollapse,
  onToggleExpand,
  onOverrideChange,
}: AssetCellProps) {
  const cellId = `asset:${assetId}`;
  const color = ASSET_COLORS[assetId] ?? "#888";

  // Display window: each cell clips its chart domain independently.
  // The shared useAllTimelines query on the page always fetches the widest
  // window needed (24h if any cell is expanded), so data is always available.
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
      <Collapse in={!collapsed.left} orientation="horizontal">
        <AssetLeftSection summary={summary} />
      </Collapse>

      {/* Collapse left toggle */}
      <Tooltip title={collapsed.left ? "Expand left" : "Collapse left"}>
        <IconButton
          size="small"
          data-testid={`asset-cell-${assetId}-collapse-left`}
          onClick={() => onToggleCollapse(cellId, "left")}
          sx={{ alignSelf: "center", mx: 0.5 }}
        >
          {collapsed.left ? <ChevronRightIcon fontSize="small" /> : <ChevronLeftIcon fontSize="small" />}
        </IconButton>
      </Tooltip>

      {/* Mid section — timeline graph */}
      <AssetMidSection
        assetId={assetId}
        timePoints={timePoints}
        color={color}
        nowMs={nowMs}
        hoursBack={window.hoursBack}
        hoursForward={window.hoursForward}
      />

      {/* Collapse right toggle */}
      <Tooltip title={collapsed.right ? "Expand right" : "Collapse right"}>
        <IconButton
          size="small"
          data-testid={`asset-cell-${assetId}-collapse-right`}
          onClick={() => onToggleCollapse(cellId, "right")}
          sx={{ alignSelf: "center", mx: 0.5 }}
        >
          {collapsed.right ? <ChevronLeftIcon fontSize="small" /> : <ChevronRightIcon fontSize="small" />}
        </IconButton>
      </Tooltip>

      {/* Right section — simulation controls */}
      <Collapse in={!collapsed.right} orientation="horizontal">
        <Box data-testid={`asset-cell-${assetId}-right`}>
          <AssetRightSection
            assetId={assetId}
            simSnapshot={simSnapshot}
            overrides={simOverrides}
            onOverrideChange={onOverrideChange}
          />
        </Box>
      </Collapse>

      {/* Right column: pin button on top, expand toggle below */}
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
        <Tooltip title={extended ? "Collapse to ±1h view" : "Expand to 24h planning horizon"}>
          <IconButton
            size="small"
            data-testid={`asset-cell-${assetId}-extend-btn`}
            onClick={() => onToggleExpand(cellId)}
            sx={{ m: 0.5 }}
          >
            {extended ? <ZoomInMapIcon fontSize="small" /> : <ZoomOutMapIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
      </Box>
    </Paper>
  );
}
