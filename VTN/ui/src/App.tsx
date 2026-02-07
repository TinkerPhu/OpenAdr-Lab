import { createContext, useContext, useMemo, useState } from "react";
import { BrowserRouter, Link, Route, Routes } from "react-router-dom";
import {
  AppBar, Box, Button, Chip, Container, Stack, Toolbar, Typography,
} from "@mui/material";
import { useQueryClient } from "@tanstack/react-query";
import { BffApi } from "./api/client";
import { useHealth } from "./api/hooks";
import { DashboardPage } from "./pages/Dashboard";
import { ProgramsPage } from "./pages/Programs";
import { EventsPage } from "./pages/Events";
import { VensPage } from "./pages/Vens";

type BffContextType = {
  api: BffApi;
};

const BffContext = createContext<BffContextType | null>(null);

export function useBffContext(): BffContextType {
  const ctx = useContext(BffContext);
  if (!ctx) throw new Error("useBffContext must be used within BffProvider");
  return ctx;
}

function HealthChip() {
  const { data, isError } = useHealth();
  const vtnOk = data?.vtn?.reachable && data?.vtn?.authOk;
  const status = isError ? "offline" : vtnOk ? "ok" : data ? "degraded" : "unknown";
  const color = status === "ok" ? "success" : status === "offline" ? "error" : status === "degraded" ? "warning" : "default";

  return (
    <Chip
      label={`VTN: ${status}`}
      color={color}
      size="small"
      data-testid="health-status"
      role="status"
      aria-label={`Health status: ${status}`}
    />
  );
}

export default function App() {
  const [autoRefresh, setAutoRefresh] = useState(true);
  const api = useMemo(() => new BffApi(), []);
  const queryClient = useQueryClient();

  function handleRefreshAll() {
    queryClient.invalidateQueries();
  }

  function handleToggleAuto() {
    setAutoRefresh((a) => !a);
    if (autoRefresh) {
      queryClient.setDefaultOptions({
        queries: { refetchInterval: false },
      });
    } else {
      queryClient.setDefaultOptions({
        queries: { refetchInterval: undefined },
      });
    }
    queryClient.invalidateQueries();
  }

  const ctx = useMemo(() => ({ api }), [api]);

  return (
    <BffContext.Provider value={ctx}>
      <BrowserRouter>
        <AppBar position="sticky">
          <Toolbar>
            <Typography variant="h6" sx={{ mr: 2 }}>
              VTN Dashboard
            </Typography>

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
            <Button component={Link} to="/" data-testid="nav-dashboard">
              Dashboard
            </Button>
            <Button component={Link} to="/programs" data-testid="nav-programs">
              Programs
            </Button>
            <Button component={Link} to="/events" data-testid="nav-events">
              Events
            </Button>
            <Button component={Link} to="/vens" data-testid="nav-vens">
              VENs
            </Button>
          </Stack>

          <Routes>
            <Route path="/" element={<DashboardPage />} />
            <Route path="/programs" element={<ProgramsPage />} />
            <Route path="/events" element={<EventsPage />} />
            <Route path="/vens" element={<VensPage />} />
          </Routes>
        </Container>
      </BrowserRouter>
    </BffContext.Provider>
  );
}
