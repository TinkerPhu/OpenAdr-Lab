import { TextField } from "@mui/material";
import type { UserRequestMode } from "../../api/types";

const USER_REQUEST_MODES: UserRequestMode[] = [
  "BY_DEADLINE",
  "ASAP",
  "ASAP_FREE",
  "BY_DEADLINE_FREE",
  "MAX_COST",
  "OPPORTUNISTIC",
];

export type ModeSelectProps = {
  value: UserRequestMode;
  onChange: (mode: UserRequestMode) => void;
  testId: string;
};

/** Request-mode picker (BL-28). Native select so tests can use selectOptions. */
export function ModeSelect({ value, onChange, testId }: ModeSelectProps) {
  return (
    <TextField
      select
      label="Mode"
      value={value}
      onChange={(e) => onChange(e.target.value as UserRequestMode)}
      SelectProps={{ native: true }}
      data-testid={testId}
      size="small"
    >
      {USER_REQUEST_MODES.map((m) => (
        <option key={m} value={m}>
          {m}
        </option>
      ))}
    </TextField>
  );
}
