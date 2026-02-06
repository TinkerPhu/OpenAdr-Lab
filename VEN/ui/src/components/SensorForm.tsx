import { useState } from "react";
import { Button, Paper, Stack, TextField, Typography } from "@mui/material";
import { usePostSensor } from "../api/hooks";

export function SensorForm() {
  const [temp, setTemp] = useState("");
  const [power, setPower] = useState("");
  const [voltage, setVoltage] = useState("");
  const [raw, setRaw] = useState("");
  const mutation = usePostSensor();

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const payload: Record<string, unknown> = {};
    if (temp) payload.temperature_c = parseFloat(temp);
    if (power) payload.power_w = parseFloat(power);
    if (voltage) payload.voltage_v = parseFloat(voltage);
    if (raw) {
      try {
        payload.raw = JSON.parse(raw);
      } catch {
        payload.raw = raw;
      }
    }
    mutation.mutate(payload);
  }

  return (
    <Paper sx={{ p: 2 }}>
      <Typography variant="h6" gutterBottom>
        Inject Sensor Reading
      </Typography>
      <form onSubmit={handleSubmit}>
        <Stack spacing={2}>
          <TextField
            label="Temperature (C)"
            size="small"
            type="number"
            value={temp}
            onChange={(e) => setTemp(e.target.value)}
            inputProps={{
              "data-testid": "sensor-form-temp",
              "aria-label": "Temperature",
              step: "0.1",
            }}
          />
          <TextField
            label="Power (W)"
            size="small"
            type="number"
            value={power}
            onChange={(e) => setPower(e.target.value)}
            inputProps={{
              "data-testid": "sensor-form-power",
              "aria-label": "Power",
              step: "0.1",
            }}
          />
          <TextField
            label="Voltage (V)"
            size="small"
            type="number"
            value={voltage}
            onChange={(e) => setVoltage(e.target.value)}
            inputProps={{
              "data-testid": "sensor-form-voltage",
              "aria-label": "Voltage",
              step: "0.1",
            }}
          />
          <TextField
            label="Raw JSON"
            size="small"
            multiline
            rows={3}
            value={raw}
            onChange={(e) => setRaw(e.target.value)}
            inputProps={{
              "data-testid": "sensor-form-raw",
              "aria-label": "Raw JSON",
            }}
          />
          <Button
            type="submit"
            variant="contained"
            disabled={mutation.isPending}
            data-testid="sensor-form-submit"
            aria-label="Submit sensor reading"
          >
            {mutation.isPending ? "Submitting..." : "Submit"}
          </Button>
          {mutation.isSuccess && (
            <Typography
              color="success.main"
              data-testid="sensor-form-success"
              role="status"
            >
              Sensor reading submitted successfully
            </Typography>
          )}
          {mutation.isError && (
            <Typography color="error.main" role="alert">
              Error: {(mutation.error as Error).message}
            </Typography>
          )}
        </Stack>
      </form>
    </Paper>
  );
}
