import { useEffect, useState } from "react";
import {
  Button, Checkbox, Dialog, DialogActions, DialogContent, DialogTitle,
  FormControlLabel, FormGroup, TextField, Typography,
} from "@mui/material";
import type { Program, ProgramInput, Ven } from "../api/types";

type ProgramFormDialogProps = {
  open: boolean;
  program: Program | null; // null = create, non-null = edit
  vens: Ven[];
  onSubmit: (input: ProgramInput) => void;
  onCancel: () => void;
  loading?: boolean;
};

function getEnrolledVenNames(program: Program | null): string[] {
  if (!program?.targets) return [];
  const entry = program.targets.find((t) => t.type === "VEN_NAME");
  return entry?.values ?? [];
}

export function ProgramFormDialog(props: ProgramFormDialogProps) {
  const { open, program, vens, onSubmit, onCancel, loading = false } = props;
  const [name, setName] = useState("");
  const [longName, setLongName] = useState("");
  const [programType, setProgramType] = useState("");
  const [selectedVens, setSelectedVens] = useState<string[]>([]);

  useEffect(() => {
    if (open) {
      setName(program?.programName ?? "");
      setLongName(program?.programLongName ?? "");
      setProgramType(program?.programType ?? "");
      setSelectedVens(getEnrolledVenNames(program));
    }
  }, [open, program]);

  const isEdit = program !== null;

  function handleVenToggle(venName: string) {
    setSelectedVens((prev) =>
      prev.includes(venName) ? prev.filter((v) => v !== venName) : [...prev, venName],
    );
  }

  function handleSubmit() {
    const input: ProgramInput = { programName: name };
    if (longName.trim()) input.programLongName = longName.trim();
    if (programType.trim()) input.programType = programType.trim();
    input.targets =
      selectedVens.length > 0
        ? [{ type: "VEN_NAME", values: selectedVens }]
        : null;
    onSubmit(input);
  }

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
        <TextField
          margin="dense"
          label="Long Name"
          fullWidth
          value={longName}
          onChange={(e) => setLongName(e.target.value)}
          inputProps={{ "data-testid": "program-long-name-input" }}
        />
        <TextField
          margin="dense"
          label="Program Type"
          fullWidth
          value={programType}
          onChange={(e) => setProgramType(e.target.value)}
          inputProps={{ "data-testid": "program-type-input" }}
        />
        {vens.length > 0 && (
          <>
            <Typography variant="subtitle2" sx={{ mt: 2, mb: 0.5 }}>
              Enrolled VENs
            </Typography>
            <Typography variant="caption" color="text.secondary">
              No selection = open program (visible to all VENs)
            </Typography>
            <FormGroup data-testid="program-ven-checkboxes">
              {vens.map((v) => (
                <FormControlLabel
                  key={v.id}
                  control={
                    <Checkbox
                      checked={selectedVens.includes(v.venName ?? v.id)}
                      onChange={() => handleVenToggle(v.venName ?? v.id)}
                      data-testid={`ven-checkbox-${v.venName ?? v.id}`}
                    />
                  }
                  label={v.venName ?? v.id}
                />
              ))}
            </FormGroup>
          </>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onCancel}>Cancel</Button>
        <Button
          onClick={handleSubmit}
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
