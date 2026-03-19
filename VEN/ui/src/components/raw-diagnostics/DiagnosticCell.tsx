import { Box, Card, CardContent, CircularProgress, IconButton, Typography } from "@mui/material";
import RefreshIcon from "@mui/icons-material/Refresh";

interface DiagnosticCellProps {
  title: string;
  isLoading: boolean;
  isError: boolean;
  onRefresh: () => void;
  children: React.ReactNode;
}

function toKebab(title: string): string {
  return title.toLowerCase().replace(/\s+/g, "-");
}

export function DiagnosticCell({ title, isLoading, isError, onRefresh, children }: DiagnosticCellProps) {
  const slug = toKebab(title);
  return (
    <Card data-testid={`diagnostic-cell-${slug}`} sx={{ mb: 2 }}>
      <CardContent>
        <Box sx={{ display: "flex", alignItems: "center", mb: 1 }}>
          <Typography variant="h6" sx={{ flex: 1 }}>
            {title}
          </Typography>
          <IconButton
            size="small"
            onClick={onRefresh}
            data-testid={`refresh-btn-${slug}`}
            aria-label={`Refresh ${title}`}
          >
            <RefreshIcon />
          </IconButton>
        </Box>

        {isLoading && (
          <Box
            sx={{ display: "flex", justifyContent: "center", py: 4 }}
            data-testid={`loading-indicator-${slug}`}
          >
            <CircularProgress size={32} />
          </Box>
        )}

        {isError && !isLoading && (
          <Typography
            color="error"
            variant="body2"
            data-testid={`error-msg-${slug}`}
          >
            Failed to load data
          </Typography>
        )}

        {!isLoading && !isError && children}
      </CardContent>
    </Card>
  );
}
