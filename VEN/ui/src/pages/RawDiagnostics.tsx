import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Typography } from "@mui/material";
import { useVenContext } from "../App";
import { DiagnosticCell } from "../components/raw-diagnostics/DiagnosticCell";
import { SimProfileChart } from "../components/raw-diagnostics/SimProfileChart";
import { TariffsLineChart } from "../components/raw-diagnostics/TariffsLineChart";
import { TimelineSeriesChart } from "../components/raw-diagnostics/TimelineSeriesChart";
import type { SimSnapshot, PlannedRates } from "../api/types";
import type { AssetTimelinePoint } from "../components/controller-v2/types";

export function RawDiagnosticsPage() {
  const { api } = useVenContext();

  // ── Simulator State ──────────────────────────────────────────────────────────
  const simQuery = useQuery<SimSnapshot>({
    queryKey: ["raw-diag-sim", api.baseUrl],
    queryFn: () => api.sim(),
    enabled: false,
  });

  // ── Tariffs ──────────────────────────────────────────────────────────────────
  const tariffsQuery = useQuery<PlannedRates>({
    queryKey: ["raw-diag-tariffs", api.baseUrl],
    queryFn: () => api.rates(),
    enabled: false,
  });

  // ── Timeline ─────────────────────────────────────────────────────────────────
  const [selectedSeries, setSelectedSeries] = useState("grid");
  const timelineQuery = useQuery<Record<string, AssetTimelinePoint[]>>({
    queryKey: ["raw-diag-timeline", api.baseUrl],
    queryFn: () => api.allTimelines({ hoursBack: 1.0, hoursForward: 1.0 }),
    enabled: false,
  });

  return (
    <div data-testid="raw-diagnostics-page">
      <Typography variant="h5" sx={{ mb: 2 }}>
        Raw Data Diagnostics
      </Typography>

      <DiagnosticCell
        title="Simulator State"
        isLoading={simQuery.isFetching}
        isError={simQuery.isError}
        onRefresh={() => simQuery.refetch()}
      >
        {simQuery.data ? (
          <SimProfileChart data={simQuery.data} />
        ) : (
          <Typography variant="body2" color="text.secondary">
            Click refresh to load simulator state.
          </Typography>
        )}
      </DiagnosticCell>

      <DiagnosticCell
        title="Tariffs"
        isLoading={tariffsQuery.isFetching}
        isError={tariffsQuery.isError}
        onRefresh={() => tariffsQuery.refetch()}
      >
        {tariffsQuery.data ? (
          <TariffsLineChart data={tariffsQuery.data} />
        ) : (
          <Typography variant="body2" color="text.secondary">
            Click refresh to load tariff data.
          </Typography>
        )}
      </DiagnosticCell>

      <DiagnosticCell
        title="Timeline"
        isLoading={timelineQuery.isFetching}
        isError={timelineQuery.isError}
        onRefresh={() => timelineQuery.refetch()}
      >
        {timelineQuery.data ? (
          <TimelineSeriesChart
            data={timelineQuery.data}
            selectedSeries={selectedSeries}
            onSeriesChange={setSelectedSeries}
          />
        ) : (
          <Typography variant="body2" color="text.secondary">
            Select a series and click refresh to load timeline data.
          </Typography>
        )}
      </DiagnosticCell>
    </div>
  );
}
