import {
  Alert,
  Box,
  CircularProgress,
  Grid,
  Typography,
} from "@mui/material";
import {
  useRequests,
  useEvSettings,
  usePostRequest,
  useDeleteRequest,
  usePutEvSettings,
  useArbiterSettings,
  usePutArbiterSettings,
  useArbiterDiagnostics,
} from "../api/hooks";
import { EvCard } from "../components/devices/EvCard";
import { HeaterCard } from "../components/devices/HeaterCard";
import { ShiftableLoadsCard } from "../components/devices/ShiftableLoadsCard";
import { ComfortCurveCard } from "../components/devices/ComfortCurveCard";
import { ArbiterSettingsCard } from "../components/devices/ArbiterSettingsCard";
import { BaselineOverrideCard } from "../components/devices/BaselineOverrideCard";
import { AllRequestsSection } from "../components/devices/AllRequestsSection";

export function DevicesPage() {
  const { data: allRequests = [], isLoading, isError, error } = useRequests();
  const { data: evSettings } = useEvSettings();
  const { data: arbiterSettings } = useArbiterSettings();
  const { data: arbiterDiagnostics } = useArbiterDiagnostics(
    arbiterSettings?.deviation_arbiter_enabled ?? false,
  );
  const postMut = usePostRequest();
  const deleteMut = useDeleteRequest();
  const putEvMut = usePutEvSettings();
  const putArbiterMut = usePutArbiterSettings();

  const evRequest = allRequests.find(
    (r) => r.session_type === "ev" && r.status === "ACTIVE",
  );
  const heaterRequest = allRequests.find(
    (r) => r.session_type === "heater" && r.status === "ACTIVE",
  );
  const shiftableActive = allRequests.filter(
    (r) => r.session_type === "shiftable_load" && r.status === "ACTIVE",
  );

  return (
    <Box data-testid="devices-page">
      <Typography variant="h5" gutterBottom>
        Devices
      </Typography>
      {isLoading && <CircularProgress />}
      {isError && <Alert severity="error">{String(error)}</Alert>}
      <Grid container spacing={2} sx={{ mb: 3 }}>
        <Grid item xs={12} md={4}>
          <EvCard
            request={evRequest}
            evSettings={evSettings}
            postRequest={postMut.mutateAsync}
            deleteRequest={deleteMut.mutateAsync}
            putEvSettings={putEvMut.mutate}
            isPosting={postMut.isPending}
            isDeleting={deleteMut.isPending}
          />
        </Grid>
        <Grid item xs={12} md={4}>
          <HeaterCard
            request={heaterRequest}
            postRequest={postMut.mutateAsync}
            deleteRequest={deleteMut.mutateAsync}
            isPosting={postMut.isPending}
            isDeleting={deleteMut.isPending}
          />
        </Grid>
        <Grid item xs={12} md={4}>
          <ShiftableLoadsCard
            loads={shiftableActive}
            postRequest={postMut.mutateAsync}
            deleteRequest={deleteMut.mutateAsync}
            isPosting={postMut.isPending}
            isDeleting={deleteMut.isPending}
          />
        </Grid>
        <Grid item xs={12} md={4}>
          <ComfortCurveCard />
        </Grid>
        <Grid item xs={12} md={4}>
          <ArbiterSettingsCard
            arbiterSettings={arbiterSettings}
            putArbiterSettings={putArbiterMut.mutate}
            diagnostics={arbiterDiagnostics}
          />
        </Grid>
        <Grid item xs={12} md={4}>
          <BaselineOverrideCard />
        </Grid>
      </Grid>
      <AllRequestsSection
        requests={allRequests}
        deleteRequest={deleteMut.mutateAsync}
        isDeleting={deleteMut.isPending}
      />
    </Box>
  );
}
