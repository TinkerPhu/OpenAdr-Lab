  graph TB      subgraph ADAPTERS["🔌 Adapters Layer"]
          direction LR          subgraph ROUTES["routes/"]
              R_ROUTER["build_router()"]
              R_HEMS["hems.rs\nPOST /plan\nGET /obligations\n/user-requests\n/ev-session"]
              R_SIM["sim.rs\nGET/POST /sim\n/sim/inject\n/sim/schema"]
              R_TL["timeline.rs\nGET /timeline/all\n/timeline/:asset_id"]
              R_ASSETS["assets.rs\nGET /forecast/:id\n/history/:id\n/capability/:id"]
              R_REPORTS["reports.rs\nGET/POST /reports"]
              R_TRACE["trace.rs\nGET /trace/events"]
          end
          subgraph TASKS["tasks/"]
              T_SIMTICK["spawn_sim_tick()\ntick_once()"]
              T_PLANNING["spawn_planning()"]
              T_EVENTS["spawn_event_poll()"]
              T_REPORTS["spawn_report_poll()"]
              T_OBLIG["spawn_obligation_check()"]
              T_PERSIST["spawn_state_persist()"]
          end
      end

      subgraph APP["⚙️  Application Layer"]
          SVC_PLAN["PlanningService\nadopt_if_warranted()\nevaluate_acceptance_gate()"]
          SVC_UREQ["UserRequestService\ncreate_ev()\ncreate_heater()\ncancel()"]
          SVC_OBLIG["ObligationService\ncheck_and_fulfill()"]
          SVC_HEMS["EvSessionService\nHvacService"]
      end

      subgraph DOMAIN["🎯 Domain / Controller"]
          direction TB
          subgraph ENTITIES["entities/"]
              E_PLAN["Plan\nPlanTimeSlot\nAssetAllocation"]
              E_ASSET["PlanTrigger\nAssetType\nPowerAdjustability"]
              E_CAP["OadrCapacityState\nOadrEventCache\nOadrReportObligation"]
              E_SESSION["EvSession\nHeaterTarget\nShiftableLoad"]
              E_TARIFF["TariffSnapshot\nTariffTimeSeries"]
              E_PARAMS["PlannerParams\nPlannerObjective\nSimulatorParams"]
          end
          subgraph CTRL["controller/"]
              C_PORT_SIM["≪trait≫\nSimulatorPort\nsnapshot() → SimSnapshot"]
              C_PORT_VTN["≪trait≫\nVtnPort\nfetch_events()\nfetch_reports()\nupsert_report()"]
              C_PORT_MILP["≪trait≫\nAssetMilpContext\nbuild_variables()\nextract_solution()"]
              C_DISPATCH["dispatcher.rs\nbuild_setpoints()"]
              C_ENVELOPE["envelope.rs\ncompute_flexibility_envelope()"]
              C_OAADR["openadr_interface.rs\nparse_rate_snapshots()\nparse_capacity_state()"]
              C_REPORTER["reporter.rs\nbuild_telemetry_usage_report()\nbuild_status_report()"]
              C_TIMELINE["timeline.rs\nbuild_asset_timeline()"]
              C_MILP["milp_planner/\nrun_planner()\nsolver_phase1\nsolver_phase2\nBatteryMilpContext\nEvMilpContext\nHeat
          end
          STATE["AppState\nactive_plan\nactive_requests\nev_session\ncapacity_state\ncontroller_trace\ntariff_ledger"]
      end

      subgraph INFRA["🏗️  Infrastructure"]
          direction LR
          subgraph SIM["simulator/"]
              SIM_STATE["SimState\ntick(setpoints)\nto_sim_snapshot()\nto_timeline_snapshot()\npersist load/save"]
              ASSETS_MOD["≪trait≫ Asset\nstep()\ncapability()\ncontrol_schema()"]
              A_BAT["Battery\nBatteryState"]
              A_EV["EvCharger\nEvState"]
              A_HTR["Heater\nHeaterState"]
              A_PV["PvInverter\nPvState"]
              A_BASE["BaseLoad\nBaseLoadState"]
              A_GRID["Grid\nGridState"]
          end
          VTN_CLIENT["vtn.rs\nVtnClient\nOAuth2 HTTP client"]
          PROFILE["profile.rs\nProfileConfig\nBatteryParams\nEvParams\nHeaterParams"]
          COMMON["common/mod.rs\nTimeSeries\nInterpolation\nAggregation"]
      end

      subgraph ENTRY["🚀 Entry / Context"]
          MAIN["main.rs\nbuild_domain_params()"]
          APPCTX["AppCtx\nstate: AppState\nvtn: VtnClient ⚠️ \nsim: Arc<Mutex<SimState>> ⚠️ \ntrigger_tx\nplanner_event_tx\
      end

      %% Port implementations
      SIM_STATE -. "impl SimulatorPort" .-> C_PORT_SIM
      VTN_CLIENT -. "impl VtnPort" .-> C_PORT_VTN
      A_BAT -. "impl AssetMilpContext" .-> C_PORT_MILP
      A_EV -. "impl AssetMilpContext" .-> C_PORT_MILP
      A_HTR -. "impl AssetMilpContext" .-> C_PORT_MILP
      ASSETS_MOD -. "impl Asset" .-> A_BAT & A_EV & A_HTR & A_PV & A_BASE & A_GRID

      %% Correct flows
      MAIN --> APPCTX
      APPCTX --> STATE
      APPCTX --> ROUTES
      APPCTX --> TASKS
      PROFILE --> MAIN

      T_PLANNING --> SVC_PLAN
      T_PLANNING --> C_PORT_SIM
      T_PLANNING --> C_MILP
      T_PLANNING --> C_ENVELOPE
      T_EVENTS --> C_OAADR
      T_EVENTS --> C_PORT_VTN
      T_REPORTS --> C_REPORTER
      T_OBLIG --> SVC_OBLIG
      T_SIMTICK --> C_DISPATCH

      R_HEMS --> SVC_UREQ
      R_HEMS --> SVC_HEMS

      C_MILP --> C_PORT_MILP
      C_REPORTER --> C_PORT_VTN
      SVC_OBLIG --> C_PORT_VTN

      ENTITIES --> DOMAIN

      %% ⚠️  Architectural Violations — all resolved as of 2026-07-03
      %% T_PLANNING: uses VtnPort trait (not VtnClient directly) ✓
      %% T_SIMTICK:  uses VtnPort trait (not VtnClient directly) ✓
      %% C_REPORTER: uses SimSnapshot from SimulatorPort (not SimState directly) ✓
      %% C_TIMELINE: uses only entities/ and trace types ✓
      %% SVC_UREQ:   uses only entities/ and state ✓

      %% Styling
      classDef port fill:#ddeeff,stroke:#336699,stroke-dasharray:5 5
      classDef entity fill:#eeffee,stroke:#336633
      classDef infra fill:#fff8ee,stroke:#996600
      classDef adapter fill:#f5eeff,stroke:#663399

      class C_PORT_SIM,C_PORT_VTN,C_PORT_MILP,ASSETS_MOD port
      class E_PLAN,E_ASSET,E_CAP,E_SESSION,E_TARIFF,E_PARAMS entity
      class SIM_STATE,VTN_CLIENT,PROFILE,COMMON,A_BAT,A_EV,A_HTR,A_PV,A_BASE,A_GRID infra
      class R_HEMS,R_SIM,R_TL,R_ASSETS,R_REPORTS,R_TRACE,T_EVENTS,T_REPORTS,T_OBLIG,T_PERSIST adapter

