import {
  Accordion,
  AccordionDetails,
  AccordionSummary,
  IconButton,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import CloseIcon from "@mui/icons-material/Close";
import type { UserRequestWithSession } from "../../api/types";

// ── Helpers ──────────────────────────────────────────────────────────────────

function fmtDate(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function sessionSummary(req: UserRequestWithSession): string {
  const s = req.session;
  if (!s) return req.asset_id;
  if (s.type === "ev") return `${(s.target_soc * 100).toFixed(0)}% SoC · depart ${fmtDate(s.departure_time)}`;
  if (s.type === "heater") return `${s.target_temp_c}°C · ready ${fmtDate(s.ready_by)}`;
  if (s.type === "shiftable_load") return `${s.power_kw}kW ${s.duration_min}min · by ${fmtDate(s.latest_end)}`;
  return req.asset_id;
}

function deviceIcon(req: UserRequestWithSession): string {
  if (req.session_type === "ev") return "⚡";
  if (req.session_type === "heater") return "🔥";
  return "⏱";
}

// ── Props ────────────────────────────────────────────────────────────────────

export type AllRequestsSectionProps = {
  requests: UserRequestWithSession[];
  deleteRequest: (id: string) => Promise<unknown>;
  isDeleting: boolean;
};

// ── Component ────────────────────────────────────────────────────────────────

export function AllRequestsSection(props: AllRequestsSectionProps) {
  const { requests, deleteRequest, isDeleting } = props;

  // Sort: ACTIVE first, then by created_at descending
  const sorted = [...requests].sort((a, b) => {
    if (a.status === "ACTIVE" && b.status !== "ACTIVE") return -1;
    if (a.status !== "ACTIVE" && b.status === "ACTIVE") return 1;
    return new Date(b.created_at).getTime() - new Date(a.created_at).getTime();
  });

  return (
    <Accordion defaultExpanded={false} data-testid="all-requests-accordion">
      <AccordionSummary expandIcon={<ExpandMoreIcon />}>
        <Typography>All Requests ({requests.length} total)</Typography>
      </AccordionSummary>
      <AccordionDetails>
        <TableContainer>
          <Table size="small" data-testid="all-requests-table">
            <TableHead>
              <TableRow>
                <TableCell>Device</TableCell>
                <TableCell>Summary</TableCell>
                <TableCell>Mode</TableCell>
                <TableCell>Status</TableCell>
                <TableCell>Est. Cost €</TableCell>
                <TableCell>Created</TableCell>
                <TableCell />
              </TableRow>
            </TableHead>
            <TableBody>
              {sorted.map((req) => (
                <TableRow key={req.id} data-testid={`all-requests-row-${req.id}`}>
                  <TableCell>{deviceIcon(req)} {req.asset_id}</TableCell>
                  <TableCell>{sessionSummary(req)}</TableCell>
                  <TableCell data-testid={`request-mode-${req.id}`}>{req.mode}</TableCell>
                  <TableCell>{req.status}</TableCell>
                  <TableCell>{req.estimated_cost_eur.toFixed(2)}</TableCell>
                  <TableCell>{fmtDate(req.created_at)}</TableCell>
                  <TableCell>
                    <IconButton
                      size="small"
                      disabled={req.status !== "ACTIVE" || isDeleting}
                      data-testid={`all-requests-cancel-${req.id}`}
                      onClick={() => deleteRequest(req.id)}
                    >
                      <CloseIcon fontSize="small" />
                    </IconButton>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableContainer>
      </AccordionDetails>
    </Accordion>
  );
}
