import { useEffect, useState } from "react";
import {
  Button, Dialog, DialogActions, DialogContent, DialogTitle,
  MenuItem, Stack, TextField,
} from "@mui/material";
import type { EventInput, Program, VtnEvent } from "../api/types";

type EventFormDialogProps = {
  open: boolean;
  event: VtnEvent | null; // null = create, non-null = edit
  programs: Program[];
  onSubmit: (input: EventInput) => void;
  onCancel: () => void;
  loading?: boolean;
};

export function EventFormDialog(props: EventFormDialogProps) {
  const { open, event, programs, onSubmit, onCancel, loading = false } = props;
  const [eventName, setEventName] = useState("");
  const [programID, setProgramID] = useState("");
  const [priority, setPriority] = useState("");
  const [startTime, setStartTime] = useState("");
  const [duration, setDuration] = useState("");
  const [targets, setTargets] = useState("");
  const [intervals, setIntervals] = useState("[]");

  useEffect(() => {
    if (open) {
      setEventName(event?.eventName ?? "");
      setProgramID(event?.programID ?? (programs[0]?.id ?? ""));
      setPriority(event?.priority != null ? String(event.priority) : "");
      setStartTime(event?.intervalPeriod?.start ?? "");
      setDuration(event?.intervalPeriod?.duration ?? "");
      setTargets(event?.targets ? JSON.stringify(event.targets, null, 2) : "");
      setIntervals(event?.intervals ? JSON.stringify(event.intervals, null, 2) : "[]");
    }
  }, [open, event, programs]);

  const isEdit = event !== null;

  let intervalsValid = true;
  try {
    JSON.parse(intervals);
  } catch {
    intervalsValid = false;
  }

  let targetsValid = true;
  if (targets.trim()) {
    try {
      JSON.parse(targets);
    } catch {
      targetsValid = false;
    }
  }

  const priorityNum = priority.trim() === "" ? null : Number(priority);
  const priorityValid = priority.trim() === "" || (!isNaN(priorityNum!) && Number.isInteger(priorityNum));

  function handleSubmit() {
    const input: EventInput = {
      eventName,
      programID,
      intervals: JSON.parse(intervals),
    };
    if (priorityNum != null) {
      input.priority = priorityNum;
    }
    if (startTime.trim()) {
      input.intervalPeriod = {
        start: startTime.trim(),
        duration: duration.trim() || null,
      };
    }
    if (targets.trim()) {
      input.targets = JSON.parse(targets);
    }
    onSubmit(input);
  }

  return (
    <Dialog open={open} onClose={onCancel} fullWidth maxWidth="sm" data-testid="event-form-dialog">
      <DialogTitle>{isEdit ? "Edit Event" : "Create Event"}</DialogTitle>
      <DialogContent>
        <TextField
          autoFocus
          margin="dense"
          label="Event Name"
          fullWidth
          value={eventName}
          onChange={(e) => setEventName(e.target.value)}
          inputProps={{ "data-testid": "event-name-input" }}
        />
        <TextField
          select
          margin="dense"
          label="Program"
          fullWidth
          value={programID}
          onChange={(e) => setProgramID(e.target.value)}
          inputProps={{ "data-testid": "event-program-select" }}
        >
          {programs.map((p) => (
            <MenuItem key={p.id} value={p.id}>
              {p.programName ?? p.id}
            </MenuItem>
          ))}
        </TextField>
        <TextField
          margin="dense"
          label="Priority"
          fullWidth
          type="number"
          value={priority}
          onChange={(e) => setPriority(e.target.value)}
          error={!priorityValid}
          helperText={priorityValid ? "0 = highest. Leave empty for no priority." : "Must be an integer"}
          inputProps={{ "data-testid": "event-priority-input", min: 0 }}
        />
        <Stack direction="row" spacing={1} sx={{ mt: 1 }}>
          <TextField
            margin="dense"
            label="Start Time (ISO 8601)"
            fullWidth
            value={startTime}
            onChange={(e) => setStartTime(e.target.value)}
            placeholder="2026-02-09T14:00:00Z"
            inputProps={{ "data-testid": "event-start-input" }}
          />
          <TextField
            margin="dense"
            label="Duration (ISO 8601)"
            fullWidth
            value={duration}
            onChange={(e) => setDuration(e.target.value)}
            placeholder="PT4H"
            inputProps={{ "data-testid": "event-duration-input" }}
          />
        </Stack>
        <TextField
          margin="dense"
          label="Targets (JSON)"
          fullWidth
          multiline
          minRows={2}
          value={targets}
          onChange={(e) => setTargets(e.target.value)}
          error={!targetsValid}
          helperText={targetsValid ? 'e.g. [{"type":"VEN_NAME","values":["ven-1"]}]' : "Invalid JSON"}
          inputProps={{ "data-testid": "event-targets-input" }}
        />
        <TextField
          margin="dense"
          label="Intervals (JSON)"
          fullWidth
          multiline
          minRows={3}
          value={intervals}
          onChange={(e) => setIntervals(e.target.value)}
          error={!intervalsValid}
          helperText={intervalsValid ? "" : "Invalid JSON"}
          inputProps={{ "data-testid": "event-intervals-input" }}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onCancel}>Cancel</Button>
        <Button
          onClick={handleSubmit}
          variant="contained"
          disabled={!eventName.trim() || !programID || !intervalsValid || !targetsValid || !priorityValid || loading}
          data-testid="event-form-submit"
        >
          {loading ? "Saving..." : isEdit ? "Save" : "Create"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
