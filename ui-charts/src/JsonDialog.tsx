import { Dialog, DialogContent, DialogTitle, IconButton } from "@mui/material";
import CloseIcon from "@mui/icons-material/Close";

type JsonDialogProps = {
  open: boolean;
  title: string;
  json: unknown;
  onClose: () => void;
};

export function JsonDialog(props: JsonDialogProps) {
  const { open, title, json, onClose } = props;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      fullWidth
      maxWidth="md"
      data-testid="json-dialog"
      aria-modal="true"
      aria-labelledby="json-dialog-title"
    >
      <DialogTitle
        id="json-dialog-title"
        data-testid="json-dialog-title"
        sx={{ display: "flex", alignItems: "center", gap: 1 }}
      >
        {title}
        <span style={{ flex: 1 }} />
        <IconButton
          onClick={onClose}
          size="small"
          data-testid="json-dialog-close"
          aria-label="Close dialog"
        >
          <CloseIcon />
        </IconButton>
      </DialogTitle>
      <DialogContent>
        <pre
          data-testid="json-dialog-content"
          style={{ margin: 0, whiteSpace: "pre-wrap", wordBreak: "break-word" }}
        >
          {JSON.stringify(json, null, 2)}
        </pre>
      </DialogContent>
    </Dialog>
  );
}
