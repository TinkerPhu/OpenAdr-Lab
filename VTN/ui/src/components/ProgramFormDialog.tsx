import { useEffect, useState } from "react";
import {
  Button, Checkbox, Dialog, DialogActions, DialogContent, DialogTitle,
  FormControlLabel, FormGroup, TextField, Typography,
} from "@mui/material";
import type { Program, ProgramInput, Ven } from "../api/types";
import { targetsOf } from "../api/targets";

type ProgramFormDialogProps = {
  open: boolean;
  program: Program | null; // null = create, non-null = edit
  vens: Ven[];
  onSubmit: (input: ProgramInput) => void;
  onCancel: () => void;
  loading?: boolean;
};

function getEnrolledVenNames(program: Program | null): string[] {
  return program ? targetsOf(program) : [];
}

export function ProgramFormDialog(props: ProgramFormDialogProps) {
  const { open, program, vens, onSubmit, onCancel, loading = false } = props;
  const [name, setName] = useState("");
  const [descriptionUrl, setDescriptionUrl] = useState("");
  const [selectedVens, setSelectedVens] = useState<string[]>([]);

  useEffect(() => {
    if (open) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- form reset on dialog open; no loop risk
      setName(program?.programName ?? "");
      setDescriptionUrl(program?.programDescriptions?.[0]?.URL ?? "");
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
    if (descriptionUrl.trim()) {
      input.programDescriptions = [{ URL: descriptionUrl.trim() }];
    } else {
      input.programDescriptions = null;
    }
    // 3.1 targets are a flat list of strings and are non-optional: an empty
    // list is how a program says "open to every VEN", where 3.0 sent null.
    input.targets = selectedVens;
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
          label="Description URL"
          fullWidth
          type="url"
          value={descriptionUrl}
          onChange={(e) => setDescriptionUrl(e.target.value)}
          inputProps={{ "data-testid": "program-description-url-input" }}
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
