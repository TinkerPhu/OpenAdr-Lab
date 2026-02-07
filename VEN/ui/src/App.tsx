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

const DEFAULT_VENS = [
  { label: "VEN1", url: "http://raspberrypi.local:8211" },
  { label: "VEN2", url: "http://raspberrypi.local:8212" },
  { label: "VEN3", url: "http://raspberrypi.local:8213" },
];

type VenContextType = {
  venUrl: string;
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
  const { data, isError } = useHealth();
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

export default function App() {
  const [venUrl, setVenUrl] = useState(DEFAULT_VENS[0].url);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const api = useMemo(() => new VenApi(venUrl), [venUrl]);
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

  const ctx = useMemo(() => ({ venUrl, setVenUrl, api }), [venUrl, api]);

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
          </Stack>

          <Routes>
            <Route path="/" element={<DashboardPage />} />
            <Route path="/programs" element={<ProgramsPage />} />
            <Route path="/events" element={<EventsPage />} />
            <Route path="/sensors" element={<SensorsPage />} />
          </Routes>
        </Container>
      </BrowserRouter>
    </VenContext.Provider>
  );
}
