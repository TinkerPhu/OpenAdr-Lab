import { useEffect, useState } from "react";
import {
  Button, Dialog, DialogActions, DialogContent, DialogTitle, TextField,
} from "@mui/material";
import type { Program, ProgramInput } from "../api/types";

type ProgramFormDialogProps = {
  open: boolean;
  program: Program | null; // null = create, non-null = edit
  onSubmit: (input: ProgramInput) => void;
  onCancel: () => void;
  loading?: boolean;
};

export function ProgramFormDialog(props: ProgramFormDialogProps) {
  const { open, program, onSubmit, onCancel, loading = false } = props;
  const [name, setName] = useState("");

  useEffect(() => {
    if (open) setName(program?.programName ?? "");
  }, [open, program]);

  const isEdit = program !== null;

  return (
    <Dialog open={open} onClose={onCancel} fullWidth maxWidth="sm" data-testid="program-form-dialog">
      <DialogTitle>{isEdit ? "Edit Program" : "Create Program"}</DialogTitle>
      <DialogContent>
        <TextField
          autoFocus
          margin="dense"
          label="Program Name"
          fullWidth
          value={name}
          onChange={(e) => setName(e.target.value)}
          inputProps={{ "data-testid": "program-name-input" }}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onCancel}>Cancel</Button>
        <Button
          onClick={() => onSubmit({ programName: name })}
          variant="contained"
          disabled={!name.trim() || loading}
          data-testid="program-form-submit"
        >
          {loading ? "Saving..." : isEdit ? "Save" : "Create"}
        </Button>
      </DialogActions>
    </Dialog>
  );
}
