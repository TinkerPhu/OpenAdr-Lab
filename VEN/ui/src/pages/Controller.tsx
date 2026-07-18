import { useState, useMemo, useEffect } from "react";
import { Alert, Box, CircularProgress, IconButton, Tooltip, Typography } from "@mui/material";
import ZoomOutMapIcon from "@mui/icons-material/ZoomOutMap";
import ZoomInMapIcon from "@mui/icons-material/ZoomInMap";
import { useSim, useTariffs, useRequests, useSimInject, useSetSimInject, useResetAssetSoc, useAllTimelines, useSimSchema } from "../api/hooks";
import type { AssetId, CollapseState } from "../components/controller/types";
import { deriveAssetSummaries, deriveTariffSnapshot } from "../components/controller/dataBuilders";
import { enrichAllAssetTimelines } from "../components/controller/tariffBuilders";
import { AssetCell } from "../components/controller/AssetCell";
import { PinnedZone } from "../components/controller/PinnedZone";
import { GridTariffCell } from "../components/controller/GridTariffCell";
import { GridSignalStrip } from "../components/controller/GridSignalStrip";
import { GridAccumulatedCell } from "../components/controller/GridAccumulatedCell";
import { FlexibilityForecastPanel } from "../components/controller/FlexibilityForecastPanel";
import type { SimInjectState } from "../api/types";

