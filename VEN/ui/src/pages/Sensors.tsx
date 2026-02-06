import { useState } from "react";
import { Link as MuiLink, Paper, Stack, Typography } from "@mui/material";
import { useSensor } from "../api/hooks";
import { JsonDialog } from "../components/JsonDialog";
import { SensorForm } from "../components/SensorForm";

export function SensorsPage() {
  const { data: sensor, dataUpdatedAt } = useSensor();
  const [openRaw, setOpenRaw] = useState(false);

  const lastUpdated = dataUpdatedAt ? new Date(dataUpdatedAt).toLocaleString() : "—";

  return (
    <Stack spacing={2}>
      <div>
        <Typography
          variant="h5"
          data-testid="sensors-heading"
        >
          Sensors
        </Typography>
        <Typography
          variant="body2"
          color="text.secondary"
          data-testid="sensors-last-updated"
        >
          Last updated: {lastUpdated}
        </Typography>
      </div>

      <Paper sx={{ p: 2 }}>
        <Stack spacing={1}>
          <Typography data-testid="sensor-timestamp">
            Timestamp: {sensor?.ts ?? "—"}
          </Typography>
          <Typography data-testid="sensor-power">
            Power (W): {sensor?.power_w ?? "—"}
          </Typography>
          <Typography data-testid="sensor-temp">
            Temp (C): {sensor?.temperature_c ?? "—"}
          </Typography>
          <Typography data-testid="sensor-voltage">
            Voltage (V): {sensor?.voltage_v ?? "—"}
          </Typography>
          <MuiLink
            component="button"
            onClick={() => setOpenRaw(true)}
            data-testid="sensor-raw-link"
            aria-label="View raw JSON"
            sx={{ width: "fit-content", cursor: "pointer" }}
          >
            View raw JSON
          </MuiLink>
        </Stack>
      </Paper>

      <SensorForm />

      <JsonDialog
        open={openRaw}
        title="Sensor raw payload"
        json={sensor?.raw ?? {}}
        onClose={() => setOpenRaw(false)}
      />
    </Stack>
  );
}
