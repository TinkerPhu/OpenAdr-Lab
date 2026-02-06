import { useMemo, useState } from "react";
import { BrowserRouter, Link, Route, Routes } from "react-router-dom";
import {
  AppBar, Box, Button, Container, FormControl, InputLabel, MenuItem, Select,
  Stack, Toolbar, Typography
} from "@mui/material";
import { VenApi } from "./api/client";
import { Event, Program, SensorSnapshot } from "./api/types";
import { usePoll } from "./hooks/usePoll";
import { DashboardPage } from "./pages/Dashboard";
import { ProgramsPage } from "./pages/Programs";
import { EventsPage } from "./pages/Events";
import { SensorsPage } from "./pages/Sensors";

const DEFAULT_VENS = [
  { label: "VEN1", url: "http://raspberrypi.local:8081" },
  { label: "VEN2", url: "http://raspberrypi.local:8082" },
  { label: "VEN3", url: "http://raspberrypi.local:8083" },
];

export default function App() {
  const [venUrl, setVenUrl] = useState(DEFAULT_VENS[0].url);
  const api = useMemo(() => new VenApi(venUrl), [venUrl]);

  const [health, setHealth] = useState<"ok" | "offline" | "unknown">("unknown");
  const [programs, setPrograms] = useState<Program[]>([]);
  const [events, setEvents] = useState<Event[]>([]);
  const [sensor, setSensor] = useState<SensorSnapshot | null>(null);

  const [auto, setAuto] = useState(true);
  const [eventsUpdated, setEventsUpdated] = useState<Date | null>(null);
  const [programsUpdated, setProgramsUpdated] = useState<Date | null>(null);
  const [sensorsUpdated, setSensorsUpdated] = useState<Date | null>(null);

  async function refreshHealth() {
    try {
      await api.health();
      setHealth("ok");
    } catch {
      setHealth("offline");
    }
  }

  async function refreshPrograms() {
    try {
      const p = await api.programs();
      setPrograms(p);
      setProgramsUpdated(new Date());
    } catch {}
  }

  async function refreshEvents() {
    try {
      const e = await api.events(200);
      setEvents(e);
      setEventsUpdated(new Date());
    } catch {}
  }

  async function refreshSensors() {
    try {
      const s = await api.sensors();
      setSensor(s);
      setSensorsUpdated(new Date());
    } catch {}
  }

  // Pollers (config matches VEN defaults)
  usePoll(refreshHealth, 10_000, auto);
  usePoll(refreshEvents, 30_000, auto);
  usePoll(refreshPrograms, 300_000, auto);
  usePoll(refreshSensors, 10_000, auto);

  async function refreshAll() {
    await Promise.all([refreshHealth(), refreshPrograms(), refreshEvents(), refreshSensors()]);
  }

  return (
    <BrowserRouter>
      <AppBar position="sticky">
        <Toolbar>
          <Typography variant="h6" sx={{ mr: 2 }}>VEN Dashboard</Typography>

          <FormControl size="small" sx={{ minWidth: 220, bgcolor: "rgba(255,255,255,0.1)", borderRadius: 1 }}>
            <InputLabel sx={{ color: "white" }}>VEN</InputLabel>
            <Select
              value={venUrl}
              label="VEN"
              onChange={(e) => setVenUrl(e.target.value)}
              sx={{ color: "white" }}
            >
              {DEFAULT_VENS.map(v => (
                <MenuItem key={v.url} value={v.url}>{v.label} — {v.url}</MenuItem>
              ))}
            </Select>
          </FormControl>

          <Box sx={{ flex: 1 }} />

          <Button color="inherit" onClick={() => setAuto(a => !a)}>
            Auto: {auto ? "On" : "Off"}
          </Button>
          <Button color="inherit" onClick={refreshAll}>Refresh</Button>
        </Toolbar>
      </AppBar>

      <Container sx={{ py: 3 }}>
        <Stack direction="row" spacing={2} sx={{ mb: 2 }}>
          <Button component={Link} to="/">Dashboard</Button>
          <Button component={Link} to="/programs">Programs</Button>
          <Button component={Link} to="/events">Events</Button>
          <Button component={Link} to="/sensors">Sensors</Button>
        </Stack>

        <Routes>
          <Route
            path="/"
            element={<DashboardPage programs={programs} events={events} sensor={sensor} health={health} />}
          />
          <Route path="/programs" element={<ProgramsPage programs={programs} lastUpdated={programsUpdated} />} />
          <Route path="/events" element={<EventsPage events={events} lastUpdated={eventsUpdated} />} />
          <Route path="/sensors" element={<SensorsPage sensor={sensor} lastUpdated={sensorsUpdated} />} />
        </Routes>
      </Container>
    </BrowserRouter>
  );
}
