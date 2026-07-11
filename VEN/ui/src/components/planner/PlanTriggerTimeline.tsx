import { useRef, useEffect, useState } from "react";
import {
  Badge, Box, Chip, Popover, Stack, Typography,
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
        label: `● ${event.trigger_reason}`,
        color: colorMap[event.trigger_reason] ?? "default",
      };
    }
    case "RateChange":
      return { label: `◆ ${event.import_eur_kwh.toFixed(3)} €`, color: "info" };
    case "CapacityChange":
      return {
        label: `◆ ${event.import_limit_kw != null ? `${event.import_limit_kw}kW` : "Cap"}`,
        color: "warning",
      };
    case "OpenAdrArrived":
      return { label: `★ ${event.event_name.slice(0, 12)}`, color: "success" };
    case "OpenAdrExpired":
      return { label: `☆ ${event.event_name.slice(0, 10)} ✗`, color: "default" };
    case "RequestTransition":
      return { label: `→ req: ${event.from_status}→${event.to_status}`, color: "secondary" };
    case "DispatchOverride":
      return {
        label: event.active ? `⚡ dispatch ${event.setpoint_kw ?? "?"} kW` : "⚡ dispatch cleared",
        color: "error",
      };
  }
}

// ─── Consecutive-group accumulator (like browser console) ────────────────────

type Group = { events: TraceEntry[]; label: string; color: ChipProps["color"] };

function groupConsecutive(ordered: TraceEntry[]): Group[] {
  const groups: Group[] = [];
  for (const ev of ordered) {
    const { label, color } = chipFor(ev);
    const last = groups[groups.length - 1];
    if (last && last.label === label) {
      last.events.push(ev);
    } else {
      groups.push({ events: [ev], label, color });
    }
  }
  return groups;
}

// ─── Popover detail renderer ──────────────────────────────────────────────────

function EventDetail({ group }: { group: Group }) {
  const event = group.events[group.events.length - 1];
  const ts = new Date(event.ts).toLocaleString();
  const countHeader = group.events.length > 1 && (
    <Typography variant="caption" color="text.secondary">
      ×{group.events.length} — {new Date(group.events[0].ts).toLocaleTimeString()} → {new Date(event.ts).toLocaleTimeString()}
    </Typography>
  );
  switch (event.type) {
    case "PlanCycle":
      return (
        <>
          {countHeader}
          <Typography variant="caption" fontWeight="bold">PlanCycle</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">trigger: {event.trigger_reason}</Typography>
          <Typography variant="caption" display="block">slots: {event.total_slots}</Typography>
        </>
      );
    case "RateChange":
      return (
        <>
          {countHeader}
          <Typography variant="caption" fontWeight="bold">RateChange</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">import: {event.import_eur_kwh.toFixed(4)} €/kWh</Typography>
          <Typography variant="caption" display="block">export: {event.export_eur_kwh.toFixed(4)} €/kWh</Typography>
        </>
      );
    case "CapacityChange":
      return (
        <>
          {countHeader}
          <Typography variant="caption" fontWeight="bold">CapacityChange</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">import limit: {event.import_limit_kw ?? "—"} kW</Typography>
          <Typography variant="caption" display="block">export limit: {event.export_limit_kw ?? "—"} kW</Typography>
        </>
      );
    case "OpenAdrArrived":
      return (
        <>
          {countHeader}
          <Typography variant="caption" fontWeight="bold">OpenAdrArrived</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">event: {event.event_name}</Typography>
          <Typography variant="caption" display="block">signal: {event.signal_type} @ {event.value}</Typography>
        </>
      );
    case "OpenAdrExpired":
      return (
        <>
          {countHeader}
          <Typography variant="caption" fontWeight="bold">OpenAdrExpired</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">event: {event.event_name}</Typography>
        </>
      );
    case "RequestTransition":
      return (
        <>
          {countHeader}
          <Typography variant="caption" fontWeight="bold">RequestTransition</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">asset: {event.asset_id}</Typography>
          <Typography variant="caption" display="block">{event.from_status} → {event.to_status}</Typography>
          <Typography variant="caption" display="block">request: …{event.request_id.slice(-6)}</Typography>
        </>
      );
    case "DispatchOverride":
      return (
        <>
          {countHeader}
          <Typography variant="caption" fontWeight="bold">DispatchOverride</Typography>
          <Typography variant="caption" display="block">ts: {ts}</Typography>
          <Typography variant="caption" display="block">
            {event.active ? `active · ${event.setpoint_kw ?? "?"} kW` : "cleared"}
          </Typography>
        </>
      );
  }
}

// ─── Main component ───────────────────────────────────────────────────────────

type Props = { events: TraceEntry[] };

export function PlanTriggerTimeline({ events }: Props) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [anchorEl, setAnchorEl] = useState<HTMLElement | null>(null);
  const [selectedGroup, setSelectedGroup] = useState<Group | null>(null);

  // Input is newest-first; render oldest-left by reversing, then group consecutive identical events
  const groups = groupConsecutive([...events].reverse());

  // Auto-scroll to right end (newest event) on mount / when events change
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollLeft = scrollRef.current.scrollWidth;
    }
  }, [events]);

  const handleChipClick = (group: Group, el: HTMLElement) => {
    setSelectedGroup(group);
    setAnchorEl(el);
  };

  const handleClose = () => {
    setAnchorEl(null);
    setSelectedGroup(null);
  };

  return (
    <Box
      data-testid="trigger-timeline"
      ref={scrollRef}
      sx={{ overflowX: "auto", display: "flex", alignItems: "center", gap: 0.75, p: 1, minHeight: 40 }}
    >
      {groups.length === 0 ? (
        <Typography variant="caption" color="text.secondary">
          No controller events recorded yet.
        </Typography>
      ) : (
        groups.map((group, i) => {
          const count = group.events.length;
          const latest = group.events[group.events.length - 1];
          return (
            <Badge
              key={i}
              badgeContent={count > 1 ? count : undefined}
              color="default"
              sx={{ flexShrink: 0, "& .MuiBadge-badge": { fontSize: 9, minWidth: 16, height: 16, p: "0 3px" } }}
            >
              <Chip
                data-testid={`trigger-chip-${i}`}
                label={group.label}
                color={group.color}
                size="small"
                onClick={(e) => handleChipClick(group, e.currentTarget)}
                title={`${latest.type} — ${new Date(latest.ts).toLocaleTimeString()}${count > 1 ? ` (×${count})` : ""}`}
                sx={{ cursor: "pointer" }}
              />
            </Badge>
          );
        })
      )}

      <Popover
        open={Boolean(anchorEl)}
        anchorEl={anchorEl}
        onClose={handleClose}
        anchorOrigin={{ vertical: "bottom", horizontal: "left" }}
      >
        {selectedGroup && (
          <Stack
            data-testid="trigger-popover"
            spacing={0.25}
            sx={{ p: 1.5, minWidth: 160, maxWidth: 280 }}
          >
            <EventDetail group={selectedGroup} />
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
