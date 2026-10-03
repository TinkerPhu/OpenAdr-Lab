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
  useEvUsageSim,
  usePostRequest,
  useDeleteRequest,
  usePutEvSettings,
  useArbiterSettings,
  usePutArbiterSettings,
  useArbiterDiagnostics,
  useSim,
} from "../api/hooks";
import { EvCard } from "../components/devices/EvCard";
import { HeaterCard } from "../components/devices/HeaterCard";
import { ShiftableLoadsCard } from "../components/devices/ShiftableLoadsCard";
import { ComfortCurveCard } from "../components/devices/ComfortCurveCard";
import { ArbiterSettingsCard } from "../components/devices/ArbiterSettingsCard";
import { BaselineOverrideCard } from "../components/devices/BaselineOverrideCard";
import { AllRequestsSection } from "../components/devices/AllRequestsSection";
import { AssetSpecsTable } from "../components/devices/AssetSpecsTable";

export function DevicesPage() {
  const { data: allRequests = [], isLoading, isError, error } = useRequests();
  const { data: sim } = useSim();
  const { data: evSettings } = useEvSettings();
  const { data: evUsageSim } = useEvUsageSim();
  const { data: arbiterSettings } = useArbiterSettings();
  const { data: arbiterDiagnostics } = useArbiterDiagnostics(
    (arbiterSettings?.deviation_arbiter_enabled ?? false) ||
      (arbiterSettings?.limit_enforcement_enabled ?? false),
  );
  const postMut = usePostRequest();
  const deleteMut = useDeleteRequest();
  const putEvMut = usePutEvSettings();
  const putArbiterMut = usePutArbiterSettings();

  // Every active EV request, in window order - an EV may hold several queued
  // sessions, and `.find()` showed only whichever came first while the rest were
  // invisible on screen despite being planned for (`ui-transparency`).
  const evRequests = allRequests
    .filter((r) => r.session_type === "ev" && r.status === "ACTIVE")
    .sort((a, b) => {
      const aw = a.session?.type === "ev" ? a.session.window_start : "";
      const bw = b.session?.type === "ev" ? b.session.window_start : "";
      return aw.localeCompare(bw);
    });
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
      <Box sx={{ mb: 3 }}>
        <AssetSpecsTable sim={sim} />
      </Box>
      <Grid container spacing={2} sx={{ mb: 3 }}>
        <Grid item xs={12} md={4}>
          <EvCard
            requests={evRequests}
            evSettings={evSettings}
            usageSim={evUsageSim}
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
