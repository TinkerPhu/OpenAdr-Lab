import { useEffect, useState } from "react";
import {
  Button, Dialog, DialogActions, DialogContent, DialogTitle,
  MenuItem, TextField,
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
  const [intervals, setIntervals] = useState("[]");

  useEffect(() => {
    if (open) {
      setEventName(event?.eventName ?? "");
      setProgramID(event?.programID ?? (programs[0]?.id ?? ""));
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
          onClick={() => onSubmit({ eventName, programID, intervals: JSON.parse(intervals) })}
          variant="contained"
          disabled={!eventName.trim() || !programID || !intervalsValid || loading}
          data-testid="event-form-submit"
        >
          {loading ? "Saving..." : isEdit ? "Save" : "Create"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