export function ControllerPage() {
  const { data: sim, isLoading: simLoading, isError: simError, refetch: refetchSim } = useSim({ refetchInterval: false });
  const { data: rates, refetch: refetchTariffs } = useTariffs({ refetchInterval: false });
  const { data: userRequests, refetch: refetchRequests } = useRequests({ refetchInterval: false });
  const { data: simInject } = useSimInject();
  const { mutate: setSimInject } = useSetSimInject();
  const { mutate: resetAssetSoc } = useResetAssetSoc();
  // Prefetch sim schema so controls are available instantly when right sections expand.
  useSimSchema();

  const [pinnedCellIds, setPinnedCellIds] = useState<string[]>([]);
  const [collapseState, setCollapseState] = useState<CollapseState>({});
  const [expanded, setExpanded] = useState(false);

  // Widen the timeline window when the global expand toggle is active.
  const hoursForward = expanded ? 48.0 : 1.0;

  // Single timeline query shared by all asset cells and the accumulated cell.
  // refetchInterval: false — driven exclusively by the unified timer below so
  // all data sources update in the same tick.
  const { data: allTimelinesResponse, refetch: refetchTimelines } = useAllTimelines(
    1.0,
    hoursForward,
    { refetchInterval: false }
  );
  // Stable references — only recompute when the response object itself changes.
  const allTimelines = useMemo(
    () => allTimelinesResponse?.timelines ?? {},
    [allTimelinesResponse]
  );
  const zones = useMemo(
    () => allTimelinesResponse?.zones ?? [],
    [allTimelinesResponse]
  );

  // Shared nowMs: advances only when fresh timeline data arrives, keeping the
  // NOW reference line consistent across all charts in the same render.
  // eslint-disable-next-line react-hooks/purity -- intentional: captures wall time at moment timeline data updates
  const nowMs = useMemo(() => Date.now(), [allTimelines]);

  // Single poll timer — all sources refresh in one tick.
  useEffect(() => {
    const id = setInterval(() => {
      refetchSim();
      refetchTariffs();
      refetchRequests();
      refetchTimelines();
    }, 2_000);
    return () => clearInterval(id);
  }, [refetchSim, refetchTariffs, refetchRequests, refetchTimelines]);

  function handleTogglePin(cellId: string) {
    setPinnedCellIds((prev) =>
      prev.includes(cellId) ? prev.filter((id) => id !== cellId) : [...prev, cellId]
    );
  }

  function handleToggleExpand() {
    setExpanded((prev) => !prev);
  }

  function handleToggleCollapse(cellId: string, section: "left" | "right") {
    if (section !== "right") return;
    setCollapseState((prev) => {
      const current = prev[cellId] ?? { rightCollapsed: true };
      return {
        ...prev,
        [cellId]: { rightCollapsed: !current.rightCollapsed },
      };
    });
  }

  function handleOverrideChange(patch: Partial<SimInjectState>) {
    setSimInject(patch);
  }

  function handleResetSoc(assetId: string, soc: number, onDone: () => void) {
    resetAssetSoc({ assetId, soc }, { onSuccess: onDone });
  }

  const tariffs = rates ?? [];
  const requests = userRequests ?? [];

  // Enrich asset timelines with cost_rate_eur_h / co2_rate_g_h for history/now-point.
  // Requires allTimelines so gridFraction can be computed per timestamp — PV-covered
  // load is correctly costed at zero, and export produces negative (revenue) rates.
  const enrichedTimelines = useMemo(
    () => enrichAllAssetTimelines(allTimelines, tariffs),
    [allTimelines, tariffs]
  );

  const assetSummaries = useMemo(() => {
    if (!sim) return [];
    // eslint-disable-next-line react-hooks/purity -- intentional: passes current wall time into pure summary derivation
    return deriveAssetSummaries(sim, tariffs, requests, allTimelines, Date.now());
  }, [sim, tariffs, requests, allTimelines]);

  const tariffSnapshot = useMemo(() => {
    if (!sim) return null;
    // eslint-disable-next-line react-hooks/purity -- intentional: passes current wall time into pure snapshot derivation
    return deriveTariffSnapshot(sim, tariffs, Date.now());
  }, [sim, tariffs]);

  if (simLoading) {
    return (
      <Box data-testid="controller-page" sx={{ p: 4, textAlign: "center" }}>
        <CircularProgress />
      </Box>
    );
  }

  if (simError || !sim) {
    return (
      <Box data-testid="controller-page" sx={{ p: 4 }}>
        <Alert severity="error">
          Unable to load simulation data. Check that the VEN is reachable.
        </Alert>
      </Box>
    );
  }

  // Build pinned cells React elements
  const pinnedElements = pinnedCellIds.map((cellId) => {
    if (cellId === "grid:tariff") {
      if (!tariffSnapshot) return null;
      return (
        <GridTariffCell
          key={cellId}
          snapshot={tariffSnapshot}
          gridTimeline={allTimelines["grid"] ?? []}
          nowMs={nowMs}
          extended={expanded}
          pinned
          zones={zones}
          onTogglePin={() => handleTogglePin("grid:tariff")}
        />
      );
    }
    if (cellId === "grid:accumulated") {
      return (
        <GridAccumulatedCell
          key={cellId}
          assetSummaries={assetSummaries}
          allTimelines={allTimelines}
          nowMs={nowMs}
          extended={expanded}
          pinned
          gridPowerKw={sim.grid.net_power_w / 1000}
          zones={zones}
          onTogglePin={() => handleTogglePin("grid:accumulated")}
        />
      );
    }
    if (cellId.startsWith("asset:")) {
      const assetId = cellId.replace("asset:", "") as AssetId;
      const summary = assetSummaries.find((s) => s.assetId === assetId);
      if (!summary) return null;
      const collapsed = collapseState[cellId] ?? { rightCollapsed: true };
      return (
        <AssetCell
          key={cellId}
          assetId={assetId}
          summary={summary}
          simSnapshot={sim}
          simOverrides={simInject}
          collapsed={{ right: collapsed.rightCollapsed }}
          timePoints={enrichedTimelines[assetId] ?? []}
          nowMs={nowMs}
          extended={expanded}
          pinned
          zones={zones}
          onTogglePin={handleTogglePin}
          onToggleCollapse={handleToggleCollapse}
          onOverrideChange={handleOverrideChange}
          onResetSoc={handleResetSoc}
        />
      );
    }
    return null;
  });

  return (
    <Box data-testid="controller-page">
      <GridSignalStrip />
      <Box sx={{ display: "flex", alignItems: "center", mb: 1, pr: 0.5 }}>
        <Typography variant="h6">Controller</Typography>
        <Box sx={{ flex: 1 }} />
        <Tooltip title={expanded ? "Collapse to ±1h view" : "Expand to 48h planning horizon"}>
          <IconButton data-testid="global-time-range-extend-btn" size="small" onClick={handleToggleExpand} sx={{ m: 0.5 }}>
            {expanded ? <ZoomInMapIcon fontSize="small" /> : <ZoomOutMapIcon fontSize="small" />}
          </IconButton>
        </Tooltip>
      </Box>

      {/* Pinned zone */}
      <PinnedZone pinnedCellIds={pinnedCellIds}>{pinnedElements}</PinnedZone>

      {/* Scrollable content */}
      <Box data-testid="scrollable-content">
        <FlexibilityForecastPanel assetIds={assetSummaries.map((s) => s.assetId)} />

        {/* Grid-level cells */}
        {tariffSnapshot && !pinnedCellIds.includes("grid:tariff") && (
          <GridTariffCell
            snapshot={tariffSnapshot}
            gridTimeline={allTimelines["grid"] ?? []}
            nowMs={nowMs}
            extended={expanded}
            pinned={false}
            zones={zones}
            onTogglePin={() => handleTogglePin("grid:tariff")}
          />
        )}

        {!pinnedCellIds.includes("grid:accumulated") && (
          <GridAccumulatedCell
            assetSummaries={assetSummaries}
            allTimelines={allTimelines}
            nowMs={nowMs}
            extended={expanded}
            pinned={false}
            gridPowerKw={sim.grid.net_power_w / 1000}
            zones={zones}
            onTogglePin={() => handleTogglePin("grid:accumulated")}
          />
        )}

        {/* Asset cells */}
        {assetSummaries
          .filter((s) => !pinnedCellIds.includes(`asset:${s.assetId}`))
          .map((summary) => {
            const cellId = `asset:${summary.assetId}`;
            const collapsed = collapseState[cellId] ?? { rightCollapsed: true };
            return (
              <AssetCell
                key={summary.assetId}
                assetId={summary.assetId}
                summary={summary}
                simSnapshot={sim}
                simOverrides={simInject}
                collapsed={{ right: collapsed.rightCollapsed }}
                timePoints={enrichedTimelines[summary.assetId] ?? []}
                nowMs={nowMs}
                extended={expanded}
                pinned={false}
                zones={zones}
                onTogglePin={handleTogglePin}
                onToggleCollapse={handleToggleCollapse}
                onOverrideChange={handleOverrideChange}
                onResetSoc={handleResetSoc}
              />
            );
          })}
      </Box>
    </Box>
  );
}
