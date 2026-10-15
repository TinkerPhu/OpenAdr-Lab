import {
  Alert,
  Box,
  CircularProgress,
  Grid,
  Typography,
} from "@mui/material";
import {
  useRequests,
  usePostRequest,
  useDeleteRequest,
  useArbiterSettings,
  usePutArbiterSettings,
  useArbiterDiagnostics,
  useSim,
} from "../api/hooks";
import { assetsOfType } from "../api/assets";
import { EvDevice, HeaterDevice } from "../components/devices/AssetDeviceCards";
import { ShiftableLoadsCard } from "../components/devices/ShiftableLoadsCard";
import { ComfortCurveCard } from "../components/devices/ComfortCurveCard";
import { ArbiterSettingsCard } from "../components/devices/ArbiterSettingsCard";
import { BaselineOverrideCard } from "../components/devices/BaselineOverrideCard";
import { AllRequestsSection } from "../components/devices/AllRequestsSection";
import { AssetSpecsTable } from "../components/devices/AssetSpecsTable";

export function DevicesPage() {
  const { data: allRequests = [], isLoading, isError, error } = useRequests();
  const { data: sim } = useSim();
  const { data: arbiterSettings } = useArbiterSettings();
  const { data: arbiterDiagnostics } = useArbiterDiagnostics(
    (arbiterSettings?.deviation_arbiter_enabled ?? false) ||
      (arbiterSettings?.limit_enforcement_enabled ?? false),
  );
  const postMut = usePostRequest();
  const deleteMut = useDeleteRequest();
  const putArbiterMut = usePutArbiterSettings();
  // What every request-making card gets; each keeps its own asset's requests (W6).
  const requestProps = {
    requests: allRequests,
    postRequest: postMut.mutateAsync,
    deleteRequest: deleteMut.mutateAsync,
    isPosting: postMut.isPending,
    isDeleting: deleteMut.isPending,
  };
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
        {/* One card per EV and per heater, in roster order, each titled with its asset id. */}
        {assetsOfType(sim, "ev").map(([assetId]) => (
          <Grid item xs={12} md={4} key={assetId}>
            <EvDevice assetId={assetId} {...requestProps} />
          </Grid>
        ))}
        {assetsOfType(sim, "heater").map(([assetId]) => (
          <Grid item xs={12} md={4} key={assetId}>
            <HeaterDevice assetId={assetId} {...requestProps} />
          </Grid>
        ))}
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
          <ComfortCurveCard sim={sim} />
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
