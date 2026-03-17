import { useState, useMemo } from "react";
import { Alert, Box, CircularProgress, Typography } from "@mui/material";
import { useSim, useTariffs, usePlan, useRequests, useSimOverride, useSetSimOverride } from "../api/hooks";
import type { AssetId, CollapseState } from "../components/controller-v2/types";
import { deriveAssetSummaries, deriveTariffSnapshot } from "../components/controller-v2/dataBuilders";
import { AssetCell } from "../components/controller-v2/AssetCell";
import { PinnedZone } from "../components/controller-v2/PinnedZone";
import { GridTariffCell } from "../components/controller-v2/GridTariffCell";
import { GridAccumulatedCell } from "../components/controller-v2/GridAccumulatedCell";
import type { UserOverrides } from "../api/types";

export function ControllerV2Page() {
  const nowMs = useMemo(() => Date.now(), []);

  const { data: sim, isLoading: simLoading, isError: simError } = useSim();
  const { data: rates } = useTariffs();
  const { data: plan } = usePlan();
  const { data: userRequests } = useRequests();
  const { data: simOverrides } = useSimOverride();
  const { mutate: setOverride } = useSetSimOverride();

  const [pinnedCellIds, setPinnedCellIds] = useState<string[]>([]);
  const [collapseState, setCollapseState] = useState<CollapseState>({});

  function handleTogglePin(cellId: string) {
    setPinnedCellIds((prev) =>
      prev.includes(cellId) ? prev.filter((id) => id !== cellId) : [...prev, cellId]
    );
  }

  function handleToggleCollapse(cellId: string, section: "left" | "right") {
    setCollapseState((prev) => {
      const current = prev[cellId] ?? { leftCollapsed: false, rightCollapsed: false };
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
    return deriveAssetSummaries(sim, tariffs, requests, plan ?? null, nowMs);
  }, [sim, tariffs, requests, plan, nowMs]);

  const tariffSnapshot = useMemo(() => {
    if (!sim) return null;
    return deriveTariffSnapshot(sim, tariffs, nowMs);
  }, [sim, tariffs, nowMs]);

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
      const collapsed = collapseState[cellId] ?? { leftCollapsed: false, rightCollapsed: false };
      return (
        <AssetCell
          key={cellId}
          assetId={assetId}
          summary={summary}
          simSnapshot={sim}
          simOverrides={simOverrides}
          collapsed={{ left: collapsed.leftCollapsed, right: collapsed.rightCollapsed }}
          pinned
          onTogglePin={handleTogglePin}
          onToggleCollapse={handleToggleCollapse}
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
            nowMs={nowMs}
            pinned={false}
            onTogglePin={() => handleTogglePin("grid:tariff")}
          />
        )}

        {!pinnedCellIds.includes("grid:accumulated") && (
          <GridAccumulatedCell
            assetSummaries={assetSummaries}
            pinned={false}
            onTogglePin={() => handleTogglePin("grid:accumulated")}
          />
        )}

        {/* Asset cells */}
        {assetSummaries
          .filter((s) => !pinnedCellIds.includes(`asset:${s.assetId}`))
          .map((summary) => {
            const cellId = `asset:${summary.assetId}`;
            const collapsed = collapseState[cellId] ?? {
              leftCollapsed: false,
              rightCollapsed: false,
            };
            return (
              <AssetCell
                key={summary.assetId}
                assetId={summary.assetId}
                summary={summary}
                simSnapshot={sim}
                simOverrides={simOverrides}
                collapsed={{ left: collapsed.leftCollapsed, right: collapsed.rightCollapsed }}
                pinned={false}
                onTogglePin={handleTogglePin}
                onToggleCollapse={handleToggleCollapse}
                onOverrideChange={handleOverrideChange}
              />
            );
          })}
      </Box>
    </Box>
  );
}
