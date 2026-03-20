import { useState, useMemo, useEffect } from "react";
import { Alert, Box, CircularProgress, Typography } from "@mui/material";
import { useSim, useTariffs, useRequests, useSimOverride, useSetSimOverride, useAllTimelines } from "../api/hooks";
import type { AssetId, CollapseState } from "../components/controller-v2/types";
import { deriveAssetSummaries, deriveTariffSnapshot } from "../components/controller-v2/dataBuilders";
import { AssetCell } from "../components/controller-v2/AssetCell";
import { PinnedZone } from "../components/controller-v2/PinnedZone";
import { GridTariffCell } from "../components/controller-v2/GridTariffCell";
import { GridAccumulatedCell } from "../components/controller-v2/GridAccumulatedCell";
import type { UserOverrides } from "../api/types";

export function ControllerV2Page() {
  const { data: sim, isLoading: simLoading, isError: simError, refetch: refetchSim } = useSim({ refetchInterval: false });
  const { data: rates, refetch: refetchTariffs } = useTariffs({ refetchInterval: false });
  const { data: userRequests, refetch: refetchRequests } = useRequests({ refetchInterval: false });
  const { data: simOverrides } = useSimOverride();
  const { mutate: setOverride } = useSetSimOverride();

  const [pinnedCellIds, setPinnedCellIds] = useState<string[]>([]);
  const [collapseState, setCollapseState] = useState<CollapseState>({});
  const [expandedCells, setExpandedCells] = useState<Set<string>>(new Set());

  // Widen the timeline window whenever any cell is expanded.
  const hoursForward = expandedCells.size > 0 ? 24.0 : 1.0;

  // Single timeline query shared by all asset cells and the accumulated cell.
  // refetchInterval: false — driven exclusively by the unified timer below so
  // all data sources update in the same tick.
  const { data: allTimelines = {}, refetch: refetchTimelines } = useAllTimelines(
    1.0,
    hoursForward,
    { refetchInterval: false }
  );

  // Shared nowMs: advances only when fresh timeline data arrives, keeping the
  // NOW reference line consistent across all charts in the same render.
  const nowMs = useMemo(() => Date.now(), [allTimelines]);

  // Single poll timer — all sources refresh in one tick.
  useEffect(() => {
    const id = setInterval(() => {
      refetchSim();
      refetchTariffs();
      refetchRequests();
      refetchTimelines();
    }, 10_000);
    return () => clearInterval(id);
  }, [refetchSim, refetchTariffs, refetchRequests, refetchTimelines]);

  function handleTogglePin(cellId: string) {
    setPinnedCellIds((prev) =>
      prev.includes(cellId) ? prev.filter((id) => id !== cellId) : [...prev, cellId]
    );
  }

  function handleToggleExpand(cellId: string) {
    setExpandedCells((prev) => {
      const next = new Set(prev);
      if (next.has(cellId)) next.delete(cellId);
      else next.add(cellId);
      return next;
    });
  }

  function handleToggleCollapse(cellId: string, section: "left" | "right") {
    setCollapseState((prev) => {
      const current = prev[cellId] ?? { leftCollapsed: false, rightCollapsed: true };
      return {
        ...prev,
        [cellId]: {
          ...current,
          leftCollapsed: section === "left" ? !current.leftCollapsed : current.leftCollapsed,
          rightCollapsed: section === "right" ? !current.rightCollapsed : current.rightCollapsed,
        },
      };
    });
  }

  function handleOverrideChange(patch: Partial<UserOverrides>) {
    setOverride({ ...simOverrides, ...patch } as UserOverrides);
  }

  const tariffs = rates ?? [];
  const requests = userRequests ?? [];

  const assetSummaries = useMemo(() => {
    if (!sim) return [];
    return deriveAssetSummaries(sim, tariffs, requests, allTimelines, Date.now());
  }, [sim, tariffs, requests, allTimelines]);

  const tariffSnapshot = useMemo(() => {
    if (!sim) return null;
    return deriveTariffSnapshot(sim, tariffs, Date.now());
  }, [sim, tariffs]);

  if (simLoading) {
    return (
      <Box data-testid="controller-v2-page" sx={{ p: 4, textAlign: "center" }}>
        <CircularProgress />
      </Box>
    );
  }

  if (simError || !sim) {
    return (
      <Box data-testid="controller-v2-page" sx={{ p: 4 }}>
        <Alert severity="error">
          Unable to load simulation data. Check that the VEN is reachable.
        </Alert>
      </Box>
    );
  }

  // Build pinned cells React elements
  const pinnedElements = pinnedCellIds.map((cellId) => {
    if (cellId.startsWith("asset:")) {
      const assetId = cellId.replace("asset:", "") as AssetId;
      const summary = assetSummaries.find((s) => s.assetId === assetId);
      if (!summary) return null;
      const collapsed = collapseState[cellId] ?? { leftCollapsed: false, rightCollapsed: true };
      return (
        <AssetCell
          key={cellId}
          assetId={assetId}
          summary={summary}
          simSnapshot={sim}
          simOverrides={simOverrides}
          collapsed={{ left: collapsed.leftCollapsed, right: collapsed.rightCollapsed }}
          timePoints={allTimelines[assetId] ?? []}
          nowMs={nowMs}
          extended={expandedCells.has(cellId)}
          pinned
          onTogglePin={handleTogglePin}
          onToggleCollapse={handleToggleCollapse}
          onToggleExpand={handleToggleExpand}
          onOverrideChange={handleOverrideChange}
        />
      );
    }
    return null;
  });

  return (
    <Box data-testid="controller-v2-page">
      <Typography variant="h6" sx={{ mb: 1 }}>
        Controller V2
      </Typography>

      {/* Pinned zone */}
      <PinnedZone pinnedCellIds={pinnedCellIds}>{pinnedElements}</PinnedZone>

      {/* Scrollable content */}
      <Box data-testid="scrollable-content">
        {/* Grid-level cells */}
        {tariffSnapshot && !pinnedCellIds.includes("grid:tariff") && (
          <GridTariffCell
            snapshot={tariffSnapshot}
            gridTimeline={allTimelines["grid"] ?? []}
            nowMs={nowMs}
            extended={expandedCells.has("grid:tariff")}
            pinned={false}
            onTogglePin={() => handleTogglePin("grid:tariff")}
            onToggleExpand={() => handleToggleExpand("grid:tariff")}
          />
        )}

        {!pinnedCellIds.includes("grid:accumulated") && (
          <GridAccumulatedCell
            assetSummaries={assetSummaries}
            allTimelines={allTimelines}
            nowMs={nowMs}
            extended={expandedCells.has("grid:accumulated")}
            pinned={false}
            gridPowerKw={tariffSnapshot?.gridPowerKw ?? 0}
            onTogglePin={() => handleTogglePin("grid:accumulated")}
            onToggleExpand={() => handleToggleExpand("grid:accumulated")}
          />
        )}

        {/* Asset cells */}
        {assetSummaries
          .filter((s) => !pinnedCellIds.includes(`asset:${s.assetId}`))
          .map((summary) => {
            const cellId = `asset:${summary.assetId}`;
            const collapsed = collapseState[cellId] ?? {
              leftCollapsed: false,
              rightCollapsed: true,
            };
            return (
              <AssetCell
                key={summary.assetId}
                assetId={summary.assetId}
                summary={summary}
                simSnapshot={sim}
                simOverrides={simOverrides}
                collapsed={{ left: collapsed.leftCollapsed, right: collapsed.rightCollapsed }}
                timePoints={allTimelines[summary.assetId] ?? []}
                nowMs={nowMs}
                extended={expandedCells.has(cellId)}
                pinned={false}
                onTogglePin={handleTogglePin}
                onToggleCollapse={handleToggleCollapse}
                onToggleExpand={handleToggleExpand}
                onOverrideChange={handleOverrideChange}
              />
            );
          })}
      </Box>
    </Box>
  );
}
