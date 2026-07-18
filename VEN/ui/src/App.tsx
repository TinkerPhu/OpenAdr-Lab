import { createContext, useContext, useMemo, useState } from "react";
import { BrowserRouter, Link, Route, Routes } from "react-router-dom";
import {
  AppBar, Box, Button, Chip, Container, FormControl, InputLabel,
  MenuItem, Select, Stack, Toolbar, Typography,
} from "@mui/material";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { VenApi } from "./api/client";
import { useHealth } from "./api/hooks";
import { DEFAULT_VENS, fetchDiscoveredVens, mergeVens } from "./api/venRegistry";
import { NotificationsBell } from "./components/NotificationsBell";
import { DashboardPage } from "./pages/Dashboard";
import { ProgramsPage } from "./pages/Programs";
import { EventsPage } from "./pages/Events";
import { ReportsPage } from "./pages/Reports";
import { MetricsPage } from "./pages/Metrics";
import { ControllerPage } from "./pages/Controller";
import { RawDiagnosticsPage } from "./pages/RawDiagnostics";
import { TasksPage } from "./pages/Tasks";
import { HistoryPage } from "./pages/History";
import { NotificationsPage } from "./pages/Notifications";
import { PlannerPage } from "./pages/Planner";
import { DevicesPage } from "./pages/Devices";

type VenContextType = {
  venUrl: string;
  venName: string;
  setVenUrl: (url: string) => void;
  api: VenApi;
};

const VenContext = createContext<VenContextType | null>(null);

export function useVenContext(): VenContextType {
  const ctx = useContext(VenContext);
  if (!ctx) throw new Error("useVenContext must be used within VenProvider");
  return ctx;
}

function HealthChip() {
  const { data, isError, isLoading, fetchStatus, error } = useHealth();
  console.log("[VEN-UI] HealthChip render:", { isLoading, isError, fetchStatus, data, error: error?.message });
  // WP-T1 (docs/plans/ven-ui-transparency.md): /health now returns
  // {status, components} — read the real status instead of assuming "ok"
  // whenever a response merely arrived (that was the misleading-chip bug).
  const status = isError ? "offline" : data ? data.status : "unknown";
  const color =
    status === "ok" ? "success" : status === "degraded" ? "warning" : status === "offline" ? "error" : "default";

  return (
    <Chip
      label={status}
      color={color}
      size="small"
      data-testid="health-status"
      role="status"
      aria-label={`Health status: ${status}`}
    />
  );
}

console.log("[VEN-UI] Module loaded at", new Date().toISOString());

