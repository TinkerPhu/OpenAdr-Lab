╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌ Plan F — Two-Layer Control Loop with Deviation Transparency

 Context

 The MILP planner produces a 24h schedule optimized for cost/GHG/autarky/revenue, but once applied the dispatcher blindly tracks     
 plan setpoints tick-by-tick regardless of what actually happens. When PV drops due to cloud cover, base load spikes, or EV behaves  
 unexpectedly, the controller continues applying stale setpoints while silently over-importing from the grid.

 Two specific problems need solving:
 1. No reactive compensation: If PV generates 2 kW less than forecast, the grid makes up the difference. The battery could
 compensate immediately but nothing tells it to.
 2. No deviation-triggered replanning: The planner runs on a fixed 20s/300s timer. Sustained real-world divergence from the plan     
 does not trigger a fresh MILP solve until the timer fires, regardless of how far off-track the site is.

 This plan adds both layers and surfaces them via the existing SSE channel (Plan E, already implemented on
 feat/plan-e-sse-planner-status) so the user can see live corrections and deviation status in the Planner UI.

 ---
 Architecture

 Reality (PV cloud, base load spike)
         │
         ▼
 ┌──────────────────────────────────┐   every 1s
 │  Layer 1: Battery Correction     │◄──── sim tick loop
 │  apply_battery_correction_overlay│
 │  - reads: prev_actual_net_kw     │
 │  - reads: plan.current_slot.     │
 │           net_import_kw          │
 │  - adjusts: battery setpoint     │
 │  - emits: CorrectionActive SSE   │
 └──────────────────────────────────┘
         │ deviation still large after N ticks
         ▼
 ┌──────────────────────────────────┐   after ~30s sustained
 │  Layer 2: DeviceDeviation trigger│
 │  trigger_tx.send(DeviceDeviation)│
 │  → spawn_planning wakes, replans │
 │  → CorrectionCleared SSE         │
 └──────────────────────────────────┘

 Key design rule: Layer 1 is goal-aware (reads plan.objective to decide correction aggressiveness). Layer 2 replans from scratch     
 with current initial conditions — the existing MILP solve already does this correctly.

 ---
 Files to Create / Modify

 ┌─────────────────────────────────────────────┬─────────────────────────────────────────────────────────────────────────────────┐   
 │                    File                     │                                     Change                                      │   
 ├─────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────┤   
 │ docs/plans/plan_F_two_layer_control_loop.md │ New — this plan document                                                        │   
 ├─────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────┤   
 │ VEN/src/profile.rs                          │ Add 3 deviation/correction threshold fields to PlannerConfig                    │   
 ├─────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────┤   
 │ VEN/src/controller/dispatcher.rs            │ Add apply_battery_correction_overlay() + unit tests                             │   
 ├─────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────┤   
 │ VEN/src/planner_events.rs                   │ Add CorrectionActive, CorrectionCleared variants; add trigger field to          │   
 │                                             │ PlanReady                                                                       │   
 ├─────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────┤   
 │ VEN/src/loops.rs                            │ Deviation accumulator state, correction call, SSE emission; thread event_tx     │   
 │                                             │ param                                                                           │   
 ├─────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────┤   
 │ VEN/src/main.rs                             │ Move planner_event_tx creation before spawn_sim_tick; pass to spawn_sim_tick    │   
 ├─────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────┤   
 │ VEN/ui/src/api/types.ts                     │ Extend PlannerEvent union with 2 new variants; extend plan_ready                │   
 ├─────────────────────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────┤   
 │ VEN/ui/src/pages/Planner.tsx                │ Add CorrectionBanner component; extend status state machine; handle 2 new event │   
 │                                             │  types                                                                          │   
 └─────────────────────────────────────────────┴─────────────────────────────────────────────────────────────────────────────────┘   

 ---
 Step 1 — Create plan document in docs/plans/

 Create docs/plans/plan_F_two_layer_control_loop.md by copying this plan file content there (the canonical location for project      
 plans per workflow conventions).

 ---
 Step 2 — VEN/src/profile.rs: Add threshold config

 Add to PlannerConfig struct (after replan_interval_s, before the weight fields):

 /// Minimum absolute grid import error (kW) that activates battery correction.
 /// Layer 1 fires when |actual_net_kw − planned_net_kw| exceeds this value.
 /// Set to 0.0 to disable Layer 1 correction entirely. Default: 1.0 kW.
 #[serde(default = "default_deviation_threshold_kw")]
 pub deviation_threshold_kw: f64,

 /// Consecutive 1-second ticks of sustained deviation before a DeviceDeviation
 /// replan is triggered (Layer 2). Default: 30 (= 30 seconds).
 #[serde(default = "default_deviation_trigger_ticks")]
 pub deviation_trigger_ticks: u32,

 /// Minimum battery setpoint change to apply (noise floor). Corrections smaller
 /// than this are suppressed to avoid chattering. Default: 0.2 kW.
 #[serde(default = "default_correction_min_kw")]
 pub correction_min_kw: f64,

 Add default functions below the existing default_replan_interval:

 fn default_deviation_threshold_kw() -> f64 { 1.0 }
 fn default_deviation_trigger_ticks() -> u32 { 30 }
 fn default_correction_min_kw() -> f64 { 0.2 }

 ---
 Step 3 — VEN/src/controller/dispatcher.rs: Battery correction overlay

 Add after apply_surplus_ev_overlay. The function follows the exact same pattern (pure function, setpoints: &mut HashMap, returns    
 scalar).

 Before writing: Check exact battery field names in VEN/src/assets/ — look for BatteryConfig and BatteryState structs. Key fields    
 expected: capacity_kwh, max_charge_kw, max_discharge_kw, min_soc (max SoC is hardcoded to 1.0); state: soc: f64.

 /// Layer 1 reactive correction: adjust battery setpoint when actual grid import
 /// deviates from the plan's expectation by more than `threshold_kw`.
 ///
 /// Sign convention: positive setpoint = battery charging (importing), negative = discharging.
 /// Returns the correction delta applied to the battery setpoint (0.0 if no correction).
 pub fn apply_battery_correction_overlay(
     setpoints: &mut HashMap<String, f64>,
     assets: &[AssetEntry],
     asset_configs: &[AssetConfig],
     plan_net_import_kw: f64,     // plan.current_slot(now).net_import_kw
     actual_net_kw: f64,          // sim_guard.grid.net_power_w / 1000.0 (prev tick)
     objective: PlannerObjective,
     threshold_kw: f64,
     min_correction_kw: f64,
 ) -> f64 {
     let deviation_kw = actual_net_kw - plan_net_import_kw;
     if deviation_kw.abs() <= threshold_kw {
         return 0.0;
     }

     // Objective gate: MaxRevenue suppresses discharge corrections (preserve for export)
     if objective == PlannerObjective::MaxRevenue && deviation_kw > 0.0 {
         return 0.0;
     }

     // Find battery asset and config
     let Some(idx) = assets.iter().position(|a| a.id == "battery") else {
         return 0.0;
     };
     let (AssetState::Battery(bs), AssetConfig::Battery(bcfg)) =
         (&assets[idx].state, &asset_configs[idx])
     else {
         return 0.0;
     };

     let current_sp = setpoints.get("battery").copied().unwrap_or(0.0);
     // Correction direction: importing more than planned → discharge more (negative delta)
     let raw_target = current_sp - deviation_kw;

     // Clamp to power limits
     let clamped = raw_target.clamp(-bcfg.max_discharge_kw, bcfg.max_charge_kw);

     // SoC feasibility: don't discharge below min_soc, don't charge above 1.0 (max SoC is hardcoded)
     let clamped = if clamped < 0.0 && bs.soc <= bcfg.min_soc + 0.01 {
         current_sp.max(0.0) // already at floor, suppress discharge
     } else if clamped > 0.0 && bs.soc >= 1.0 - 0.01 {
         current_sp.min(0.0) // already at ceiling, suppress charge
     } else {
         clamped
     };

     let delta = clamped - current_sp;
     if delta.abs() < min_correction_kw {
         return 0.0;
     }

     setpoints.insert("battery".to_string(), clamped);
     delta
 }

 Unit tests (add to the existing #[cfg(test)] mod tests block, following the surplus_ev_overlay test pattern):

 - correction_discharges_battery_on_pv_shortfall: actual_net=3.0, planned_net=0.0, threshold=1.0 → battery sp decreases (negative    
 delta)
 - correction_suppressed_below_threshold: deviation = 0.5 kW with threshold 1.0 → returns 0.0
 - correction_suppressed_when_battery_at_min_soc: soc at min_soc + 0.005 → discharge correction returns 0.0
 - correction_suppressed_for_maxrevenue_discharge: objective=MaxRevenue, deviation > 0 → returns 0.0
 - correction_allows_maxrevenue_on_export_excess: objective=MaxRevenue, deviation < 0 (exporting more than planned) → correction     
 still applies (charge more)
 - correction_clamped_to_max_discharge_kw: large deviation → setpoint not below -max_discharge_kw

 ---
 Step 4 — VEN/src/planner_events.rs: Two new variants + trigger on PlanReady

 Extend the enum:

 #[derive(Debug, Clone, Serialize)]
 #[serde(tag = "type", rename_all = "snake_case")]
 pub enum PlannerEvent {
     SolvingStarted { ... },           // unchanged
     SolvingProgress { ... },          // unchanged
     PlanReady {
         plan_id: Uuid,
         objective: PlannerObjective,
         solver_ms: u64,
         objective_eur: f64,
         slot_count: usize,
         trigger: String,              // NEW: "DeviceDeviation" | "RateChange" | "Periodic" | ...
     },
     // ── NEW ──────────────────────────────────────────────────────────
     CorrectionActive {
         ts: DateTime<Utc>,
         asset_id: String,         // "battery"
         reason: String,           // "import_excess" | "export_excess"
         planned_net_kw: f64,
         actual_net_kw: f64,
         deviation_kw: f64,
         correction_kw: f64,       // delta applied to battery setpoint
         objective: PlannerObjective,
     },
     CorrectionCleared {
         ts: DateTime<Utc>,
         reason: String,           // "within_threshold" | "superseded_by_replan" | "battery_limit"
     },
 }

 The trigger field on PlanReady is additive (JSON object gains a field; existing TS consumers that don't reference it are
 unaffected).

 ---
 Step 5 — VEN/src/loops.rs: Deviation accumulator + correction call + SSE

 5a. Add event_tx: PlannerEventTx parameter to spawn_sim_tick

 pub(crate) fn spawn_sim_tick(
     state: AppState,
     sim: Arc<Mutex<SimState>>,
     profile: Arc<Profile>,
     ven_name: String,
     vtn: VtnClient,
     trigger_tx: Arc<tokio::sync::watch::Sender<PlanTrigger>>,
     data_dir: String,
     event_tx: PlannerEventTx,          // NEW
 ) -> tokio::task::JoinHandle<()>

 5b. Before the loop { block, declare accumulator state:

 let mut deviation_ticks: u32 = 0;
 let mut last_correction_kw: f64 = 0.0;
 let mut correction_is_active = false;

 5c. Inside tick loop, BEFORE build_setpoints() (inside sim_guard lock scope):

 // Capture previous tick's actual net import (before this tick's setpoints are applied).
 let prev_actual_net_kw = sim_guard.grid.net_power_w / 1000.0;
 let plan_net_kw = plan_snap.as_ref()
     .and_then(|p| p.current_slot(now))
     .map(|s| s.net_import_kw)
     .unwrap_or(0.0);

 5d. After build_setpoints() call, apply correction:

 // Layer 1: battery correction overlay (goal-aware reactive compensation)
 let correction_kw = if let Some(ref plan) = plan_snap {
     controller::dispatcher::apply_battery_correction_overlay(
         &mut sp_map,
         &sim_guard.assets,
         &sim_guard.asset_configs,
         plan_net_kw,
         prev_actual_net_kw,
         plan.objective,
         profile.planner.deviation_threshold_kw,
         profile.planner.correction_min_kw,
     )
 } else {
     0.0
 };

 5e. After correction, emit SSE when correction state changes (change threshold 0.2 kW):

 // Emit CorrectionActive/CorrectionCleared SSE on significant state change
 if (correction_kw - last_correction_kw).abs() > 0.2 {
     if correction_kw.abs() > profile.planner.correction_min_kw {
         let reason = if (prev_actual_net_kw - plan_net_kw) > 0.0 {
             "import_excess"
         } else {
             "export_excess"
         };
         let obj = plan_snap.as_ref().map(|p| p.objective).unwrap_or_default();
         let _ = event_tx.send(PlannerEvent::CorrectionActive {
             ts: now,
             asset_id: "battery".to_string(),
             reason: reason.to_string(),
             planned_net_kw: plan_net_kw,
             actual_net_kw: prev_actual_net_kw,
             deviation_kw: prev_actual_net_kw - plan_net_kw,
             correction_kw,
             objective: obj,
         });
         correction_is_active = true;
     } else if correction_is_active {
         let _ = event_tx.send(PlannerEvent::CorrectionCleared {
             ts: now,
             reason: "within_threshold".to_string(),
         });
         correction_is_active = false;
     }
     last_correction_kw = correction_kw;
 }

 5f. After sim_guard.tick() (post-tick, OUTSIDE the lock), deviation accumulation for Layer 2:

 Note: the lock scope ends at line ~524 with sim_guard.clone(). The sim_snapshot returned contains grid.net_power_w. Use it for      
 deviation counting.

 // Layer 2: accumulate sustained deviation → DeviceDeviation trigger
 if let Some(ref plan) = plan_snap {
     if let Some(slot) = plan.current_slot(now) {
         let post_net_kw = sim_snapshot.grid.net_power_w / 1000.0;
         let post_error_kw = (post_net_kw - slot.net_import_kw).abs();
         if post_error_kw > profile.planner.deviation_threshold_kw {
             deviation_ticks = deviation_ticks.saturating_add(1);
             if deviation_ticks >= profile.planner.deviation_trigger_ticks {
                 deviation_ticks = 0;
                 let _ = trigger_tx.send(PlanTrigger::DeviceDeviation);
             }
         } else {
             deviation_ticks = 0;
         }
     }
 }

 Placement: The sim_snapshot variable is the result of the { let mut sim_guard = ... sim_guard.clone() } block. It's already
 assigned but currently unused (let _ = sim_snapshot; at line 527). Remove that suppression and use it here.

 5g. In spawn_planning: add trigger to PlanReady and send CorrectionCleared after plan_ready:

 // After plan is stored and PlanCycle event emitted:
 let trigger_str = format!("{:?}", trigger);
 let _ = event_tx.send(PlannerEvent::PlanReady {
     plan_id: plan.id,
     objective: obj,
     solver_ms,
     objective_eur: plan.objective_eur,
     slot_count: plan.slots.len(),
     trigger: trigger_str,           // NEW
 });
 // Replan supersedes any active correction:
 let _ = event_tx.send(PlannerEvent::CorrectionCleared {
     ts: now,
     reason: "superseded_by_replan".to_string(),
 });

 ---
 Step 6 — VEN/src/main.rs: Move planner_event_tx before spawn_sim_tick

 Currently planner_event_tx is created at line 129, AFTER spawn_sim_tick is called at line 113.

 Move lines 129–130 to BEFORE spawn_sim_tick (before line 113), then pass planner_event_tx.clone() as the last argument:

 // Move to before spawn_sim_tick:
 let (planner_event_tx_inner, _) = tokio::sync::broadcast::channel::<PlannerEvent>(128);
 let planner_event_tx: PlannerEventTx = Arc::new(planner_event_tx_inner);

 tasks::spawn_sim_tick(
     state.clone(),
     sim_state.clone(),
     profile.clone(),
     cfg.ven_name.clone(),
     vtn.clone(),
     trigger_tx.clone(),
     data_dir.clone(),
     planner_event_tx.clone(),   // NEW
 );

 The planner_event_tx.clone() passed to spawn_planning (line 139) stays unchanged. The AppCtx assignment at line 154 also stays      
 unchanged.

 ---
 Step 7 — VEN/ui/src/api/types.ts: Extend PlannerEvent union

 Current (3-variant union at line ~392–395). Extend to 5 variants:

 export type PlannerEvent =
   | { type: "solving_started"; objective: PlannerObjective; num_slots: number; triggered_at: string }
   | { type: "solving_progress"; elapsed_ms: number; iteration: number }
   | { type: "plan_ready"; plan_id: string; objective: PlannerObjective; solver_ms: number;
       objective_eur: number; slot_count: number; trigger: string }  // trigger field added
   | { type: "correction_active"; ts: string; asset_id: string; reason: string;
       planned_net_kw: number; actual_net_kw: number; deviation_kw: number;
       correction_kw: number; objective: PlannerObjective }
   | { type: "correction_cleared"; ts: string; reason: string };

 ---
 Step 8 — VEN/ui/src/pages/Planner.tsx: CorrectionBanner + state machine

 8a. Extend PlannerStatus type and add CorrectionStatus:

 // Existing — add trigger to updated phase:
 type PlannerStatus =
   | { phase: "idle" }
   | { phase: "solving"; elapsed_ms: number; iteration: number; objective: PlannerObjective }
   | { phase: "updated"; solver_ms: number; trigger: string };   // trigger field added

 // New:
 type CorrectionStatus =
   | { active: false }
   | { active: true; asset_id: string; reason: string;
       planned_net_kw: number; actual_net_kw: number;
       deviation_kw: number; correction_kw: number; objective: PlannerObjective };

 8b. Add correctionStatus state:

 const [correctionStatus, setCorrectionStatus] = useState<CorrectionStatus>({ active: false });

 8c. Extend usePlannerEvents callback to handle new event types:

 usePlannerEvents(useCallback((event: PlannerEvent) => {
   if (event.type === "solving_started") {
     setPlannerStatus({ phase: "solving", elapsed_ms: 0, iteration: 0, objective: event.objective });
   } else if (event.type === "solving_progress") {
     setPlannerStatus((prev) =>
       prev.phase === "solving" ? { ...prev, elapsed_ms: event.elapsed_ms, iteration: event.iteration } : prev
     );
   } else if (event.type === "plan_ready") {
     setPlannerStatus({ phase: "updated", solver_ms: event.solver_ms, trigger: event.trigger });
     queryClient.invalidateQueries({ queryKey: ["plan"] });
     setTimeout(() => setPlannerStatus({ phase: "idle" }), 3000);
   } else if (event.type === "correction_active") {
     setCorrectionStatus({
       active: true, asset_id: event.asset_id, reason: event.reason,
       planned_net_kw: event.planned_net_kw, actual_net_kw: event.actual_net_kw,
       deviation_kw: event.deviation_kw, correction_kw: event.correction_kw,
       objective: event.objective,
     });
   } else if (event.type === "correction_cleared") {
     setCorrectionStatus({ active: false });
   }
 }, [queryClient]));

 8d. Add CorrectionBanner inline component:

 function CorrectionBanner({ status }: { status: CorrectionStatus }) {
   if (!status.active) return null;
   const directionLabel = status.deviation_kw > 0 ? "import excess" : "export excess";
   const corrLabel = status.correction_kw < 0
     ? `discharge +${Math.abs(status.correction_kw).toFixed(1)} kW`
     : `charge reduced ${status.correction_kw.toFixed(1)} kW`;
   return (
     <Alert severity="info" icon={<BoltIcon fontSize="small" />} sx={{ mb: 1 }}>
       <strong>Reactive correction active — {status.asset_id}</strong>
       {" "}Grid {directionLabel}: {Math.abs(status.deviation_kw).toFixed(1)} kW above plan
       (planned {status.planned_net_kw.toFixed(1)} kW, actual {status.actual_net_kw.toFixed(1)} kW).
       Battery {corrLabel}. Objective: {status.objective}.
     </Alert>
   );
 }

 Uses MUI Alert (already a dependency). BoltIcon from @mui/icons-material/Bolt (already used elsewhere — verify).

 8e. Render in JSX, above PlannerStatusBar:

 <CorrectionBanner status={correctionStatus} />
 <PlannerStatusBar status={plannerStatus} />

 Also update PlannerStatusBar to show trigger context on phase === "updated":
 // phase === "updated"
 return (
   <Chip
     size="small"
     color={status.trigger === "DeviceDeviation" ? "warning" : "success"}
     label={`Plan updated (${status.trigger}) — solved in ${(status.solver_ms / 1000).toFixed(1)} s`}
     sx={{ mb: 1 }}
   />
 );

 ---
 Critical Implementation Notes

 1. sim_snapshot.grid.net_power_w vs sim_guard.grid.net_power_w: The value sim_guard.grid.net_power_w is the net power BEFORE the    
 current tick's setpoints are applied (i.e., previous tick outcome) when read before sim_guard.tick(). Use this for
 prev_actual_net_kw. After tick(), sim_guard.grid.net_power_w reflects the current tick — use sim_snapshot.grid.net_power_w (from    
 the cloned guard returned after the lock scope) for Layer 2 accumulation.
 2. let _ = sim_snapshot; at loops.rs:527: Remove this suppressor. The variable is already used by reporting below it. After this    
 change it's also used by deviation accumulation — no issue.
 3. Battery field names: Before writing apply_battery_correction_overlay, grep VEN/src/assets/ for BatteryConfig and BatteryState to 
  confirm exact field names. **Resolved**: Battery has no soc_max field; max SoC is hardcoded to 1.0.
 4. PlannerObjective import in dispatcher.rs: PlannerObjective lives in crate::profile. Add use crate::profile::PlannerObjective; to 
  the imports.
 5. No BDD test changes required: The new behavior is additive. Existing scenarios are unaffected since the correction overlay only  
 fires when plan deviates from reality (BDD tests inject controlled state). However, a new BDD scenario testing the correction is    
 desirable — defer to a follow-up.
 6. No SSE test changes required: usePlannerEvents in Vitest is already mocked; new event types just add branches to the callback.
