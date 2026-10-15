import {
  useAssetCapabilities,
  useEvSettings,
  useEvUsageSim,
  usePutEvSettings,
} from "../../api/hooks";
import type { CreateUserRequestBody, UserRequestWithSession } from "../../api/types";
import { EvCard } from "./EvCard";
import { HeaterCard } from "./HeaterCard";

/** What every request-making card needs from the page: the requests and the two mutations. */
type RequestProps = {
  /** Every request of the VEN; each card keeps its own asset's. */
  requests: UserRequestWithSession[];
  postRequest: (body: CreateUserRequestBody) => Promise<unknown>;
  deleteRequest: (id: string) => Promise<unknown>;
  isPosting: boolean;
  isDeleting: boolean;
};

/** One EV's card with its own settings, usage schedule, declared target and active requests in
 * window order (W6: one per EV, the hooks per asset id). */
export function EvDevice({ assetId, requests, ...rest }: RequestProps & { assetId: string }) {
  const { data: evSettings } = useEvSettings(assetId);
  const { data: usageSim } = useEvUsageSim(assetId);
  const putEvMut = usePutEvSettings(assetId);
  const [capability] = useAssetCapabilities([assetId]);
  // Every active request of this EV, in window order — an EV may hold several queued
  // sessions, and `.find()` showed only whichever came first (`ui-transparency`).
  const own = requests
    .filter((r) => r.asset_id === assetId && r.session_type === "ev" && r.status === "ACTIVE")
    .sort((a, b) => {
      const aw = a.session?.type === "ev" ? a.session.window_start : "";
      const bw = b.session?.type === "ev" ? b.session.window_start : "";
      return aw.localeCompare(bw);
    });
  return (
    <EvCard
      assetId={assetId}
      requests={own}
      declaredTargetSocFrac={capability?.data?.default_target_soc_frac}
      evSettings={evSettings}
      usageSim={usageSim}
      putEvSettings={putEvMut.mutate}
      {...rest}
    />
  );
}

/** One heater's card with its own declared target and active request. */
export function HeaterDevice({ assetId, requests, ...rest }: RequestProps & { assetId: string }) {
  const [capability] = useAssetCapabilities([assetId]);
  const request = requests.find(
    (r) => r.asset_id === assetId && r.session_type === "heater" && r.status === "ACTIVE",
  );
  return (
    <HeaterCard
      assetId={assetId}
      request={request}
      declaredTargetTempC={capability?.data?.default_target_temp_c}
      {...rest}
    />
  );
}
