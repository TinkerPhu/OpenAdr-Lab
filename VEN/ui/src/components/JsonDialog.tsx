import { Dialog, DialogContent, DialogTitle, IconButton } from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";

export function JsonDialog(props: {
  open: boolean;
  title: string;
  json: any;
  onClose: () => void;
}) {
  return (
    <Dialog open={props.open} onClose={props.onClose} fullWidth maxWidth="md">
      <DialogTitle sx={{ display: "flex", alignItems: "center", gap: 1 }}>
        {props.title}
        <span style={{ flex: 1 }} />
        <IconButton onClick={props.onClose} size="small">
          <CloseIcon />
        </IconButton>
      </DialogTitle>
      <DialogContent>
        <pre style={{ margin: 0, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
          {JSON.stringify(props.json, null, 2)}
        </pre>
      </DialogContent>
    </Dialog>
  );
}
