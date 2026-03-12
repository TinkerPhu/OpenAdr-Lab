import { createContext, useContext, useMemo, useState } from "react";
import { BrowserRouter, Link, Route, Routes } from "react-router-dom";
import {
  AppBar, Box, Button, Chip, Container, FormControl, InputLabel,
  MenuItem, Select, Stack, Toolbar, Typography,
} from "@mui/material";
import { useQueryClient } from "@tanstack/react-query";
import { VenApi } from "./api/client";
import { useHealth } from "./api/hooks";
import { DashboardPage } from "./pages/Dashboard";
import { ProgramsPage } from "./pages/Programs";
import { EventsPage } from "./pages/Events";
import { SensorsPage } from "./pages/Sensors";
import { ReportsPage } from "./pages/Reports";
import { MetricsPage } from "./pages/Metrics";
import { TracePage } from "./pages/Trace";
import { SimulationPage } from "./pages/Simulation";
import { ControllerPage } from "./pages/Controller";
import { UserRequestsPage } from "./pages/UserRequests";

const DEFAULT_VENS = [
  { label: "VEN1", url: import.meta.env.VITE_VEN_1_URL || "http://pi4server.local:8211", venName: "ven-1" },
  { label: "VEN2", url: import.meta.env.VITE_VEN_2_URL || "http://pi4server.local:8212", venName: "ven-2" },
  { label: "VEN3", url: import.meta.env.VITE_VEN_3_URL || "http://pi4server.local:8213", venName: "ven-3" },
];

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
  const status = isError ? "offline" : data ? "ok" : "unknown";
  const color = status === "ok" ? "success" : status === "offline" ? "error" : "default";

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
  const api = useMemo(() => { console.log("[VEN-UI] VenApi created for", venUrl); return new VenApi(venUrl); }, [venUrl]);
  const queryClient = useQueryClient();

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

  const venName = DEFAULT_VENS.find((v) => v.url === venUrl)?.venName ?? "ven-1";
  const ctx = useMemo(() => ({ venUrl, venName, setVenUrl, api }), [venUrl, venName, api]);

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
                value={venUrl}
                label="VEN"
                onChange={(e) => handleVenChange(e.target.value)}
                sx={{ color: "white" }}
                data-testid="ven-selector"
                aria-label="Select VEN"
              >
                {DEFAULT_VENS.map((v) => (
                  <MenuItem key={v.url} value={v.url}>
                    {v.label} — {v.url}
                  </MenuItem>
                ))}
              </Select>
            </FormControl>

            <Box sx={{ mx: 1 }}>
              <HealthChip />
            </Box>

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
              to="/simulation"
              data-testid="nav-simulation"
            >
              Simulation
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
              to="/user-requests"
              data-testid="nav-user-requests"
            >
              User Requests
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
              to="/sensors"
              data-testid="nav-sensors"
            >
              Sensors
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
              to="/trace"
              data-testid="nav-trace"
            >
              Trace
            </Button>
            <Button
              component={Link}
              to="/metrics"
              data-testid="nav-metrics"
            >
              Metrics
            </Button>
          </Stack>

          <Routes>
            <Route path="/" element={<DashboardPage />} />
            <Route path="/simulation" element={<SimulationPage />} />
            <Route path="/controller" element={<ControllerPage />} />
            <Route path="/user-requests" element={<UserRequestsPage />} />
            <Route path="/programs" element={<ProgramsPage />} />
            <Route path="/events" element={<EventsPage />} />
            <Route path="/sensors" element={<SensorsPage />} />
            <Route path="/reports" element={<ReportsPage />} />
            <Route path="/trace" element={<TracePage />} />
            <Route path="/metrics" element={<MetricsPage />} />
          </Routes>
        </Container>
      </BrowserRouter>
    </VenContext.Provider>
  );
}
