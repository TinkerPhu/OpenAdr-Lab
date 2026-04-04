import { useRef, useEffect, useState } from "react";
import {
  Box, Chip, Popover, Stack, Typography,
} from "@mui/material";
import type { TraceEntry } from "../../api/types";

// ─── Chip appearance per event type ──────────────────────────────────────────

type ChipProps = { label: string; color: "default" | "primary" | "secondary" | "info" | "success" | "warning" | "error" };

function chipFor(event: TraceEntry): ChipProps {
  switch (event.type) {
    case "PlanCycle": {
      const colorMap: Record<string, ChipProps["color"]> = {
        Periodic: "default",
        RateChange: "primary",
        CapacityChange: "warning",
        UserRequest: "secondary",
        Event: "success",
      };
      return {
        label: event.trigger_reason,
        color: colorMap[event.trigger_reason] ?? "default",
      };
    }
    case "RateChange":
      return { label: `${event.import_eur_kwh.toFixed(3)} €`, color: "info" };
    case "CapacityChange":
      return {
        label: event.import_limit_kw != null ? `${event.import_limit_kw}kW` : "Cap",
        color: "warning",
      };
    case "OpenAdrArrived":
      return { label: event.event_name.slice(0, 12), color: "success" };
    case "OpenAdrExpired":
      return { label: `${event.event_name.slice(0, 10)} ✗`, color: "default" };
    case "PacketTransition":
      return { label: `${event.asset_id}: ${event.from_status}→${event.to_status}`, color: "primary" };
    case "RequestTransition":
      return { label: `req: ${event.from_status}→${event.to_status}`, color: "secondary" };
  }
}

// ─── Popover detail renderer ──────────────────────────────────────────────────

function EventDetail({ event }: { event: TraceEntry }) {
  const ts = new Date(event.ts).toLocaleString();
  switch (event.type) {
    case "PlanCycle":
      return (
        <>
          <Typography variant="caption" fontWeight="bold">PlanCycle</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">trigger: {event.trigger_reason}</Typography>
          <Typography variant="caption" display="block">firm slots: {event.firm_slots}</Typography>
          <Typography variant="caption" display="block">flex slots: {event.flexible_slots}</Typography>
        </>
      );
    case "RateChange":
      return (
        <>
          <Typography variant="caption" fontWeight="bold">RateChange</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">import: {event.import_eur_kwh.toFixed(4)} €/kWh</Typography>
          <Typography variant="caption" display="block">export: {event.export_eur_kwh.toFixed(4)} €/kWh</Typography>
        </>
      );
    case "CapacityChange":
      return (
        <>
          <Typography variant="caption" fontWeight="bold">CapacityChange</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">import limit: {event.import_limit_kw ?? "—"} kW</Typography>
          <Typography variant="caption" display="block">export limit: {event.export_limit_kw ?? "—"} kW</Typography>
        </>
      );
    case "OpenAdrArrived":
      return (
        <>
          <Typography variant="caption" fontWeight="bold">OpenAdrArrived</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">event: {event.event_name}</Typography>
          <Typography variant="caption" display="block">signal: {event.signal_type} @ {event.value}</Typography>
        </>
      );
    case "OpenAdrExpired":
      return (
        <>
          <Typography variant="caption" fontWeight="bold">OpenAdrExpired</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">event: {event.event_name}</Typography>
        </>
      );
    case "PacketTransition":
      return (
        <>
          <Typography variant="caption" fontWeight="bold">PacketTransition</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">asset: {event.asset_id}</Typography>
          <Typography variant="caption" display="block">{event.from_status} → {event.to_status}</Typography>
          <Typography variant="caption" display="block">packet: …{event.packet_id.slice(-6)}</Typography>
        </>
      );
    case "RequestTransition":
      return (
        <>
          <Typography variant="caption" fontWeight="bold">RequestTransition</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">asset: {event.asset_id}</Typography>
          <Typography variant="caption" display="block">{event.from_status} → {event.to_status}</Typography>
          <Typography variant="caption" display="block">request: …{event.request_id.slice(-6)}</Typography>
        </>
      );
  }
}

// ─── Main component ───────────────────────────────────────────────────────────

type Props = { events: TraceEntry[] };

export function PlanTriggerTimeline({ events }: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [anchorEl, setAnchorEl] = useState<HTMLElement | null>(null);
  const [selectedEvent, setSelectedEvent] = useState<TraceEntry | null>(null);

  // Input is newest-first; render oldest-left by reversing
  const ordered = [...events].reverse();

  // Auto-scroll to right end (newest event) on mount / when events change
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollLeft = scrollRef.current.scrollWidth;
    }
  }, [events]);

  const handleChipClick = (event: TraceEntry, el: HTMLElement) => {
    setSelectedEvent(event);
    setAnchorEl(el);
  };

  const handleClose = () => {
    setAnchorEl(null);
    setSelectedEvent(null);
  };

  return (
    <Box
      data-testid="trigger-timeline"
      ref={scrollRef}
      sx={{ overflowX: "auto", display: "flex", alignItems: "center", gap: 0.75, p: 1, minHeight: 40 }}
    >
      {ordered.length === 0 ? (
        <Typography variant="caption" color="text.secondary">
          No controller events recorded yet.
        </Typography>
      ) : (
        ordered.map((ev, i) => {
          const { label, color } = chipFor(ev);
          return (
            <Chip
              key={i}
              data-testid={`trigger-chip-${i}`}
              label={label}
              color={color}
              size="small"
              onClick={(e) => handleChipClick(ev, e.currentTarget)}
              title={`${ev.type} — ${new Date(ev.ts).toLocaleTimeString()}`}
              sx={{ cursor: "pointer", flexShrink: 0 }}
            />
          );
        })
      )}

      <Popover
        open={Boolean(anchorEl)}
        anchorEl={anchorEl}
        onClose={handleClose}
        anchorOrigin={{ vertical: "bottom", horizontal: "left" }}
      >
        {selectedEvent && (
          <Stack
            data-testid="trigger-popover"
            spacing={0.25}
            sx={{ p: 1.5, minWidth: 160, maxWidth: 280 }}
          >
            <EventDetail event={selectedEvent} />
            <Chip
              data-testid="trigger-popover-close"
              label="Close"
              size="small"
              onClick={handleClose}
              sx={{ mt: 0.5, alignSelf: "flex-start" }}
            />
          </Stack>
        )}
      </Popover>
    </Box>
  );
}
