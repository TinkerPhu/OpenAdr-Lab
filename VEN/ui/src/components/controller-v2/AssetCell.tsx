import { Box, Collapse, IconButton, Paper, Tooltip } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import type { AssetId, AssetSummary, AssetTimePoint } from "./types";
import { ASSET_COLORS } from "./types";
import { AssetLeftSection } from "./AssetLeftSection";
import { AssetMidSection } from "./AssetMidSection";
import { AssetRightSection } from "./AssetRightSection";
import type { SimSnapshot, UserOverrides } from "../../api/types";

interface AssetCellProps {
  assetId: AssetId;
  summary: AssetSummary;
  timePoints: AssetTimePoint[];
  simSnapshot: SimSnapshot | undefined;
  simOverrides: UserOverrides | undefined;
  collapsed: { left: boolean; right: boolean };
  pinned: boolean;
  onTogglePin: (cellId: string) => void;
  onToggleCollapse: (cellId: string, section: "left" | "right") => void;
  onOverrideChange: (patch: Partial<UserOverrides>) => void;
}

export function AssetCell({
  assetId,
  summary,
  timePoints,
  simSnapshot,
  simOverrides,
  collapsed,
  pinned,
  onTogglePin,
  onToggleCollapse,
  onOverrideChange,
}: AssetCellProps) {
  const cellId = `asset:${assetId}`;
  const color = ASSET_COLORS[assetId] ?? "#888";

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
        nowMs={Date.now()}
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

      {/* Pin button */}
      <Tooltip title={pinned ? "Unpin cell" : "Pin cell to top"}>
        <IconButton
          size="small"
          data-testid={`asset-cell-${assetId}-pin-btn`}
          onClick={() => onTogglePin(cellId)}
          sx={{ alignSelf: "flex-start", m: 0.5 }}
        >
          {pinned ? <PushPinIcon fontSize="small" /> : <PushPinOutlinedIcon fontSize="small" />}
        </IconButton>
      </Tooltip>
    </Paper>
  );
}
