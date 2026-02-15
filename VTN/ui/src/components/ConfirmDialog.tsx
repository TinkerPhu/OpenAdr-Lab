import {
  Alert, Button, Dialog, DialogActions, DialogContent, DialogContentText, DialogTitle,
} from "@mui/material";

type ConfirmDialogProps = {
  open: boolean;
  title: string;
  message: string;
  onConfirm: () => void;
  onCancel: () => void;
  loading?: boolean;
  error?: string | null;
};

export function ConfirmDialog(props: ConfirmDialogProps) {
  const { open, title, message, onConfirm, onCancel, loading = false, error } = props;

  return (
    <Dialog open={open} onClose={onCancel} data-testid="confirm-dialog">
      <DialogTitle>{title}</DialogTitle>
      <DialogContent>
        <DialogContentText>{message}</DialogContentText>
        {error && (
          <Alert severity="error" sx={{ mt: 2 }} data-testid="confirm-dialog-error">
            {error}
          </Alert>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onCancel} data-testid="confirm-dialog-cancel">
          {error ? "Close" : "Cancel"}
        </Button>
        {!error && (
          <Button
            onClick={onConfirm}
            color="error"
            variant="contained"
            disabled={loading}
            data-testid="confirm-dialog-ok"
          >
            {loading ? "Deleting..." : "Delete"}
          </Button>
        )}
      </DialogActions>
    </Dialog>
  );
}
