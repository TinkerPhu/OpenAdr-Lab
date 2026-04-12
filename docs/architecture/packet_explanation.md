btw: the term 'packet' which is used in this project for planning. is it actually still usefull with milp? or is it rather hindering the new planning?

    In the MILP planner

    Packets have been demoted to input metadata. The MILP doesn't iterate over packets — it iterates over time slots and asset variables (p_ev_kw[t], z_heat_mid[t], etc.).     The packet's role is now:

    - request_mode → determines MilpLoadMode (MustRun / MayRun / MustNotRun)
    - deadline → sets the horizon constraint step (t_ev_dead_step)
    - target_energy_kwh / undelivered_energy_kwh() → sets the energy requirement (e_ev_core_kwh)
    - value_curve → bid prices for reward terms (MayRun reward)

    Everything else (packet ID, accumulated_cost_eur, per-slot profiles, status lifecycle) is bookkeeping for the dispatcher and reporting layers — not used by the solver 
    at all.

    ---
    What's genuinely useful

    1. Lifecycle tracking — Active/Pending/Completed/Abandoned states are still needed for the dispatcher and /packets API regardless of the planner algorithm.
    2. FlexibilityEnvelopes — Phase 5 uses packets to emit per-asset schedulability metadata for the VTN. This is a real use.
    3. User intent capture — request_mode, deadlines, and budgets are a clean way to express user intent that the MILP then encodes as hard/soft constraints.

    What's become awkward

    1. One packet per asset — MILP naturally thinks in assets, not tasks. The current active_packet() lookup (find the one active/pending packet for an asset) flattens the     multi-packet capability that the entity model allows. If two tasks compete for the EV simultaneously, the MILP can't represent that.
    2. Planned power profile on the packet — planned_power_profile is populated by the greedy planner's per-packet output. In MILP, the per-slot solution lives in
    PlanTimeSlot.allocations, making the field on the packet redundant/confusing.
    3. Accumulated cost / CO₂ — These are still useful for budget tracking but have nothing to do with MILP.

    ---
    Verdict

    The packet model isn't hindering the MILP — it's just more than the solver needs. The solver only cares about energy requirements, deadlines, and modes. The rest of   
    the packet model serves the broader HEMS (lifecycle management, reporting, user requests, envelopes). Worth keeping for those reasons, but the mental model shift is:  
    packets are now an intent/reporting layer, not a scheduling unit.