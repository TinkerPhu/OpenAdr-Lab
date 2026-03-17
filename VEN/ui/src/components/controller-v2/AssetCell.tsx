import { useMemo, useState } from "react";
import { Box, Collapse, IconButton, Paper, Tooltip } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import ChevronLeftIcon from "@mui/icons-material/ChevronLeft";
import ChevronRightIcon from "@mui/icons-material/ChevronRight";
import ZoomOutMapIcon from "@mui/icons-material/ZoomOutMap";
import ZoomInMapIcon from "@mui/icons-material/ZoomInMap";
import type { AssetId, AssetSummary } from "./types";
import { ASSET_COLORS } from "./types";
import { AssetLeftSection } from "./AssetLeftSection";
import { AssetMidSection } from "./AssetMidSection";
import { AssetRightSection } from "./AssetRightSection";
import type { SimSnapshot, UserOverrides } from "../../api/types";
import { useTimeline } from "../../api/hooks";

/**
 * Assets with a non-default extended time window.
 * Assets not listed here have no extend toggle button.
 * Default window is ±1h. Extended windows are useful for planning-horizon assets.
 */
const EXTENDED_WINDOWS: Partial<Record<AssetId, { hoursBack: number; hoursForward: number }>> = {
  ev: { hoursBack: 1.0, hoursForward: 24.0 },
  battery: { hoursBack: 1.0, hoursForward: 24.0 },
};

const DEFAULT_WINDOW = { hoursBack: 1.0, hoursForward: 1.0 };

interface AssetCellProps {
  assetId: AssetId;
  summary: AssetSummary;
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

  const extendedWindow = EXTENDED_WINDOWS[assetId];
  const [extended, setExtended] = useState(false);

  const window = extended && extendedWindow ? extendedWindow : DEFAULT_WINDOW;

  const { data: timelineData = [] } = useTimeline(assetId, window.hoursBack, window.hoursForward);
  // nowMs updates each time fresh timeline data arrives (every 10s refetch cycle).
  const nowMs = useMemo(() => Date.now(), [timelineData]);

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
        timePoints={timelineData}
        color={color}
        nowMs={nowMs}
        hoursBack={window.hoursBack}
        hoursForward={window.hoursForward}
      />

      {/* Extended window toggle — only shown for assets in EXTENDED_WINDOWS */}
      {extendedWindow && (
        <Tooltip title={extended ? "Collapse to ±1h view" : "Expand to 24h planning horizon"}>
          <IconButton
            size="small"
            data-testid={`asset-cell-${assetId}-extend-btn`}
            onClick={() => setExtended((v) => !v)}
            sx={{ alignSelf: "center", mx: 0.5 }}
          >
            {extended ? <ZoomInMapIcon fontSize="small" /> : <ZoomOutMapIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
      )}

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
