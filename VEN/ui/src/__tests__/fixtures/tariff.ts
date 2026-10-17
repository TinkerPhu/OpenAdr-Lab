// `TariffSnapshot` / `CapacitySnapshot` fixtures (R-133). The wire carries each interval's end;
// a test that only cares about the start gets an end one hour later.
import type { CapacitySnapshot, TariffSnapshot } from "../../api/types";

const HOUR_MS = 3_600_000;
const hourAfter = (iso: string) => new Date(new Date(iso).getTime() + HOUR_MS).toISOString();

type WithStart<T> = Partial<T> & { interval_start: string };

export function makeTariff(fields: WithStart<TariffSnapshot>): TariffSnapshot {
  return {
    interval_end: hourAfter(fields.interval_start),
    import_tariff_eur_kwh: null,
    export_tariff_eur_kwh: null,
    co2_g_kwh: null,
    ...fields,
  };
}

export function makeCapacitySnapshot(fields: WithStart<CapacitySnapshot>): CapacitySnapshot {
  return {
    interval_end: hourAfter(fields.interval_start),
    import_limit_kw: null,
    export_limit_kw: null,
    import_limit_event_id: null,
    export_limit_event_id: null,
    import_subscription_kw: null,
    export_subscription_kw: null,
    import_reservation_kw: null,
    export_reservation_kw: null,
    ...fields,
  };
}