export default function App() {
  console.log("[VEN-UI] App render");
  const [venUrl, setVenUrl] = useState(DEFAULT_VENS[0].url);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const queryClient = useQueryClient();

  // Dynamic dropdown: registered + reachable VENs beyond the default trio
  // (fleet instances). On registry error the dropdown just stays defaults.
  const { data: discovered } = useQuery({
    queryKey: ["vens-registry"],
    queryFn: () => fetchDiscoveredVens(),
    refetchInterval: 30_000,
    retry: false,
  });
  const vens = useMemo(() => mergeVens(DEFAULT_VENS, discovered ?? []), [discovered]);

  // If the selected VEN vanished from the list (fleet purged between
  // refreshes), fall back to the first default rather than rendering a
  // Select value with no matching MenuItem.
  const safeVenUrl = vens.some((v) => v.url === venUrl) ? venUrl : DEFAULT_VENS[0].url;
  const api = useMemo(() => { console.log("[VEN-UI] VenApi created for", safeVenUrl); return new VenApi(safeVenUrl); }, [safeVenUrl]);

  function handleVenChange(url: string) {
    setVenUrl(url);
    queryClient.invalidateQueries();
  }

  function handleRefreshAll() {
    queryClient.invalidateQueries();
  }

  function handleToggleAuto() {
    setAutoRefresh((a) => !a);
    if (autoRefresh) {
      // Turning off — set all refetch intervals to false
      queryClient.setDefaultOptions({
        queries: { refetchInterval: false },
      });
    } else {
      // Turning on — clear defaults so per-query intervals resume
      queryClient.setDefaultOptions({
        queries: { refetchInterval: undefined },
      });
    }
    queryClient.invalidateQueries();
  }

  const venName = vens.find((v) => v.url === safeVenUrl)?.venName ?? "ven-1";
  const ctx = useMemo(
    () => ({ venUrl: safeVenUrl, venName, setVenUrl, api }),
    [safeVenUrl, venName, api],
  );

  return (
    <VenContext.Provider value={ctx}>
      <BrowserRouter>
        <AppBar position="sticky">
          <Toolbar>
            <Typography variant="h6" sx={{ mr: 2 }}>
              VEN Dashboard
            </Typography>

            <FormControl
              size="small"
              sx={{ minWidth: 220, bgcolor: "rgba(255,255,255,0.1)", borderRadius: 1 }}
            >
              <InputLabel sx={{ color: "white" }}>VEN</InputLabel>
              <Select
                value={safeVenUrl}
                label="VEN"
                onChange={(e) => handleVenChange(e.target.value)}
                sx={{ color: "white" }}
                data-testid="ven-selector"
                aria-label="Select VEN"
              >
                {vens.map((v) => (
                  <MenuItem key={v.url} value={v.url}>
                    {v.label} — {v.url}
                  </MenuItem>
                ))}
              </Select>
            </FormControl>

            <Box sx={{ mx: 1 }}>
              <HealthChip />
            </Box>

            <NotificationsBell />

            <Box sx={{ flex: 1 }} />

            <Button
              color="inherit"
              onClick={handleToggleAuto}
              data-testid="auto-refresh-toggle"
              aria-label={`Auto refresh: ${autoRefresh ? "On" : "Off"}`}
              aria-pressed={autoRefresh}
            >
              Auto: {autoRefresh ? "On" : "Off"}
            </Button>
            <Button
              color="inherit"
              onClick={handleRefreshAll}
              data-testid="refresh-all-btn"
              aria-label="Refresh all data"
            >
              Refresh
            </Button>
          </Toolbar>
        </AppBar>

        <Container sx={{ py: 3 }}>
          <Stack
            component="nav"
            direction="row"
            spacing={2}
            sx={{ mb: 2 }}
            aria-label="Main navigation"
          >
            <Button
              component={Link}
              to="/"
              data-testid="nav-dashboard"
            >
              Dashboard
            </Button>
            <Button
              component={Link}
              to="/history"
              data-testid="nav-history"
            >
              History
            </Button>
            <Button
              component={Link}
              to="/controller"
              data-testid="nav-controller"
            >
              Controller
            </Button>
            <Button
              component={Link}
              to="/planner"
              data-testid="nav-planner"
            >
              Planner
            </Button>
            <Button
              component={Link}
              to="/devices"
              data-testid="nav-devices"
            >
              Devices
            </Button>
            <Button
              component={Link}
              to="/programs"
              data-testid="nav-programs"
            >
              Programs
            </Button>
            <Button
              component={Link}
              to="/events"
              data-testid="nav-events"
            >
              Events
            </Button>
            <Button
              component={Link}
              to="/reports"
              data-testid="nav-reports"
            >
              Reports
            </Button>
            <Button
              component={Link}
              to="/metrics"
              data-testid="nav-metrics"
            >
              Metrics
            </Button>
            <Button
              component={Link}
              to="/raw-diagnostics"
              data-testid="nav-raw-diagnostics"
            >
              Raw Data
            </Button>
            <Button
              component={Link}
              to="/tasks"
              data-testid="nav-tasks"
            >
              Tasks
            </Button>
            <Button
              component={Link}
              to="/notifications"
              data-testid="nav-notifications"
            >
              Notifications
            </Button>
          </Stack>

          <Routes>
            <Route path="/" element={<DashboardPage />} />
            <Route path="/planner" element={<PlannerPage />} />
            <Route path="/controller" element={<ControllerPage />} />
            <Route path="/devices" element={<DevicesPage />} />
            <Route path="/programs" element={<ProgramsPage />} />
            <Route path="/events" element={<EventsPage />} />
            <Route path="/reports" element={<ReportsPage />} />
            <Route path="/metrics" element={<MetricsPage />} />
            <Route path="/raw-diagnostics" element={<RawDiagnosticsPage />} />
            <Route path="/tasks" element={<TasksPage />} />
            <Route path="/history" element={<HistoryPage />} />
            <Route path="/notifications" element={<NotificationsPage />} />
          </Routes>
        </Container>
      </BrowserRouter>
    </VenContext.Provider>
  );
}
