import { useMemo } from "react";
import { Box, IconButton, Paper, Tooltip, Typography } from "@mui/material";
import PushPinIcon from "@mui/icons-material/PushPin";
import PushPinOutlinedIcon from "@mui/icons-material/PushPinOutlined";
import ZoomOutMapIcon from "@mui/icons-material/ZoomOutMap";
import ZoomInMapIcon from "@mui/icons-material/ZoomInMap";
import type { AssetId, AssetSummary, AssetTimelinePoint, StackedAreaPoint } from "./types";
import { ASSET_COLORS } from "./types";
import { StackedAreaChart } from "./charts/StackedAreaChart";

const DEFAULT_WINDOW = { hoursBack: 1.0, hoursForward: 1.0 };
const EXTENDED_WINDOW = { hoursBack: 1.0, hoursForward: 24.0 };

const KNOWN_ASSETS: AssetId[] = ["ev", "heater", "pv", "battery", "base_load"];

/** Binary search: index of the point with ts closest to `target` within `toleranceMs`. */
function findNearest(points: AssetTimelinePoint[], target: number, toleranceMs: number): AssetTimelinePoint | undefined {
  let lo = 0;
  let hi = points.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (points[mid].ts < target) lo = mid + 1;
    else hi = mid;
  }
  // lo is the first index with ts >= target; also check lo-1 for the closest
  const candidates = [points[lo - 1], points[lo]].filter(Boolean) as AssetTimelinePoint[];
  const best = candidates.reduce<AssetTimelinePoint | undefined>((prev, cur) => {
    if (!prev) return cur;
    return Math.abs(cur.ts - target) < Math.abs(prev.ts - target) ? cur : prev;
  }, undefined);
  return best && Math.abs(best.ts - target) <= toleranceMs ? best : undefined;
}

function buildStackedFromAllTimelines(
  allTimelines: Record<string, AssetTimelinePoint[]>
): StackedAreaPoint[] {
  // Collect timestamps from KNOWN_ASSETS only. The "grid" virtual asset and any
  // unknown entries are excluded because they have plan-slot entries at timestamps
  // where known assets have no allocation — causing those assets to fall through
  // to 0 on exact-match, producing false zero-spikes in the stacked chart.
  const tsSet = new Set<number>();
  for (const assetId of KNOWN_ASSETS) {
    for (const p of (allTimelines[assetId] ?? [])) tsSet.add(p.ts);
  }
  const sortedTs = [...tsSet].sort((a, b) => a - b);

  // Tolerance for nearest-neighbour lookup: half the typical sample interval (15 s).
  // All assets are pushed in the same sim tick, so any timestamp drift is sub-second.
  // Independent per-asset downsampling can shift timestamps by up to one stride (~30 s),
  // so 15 s catches genuine alignment while avoiding cross-slot false matches.
  const TOLERANCE_MS = 15_000;

  const emptyPt = (): Omit<StackedAreaPoint, "ts"> => ({
    ev_pos: 0, ev_neg: 0,
    heater_pos: 0, heater_neg: 0,
    pv_pos: 0, pv_neg: 0,
    battery_pos: 0, battery_neg: 0,
    base_load_pos: 0, base_load_neg: 0,
  });

  return sortedTs.map((ts) => {
    const pt: StackedAreaPoint = { ts, ...emptyPt() };
    for (const assetId of KNOWN_ASSETS) {
      const points = allTimelines[assetId];
      if (!points) continue;
      const match = findNearest(points, ts, TOLERANCE_MS);
      const kw = match?.values["power_kw"] ?? 0;
      const key = assetId as AssetId;
      pt[`${key}_pos` as keyof StackedAreaPoint] = Math.max(0, kw) as never;
      pt[`${key}_neg` as keyof StackedAreaPoint] = Math.min(0, kw) as never;
    }
    return pt;
  });
}

interface GridAccumulatedCellProps {
  assetSummaries: AssetSummary[];
  /** Timeline data from the shared useAllTimelines query on the page. */
  allTimelines: Record<string, AssetTimelinePoint[]>;
  /** Epoch ms — shared across all cells from the page for a consistent NOW line. */
  nowMs: number;
  /** Whether this cell's time window is expanded to 24h forward. */
  extended: boolean;
  pinned: boolean;
  onTogglePin: () => void;
  onToggleExpand: () => void;
}

export function GridAccumulatedCell({
  assetSummaries,
  allTimelines,
  nowMs,
  extended,
  pinned,
  onTogglePin,
  onToggleExpand,
}: GridAccumulatedCellProps) {
  const window = extended ? EXTENDED_WINDOW : DEFAULT_WINDOW;
  const tMin = nowMs - window.hoursBack * 3_600_000;
  const tMax = nowMs + window.hoursForward * 3_600_000;

  const stackedAreaPoints = useMemo(() => {
    const all = buildStackedFromAllTimelines(allTimelines);
    return all.filter((p) => p.ts >= tMin && p.ts <= tMax);
  }, [allTimelines, tMin, tMax]);

  const assetIds = assetSummaries.map((s) => s.assetId);

  return (
    <Paper
      variant="outlined"
      data-testid="grid-accumulated-cell"
      sx={{ display: "flex", flexDirection: "row", mb: 1, borderLeft: "4px solid #546e7a" }}
    >
      {/* Left: per-asset current power list */}
      <Box sx={{ minWidth: 160, px: 1.5, py: 1, display: "flex", flexDirection: "column", gap: 0.5 }}>
        <Typography variant="body2" fontWeight="bold">
          Accumulated Power
        </Typography>
        {assetSummaries.map((s) => (
          <Typography
            key={s.assetId}
            variant="caption"
            data-testid={`accumulated-power-${s.assetId}`}
            sx={{ color: ASSET_COLORS[s.assetId] }}
          >
            {s.label}: {s.powerKw >= 0 ? "+" : ""}
            {s.powerKw.toFixed(2)} kW
          </Typography>
        ))}
      </Box>

      {/* Right: stacked area chart */}
      <Box sx={{ flex: 1, minWidth: 200 }}>
        <StackedAreaChart
          data={stackedAreaPoints}
          assetIds={assetIds as AssetId[]}
          colorMap={ASSET_COLORS}
          nowMs={nowMs}
          hoursBack={window.hoursBack}
          hoursForward={window.hoursForward}
        />
      </Box>

      {/* Right column: pin button on top, expand toggle below */}
      <Box sx={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
        <Tooltip title={pinned ? "Unpin" : "Pin to top"}>
          <IconButton
            size="small"
            data-testid="grid-accumulated-cell-pin-btn"
            onClick={onTogglePin}
            sx={{ m: 0.5 }}
          >
            {pinned ? <PushPinIcon fontSize="small" /> : <PushPinOutlinedIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
        <Tooltip title={extended ? "Collapse to ±1h view" : "Expand to 24h planning horizon"}>
          <IconButton
            size="small"
            data-testid="grid-accumulated-cell-extend-btn"
            onClick={onToggleExpand}
            sx={{ m: 0.5 }}
          >
            {extended ? <ZoomInMapIcon fontSize="small" /> : <ZoomOutMapIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
      </Box>
    </Paper>
  );
}
