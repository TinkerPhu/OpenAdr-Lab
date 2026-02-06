import { Paper, Stack, Typography } from "@mui/material";
import { SensorSnapshot } from "../api/types";
import { JsonDialog } from "../components/JsonDialog";
import { useState } from "react";

export function SensorsPage(props: { sensor: SensorSnapshot | null; lastUpdated?: Date | null }) {
  const [openRaw, setOpenRaw] = useState(false);
  const s = props.sensor;

  return (
    <Stack spacing={2}>
      <div>
        <Typography variant="h5">Sensors</Typography>
        <Typography variant="body2" color="text.secondary">
          Last updated: {props.lastUpdated ? props.lastUpdated.toLocaleString() : "—"}
        </Typography>
      </div>

      <Paper sx={{ p: 2 }}>
        <Stack spacing={1}>
          <Typography>Timestamp: {s?.ts ?? "—"}</Typography>
          <Typography>Power (W): {s?.power_w ?? "—"}</Typography>
          <Typography>Temp (°C): {s?.temperature_c ?? "—"}</Typography>
          <Typography>Voltage (V): {s?.voltage_v ?? "—"}</Typography>
          <Typography
            sx={{ cursor: "pointer", color: "primary.main", width: "fit-content" }}
            onClick={() => setOpenRaw(true)}
          >
            View raw JSON
          </Typography>
        </Stack>
      </Paper>

      <JsonDialog
        open={openRaw}
        title="Sensor raw payload"
        json={s?.raw ?? {}}
        onClose={() => setOpenRaw(false)}
      />
    </Stack>
  );
}
