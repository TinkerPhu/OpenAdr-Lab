Here's the full test landscape:                                                                             
                                                                                                              

  ┌─────────────────────┬───────────────┬───────────────────────┬──────────────────┐
  │        Layer        │   Framework   │         Count         │     Location     │
  ├─────────────────────┼───────────────┼───────────────────────┼──────────────────┤
  │ BDD integration     │ Python/Behave │ 232 scenarios         │ tests/features/  │
  ├─────────────────────┼───────────────┼───────────────────────┼──────────────────┤
  │ Rust unit tests     │ cargo test    │ 313 #[test] functions │ VEN/src/**/*.rs  │
  ├─────────────────────┼───────────────┼───────────────────────┼──────────────────┤
  │ Frontend unit tests │ Vitest        │ 32 test files         │ VEN/ui/, VTN/ui/ │
  └─────────────────────┴───────────────┴───────────────────────┴──────────────────┘

  The BDD instrumentation we added covers all 232 scenarios — that's the slow layer (50+ min). The other two
  layers are not part of the same test run.

Vitest results already in: VEN/ui 476s (7m56s), VTN/ui 178s (2m58s).






commit: a5049ff1604281193bb4a853c05dda0fd646b6c0
====================================================================================================
TEST TIMING SUMMARY — ALL SCENARIOS (slowest first)
====================================================================================================
  #   steps_s  cleanup_s   total_s  st  feature :: scenario
----------------------------------------------------------------------------------------------------
  1     326.9        0.3     327.2   ⊘  Planner Visualization Page :: Decision matrix collapses and expands [ven-ui]
  2     239.9        0.9     240.8   ⊘  Planner Visualization Page :: Clicking a matrix cell with a step opens the step detail drawer [ven-ui]
  3     193.7        0.2     193.9   ⊘  Shiftable Load Lifecycle (Plan B) :: Shiftable load auto-completes and disappears from GET /sim [slow]
  4     178.0        1.0     179.0   ⊘  Planner Visualization Page :: Plan header shows trigger badge and summary values [ven-ui]
  5     127.2        0.2     127.4   ⊘  Shiftable Load Lifecycle (Plan B) :: Running shiftable load appears in GET /sim
  6     118.0        0.3     118.3   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-12a — Multi-asset plan with import cap allocates EV within cap
  7     105.6        1.2     106.9   ⊘  Planner Visualization Page :: Decision matrix renders asset rows and tariff header [ven-ui]
  8     100.9        0.3     101.2   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-12c — Ledger accumulates energy for all active assets concurrently
  9      94.2        2.3      96.5   ⊘  Controller V2 — Simulation Controls :: Toggling EV plugged switch triggers a POST to sim override [ven-ui]
 10      91.9        0.3      92.2   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-12b — Plan warnings are accessible when capacity is constrained
 11      91.0        0.1      91.2   ⊘  Shiftable Load Lifecycle (Plan B) :: Deleting a running shiftable load removes it from GET /sim
 12      89.8        1.1      90.8   ⊘  Planner Visualization Page :: Clicking a trigger chip shows detail popover [ven-ui]
 13      84.5        1.6      86.0   ⊘  Controller V2 — Simulation Controls :: EV plugged toggle is visible in right section [ven-ui]
 14      80.8        0.2      80.9   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: Plan allocates EV to slots given a cheap PRICE event
 15      80.6        0.1      80.7   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: EV session drives the planner to allocate EV charging power
 16      77.8        0.3      78.1   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-10a — Plan slots reflect the import capacity limit from VTN
 17      75.6        0.2      75.8   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-01a — Explicit EV session is planned and allocated
 18      73.4        2.2      75.6   ⊘  Controller V2 — Navigation and Layout Controls :: Right section starts collapsed and can be expanded then collapsed [ven-ui]
 19      75.4        0.2      75.6   ⊘  Shiftable Load Lifecycle (Plan B) :: Shiftable load appears in plan allocations after POST
 20      72.1        2.3      74.4   ⊘  Controller V2 — Simulation Controls :: EV SoC slider is visible in the Status Settings accordion [ven-ui]
 21      69.3        1.1      70.4   ⊘  Planner Visualization Page :: Trigger timeline shows at least one event chip [ven-ui]
 22      66.7        1.4      68.1   ⊘  Planner Visualization Page :: Trigger timeline section is visible on Planner page [ven-ui]
 23      66.8        1.1      67.9   ⊘  Controller V2 — Asset Cell Content :: Asset cell left section shows power, cost rate, and CO2eq rate [ven-ui]
 24      67.7        0.1      67.9   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-10b — Plan net_import_kw does not exceed the capacity limit
 25      66.2        0.9      67.2   ⊘  Planner Visualization Page :: Decision matrix section is visible on Planner page [ven-ui]
 26      65.1        1.5      66.6   ⊘  Planner Visualization Page :: Plan header section is visible on Planner page [ven-ui]
 27      61.2        1.6      62.8   ⊘  Controller V2 — Asset Cell Content :: Asset cell mid section shows a NOW reference line [ven-ui]
 28      61.7        0.3      62.0   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-06b — Plan slots respect an import capacity limit
 29      61.3        0.3      61.6   ⊘  VEN User Request Manager — Stage 5 :: Interruptible scheduled EV session contributes to up_kw in flexibility envelope
 30      60.8        0.3      61.1   ⊘  VEN EV Charging Scenarios (Chunk 4) :: (b) IMPORT_CAPACITY_LIMIT event caps net import in plan slots
 31      58.3        1.6      59.9   ⊘  Planner Visualization Page :: Decision matrix collapse button is present [ven-ui]
 32      53.7        1.5      55.2   ⊘  Controller V2 — Navigation and Layout Controls :: Pinning a cell moves it to the pinned zone [ven-ui]
 33      53.3        1.1      54.5   ⊘  Controller V2 — Navigation and Layout Controls :: Unpinning a cell removes it from the pinned zone [ven-ui]
 34      52.4        1.9      54.3   ⊘  Controller V2 — Page Layout :: Grid cells appear above asset cells by default [ven-ui]
 35      49.2        0.9      50.1   ⊘  Controller V2 — Navigation and Layout Controls :: Pin button is present on each cell [ven-ui]
 36      49.6        0.1      49.7   ⊘  Heater tank MILP trajectory model :: Plan uses only mid-tier heater (not full-tier) near T_max
 37      43.6        1.7      45.3   ⊘  Controller V2 — Asset Cell Content :: Global time range extend button is visible in the title bar [ven-ui]
 38      43.4        0.1      43.6   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: Layer 2 triggers a DeviceDeviation replan after sustained grid deviation
 39      41.0        1.8      42.7   ⊘  Controller V2 — Asset Cell Content :: Base load asset cell mid section shows a timeline chart [ven-ui]
 40      41.4        1.3      42.7   ⊘  Controller V2 — Asset Cell Content :: Battery asset cell mid section shows a timeline chart [ven-ui]
 41      36.1        1.6      37.7   ⊘  Controller V2 — Page Layout :: Page is scrollable and grid cells are not fixed by default [ven-ui]
 42      35.4        1.4      36.8   ⊘  Controller V2 — Page Layout :: At least one asset cell is present [ven-ui]
 43      34.5        1.6      36.1   ⊘  Controller V2 — Asset Cell Content :: Battery asset cell shows State of Charge [ven-ui]
 44      34.6        0.9      35.5   ⊘  Planner Visualization Page :: Navigate to Planner page [ven-ui]
 45      33.7        0.4      34.1   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC7 - Connectivity Check (open, round-trip) [ui]
 46      30.5        0.8      31.3   ⊘  VEN EV Charging Scenarios (Chunk 4) :: (c) Zero IMPORT_CAPACITY_LIMIT is reflected in plan slots
 47      30.1        0.1      30.2   ⊘  Asset Interface — history(timespan) :: Requesting more history than available returns partial result without error
 48      30.1        0.1      30.2   ⊘  Asset Interface — history(timespan) :: Battery 30-minute history returns samples
 49      30.1        0.1      30.2   ⊘  Asset Interface — history(timespan) :: PV history boundary point is present at start of window
 50      30.1        0.1      30.2   ⊘  Asset Interface — history(timespan) :: No future-timestamped entries in history response
 51      30.1        0.1      30.2   ⊘  Asset Interface — history(timespan) :: PV 30-minute history returns samples
 52      26.0        0.3      26.3   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC8 - Event Cancellation (delete via UI, VEN loses event) [ui]
 53      25.4        0.2      25.6   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC6 - Battery Dispatch (targeted to VEN-1 only) [ui]
 54      24.8        0.4      25.2   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC4 - Peak Shaving (targeted to VEN-1 and VEN-2) [ui]
 55      22.8        0.2      23.1   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC5 - EV Charging (targeted to VEN-2 with event-level targets) [ui]
 56      22.2        0.2      22.4   ⊘  Reporter multi-interval resampling (RF-05e) :: Obligation-based report contains multiple intervals [reporter-resampling]
 57      20.4        0.1      20.5   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC2 - Export Limitation (targeted to VEN-2 only) [ui]
 58      19.5        0.4      19.8   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC3 - Dynamic Pricing (open to all VENs) [ui]
 59      19.1        0.5      19.6   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC1 - Emergency Load Shed (targeted to VEN-1 only) [ui]
 60      18.5        0.5      19.1   ⊘  VEN Raw Data Diagnostics Page :: Timeline dropdown filters series [ven-ui]
 61      13.1        0.6      13.6   ⊘  VEN Raw Data Diagnostics Page :: Timeline cell shows series dropdown and refreshes [ven-ui]
 62      10.9        0.2      11.0   ⊘  Failure Recovery :: VEN recovers after its own restart [resilience]
 63      10.4        0.2      10.5   ⊘  Reporter multi-interval resampling (RF-05e) :: Timer-driven report without reportDescriptor is single-interval [reporter-resampling]
 64       9.7        0.6      10.3   ⊘  VEN Raw Data Diagnostics Page :: Tariffs cell refreshes on button click [ven-ui]
 65       8.5        0.2       8.7   ⊘  VEN Raw Data Diagnostics Page :: Sim cell refreshes on button click [ven-ui]
 66       8.2        0.5       8.6   ⊘  VEN Raw Data Diagnostics Page :: Each cell refreshes independently [ven-ui]
 67       7.9        0.4       8.3   ⊘  Planner Visualization Page :: Planner tab appears in navigation [ven-ui]
 68       7.2        0.3       7.5   ⊘  VEN Raw Data Diagnostics Page :: Page renders three diagnostic cells [ven-ui]
 69       6.5        0.2       6.7   ⊘  VEN Simulator :: Auto-report submitted for active event
 70       6.0        0.1       6.2   ⊘  Failure Recovery :: VEN re-syncs after VTN restart [resilience]
 71       3.2        2.7       5.9   ⊘  Failure Recovery :: VEN retains cached events when VTN goes down [resilience]
 72       5.4        0.1       5.5   ⊘  Failure Recovery :: Both VENs converge after VTN restart [resilience]
 73       5.2        0.2       5.4   ⊘  Phase A — Asset physics and capability coverage :: pv_irradiance override to zero silences PV output
 74       5.3        0.1       5.4   ⊘  Phase A — Asset physics and capability coverage :: pv_irradiance override to full produces nonzero PV export
 75       4.4        0.1       4.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC3c - Price correction after initial publish
 76       4.3        0.1       4.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC8 - Event Cancellation (VEN-1 sees then loses event)
 77       3.5        0.1       3.6   ⊘  VEN Reports :: Submit report via VEN and verify round-trip
 78       3.3        0.2       3.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC4b - Modify peak shaving limit mid-flight
 79       2.5        0.2       2.6   ⊘  OpenADR Use Cases — Full End-to-End :: UC5 - EV Charging (targeted to VEN-2 with event-level targets)
 80       2.3        0.3       2.6   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-06a — IMPORT_CAPACITY_LIMIT event updates /capacity import_limit_kw
 81       2.4        0.2       2.6   ⊘  OpenADR Use Cases — Full End-to-End :: UC2 - Export Limitation (targeted to VEN-2 only)
 82       2.3        0.3       2.5   ⊘  VEN Program Enrollment :: Targeted program is visible only to enrolled VEN
 83       2.3        0.2       2.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC6b - Conflicting charge and discharge events
 84       2.3        0.2       2.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC5b - Overlapping EV events with different priorities
 85       2.2        0.2       2.5   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: IMPORT_CAPACITY_LIMIT event updates the capacity state
 86       2.2        0.2       2.4   ⊘  Phase A — Asset physics and capability coverage :: ev_plugged false stops EV charging capability
 87       2.2        0.2       2.4   ⊘  Phase A — Asset physics and capability coverage :: EV unplugged reports zero capability in both directions
 88       2.3        0.1       2.4   ⊘  OpenADR Use Cases — Full End-to-End :: UC3 - Dynamic Pricing (open to all VENs)
 89       2.1        0.2       2.3   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-08b — Re-plugging the EV resumes charging
 90       1.5        0.2       1.7   ⊘  OpenADR Use Cases — Full End-to-End :: UC6 - Battery Dispatch (targeted to VEN-1 only)
 91       1.4        0.2       1.6   ⊘  OpenADR Use Cases — Full End-to-End :: UC7 - Connectivity Check (open, no-op round-trip)
 92       1.3        0.2       1.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC4 - Peak Shaving (targeted to VEN-1 and VEN-2)
 93       1.3        0.2       1.5   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: GET /obligations returns empty list when events have no reportDescriptors
 94       1.3        0.2       1.4   ⊘  VEN Program Enrollment :: Open program is visible to all VENs
 95       1.1        0.2       1.4   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: GHG event produces rate snapshots with co2_g_kwh values
 96       1.2        0.2       1.4   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-04a — PRICE event from VTN populates /tariffs with import prices
 97       1.1        0.2       1.4   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-03 — PV surplus accumulates in the asset ledger
 98       1.2        0.2       1.4   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: EXPORT_PRICE event produces rate snapshots with export_tariff_eur_kwh
 99       1.2        0.2       1.3   ⊘  VEN-VTN Integration :: VEN reflects events created in VTN
100       1.1        0.2       1.3   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: Layer 1 corrects grid deviation immediately using battery
101       1.2        0.1       1.3   ⊘  OpenADR Use Cases — Full End-to-End :: UC3b - Day-ahead pricing with 24 hourly intervals
102       1.1        0.1       1.2   ⊘  VEN-VTN Integration :: VEN reflects programs created in VTN
103       0.3        0.9       1.2   ⊘  VTN Event Management :: List events returns the created event
104       0.9        0.2       1.1   ⊘  VEN Isolation :: VEN can only see its own reports
105       0.4        0.5       0.9   ⊘  VEN EV Charging Scenarios (Chunk 4) :: (f) User request with zero IMPORT_CAPACITY_LIMIT is reflected in plan
106       0.8        0.2       0.9   ⊘  VEN Isolation :: VEN cannot retrieve another VEN's report by ID
107       0.6        0.2       0.8   ⊘  VEN Isolation :: VEN can only see its own VEN record
108       0.2        0.6       0.8   ⊘  VTN Event Active Filter :: active=true returns only current events
109       0.2        0.6       0.8   ⊘  VTN Event Active Filter :: no active filter returns all events
110       0.1        0.6       0.8   ⊘  VTN Event Management :: Create an event for a program
111       0.2        0.5       0.7   ⊘  VTN Event Active Filter :: active=false returns only past events
112       0.2        0.5       0.7   ⊘  VTN Program Management :: Create a program
113       0.1        0.6       0.7   ⊘  VTN Authentication :: Valid credentials return an access token
114       0.1        0.6       0.7   ⊘  VEN User Request Manager — Stage 5 :: Cancelling a user request clears the linked EV session
115       0.1        0.6       0.7   ⊘  VTN Authentication :: Invalid credentials are rejected
116       0.1        0.5       0.6   ⊘  VEN User Request Manager — Stage 5 :: Request for a non-storage asset is rejected
117       0.5        0.1       0.6   ⊘  Uniform-Grid Timeline API (RF-05c) :: Grid timestamps are snapped to round boundaries
118       0.3        0.2       0.6   ⊘  Uniform-Grid Timeline API (RF-05c) :: GET /timeline/all returns arrays of equal length for all assets
119       0.3        0.3       0.6   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-09c — Cancelled request clears the linked EV session
120       0.1        0.4       0.5   ⊘  VTN Program Management :: List programs includes the created program
121       0.4        0.1       0.5   ⊘  Uniform-Grid Timeline API (RF-05c) :: Grid-portion timestamps are uniformly spaced
122       0.3        0.2       0.5   ⊘  Uniform-Grid Timeline API (RF-05c) :: Response format is unchanged
123       0.2        0.2       0.5   ⊘  BFF Event CRUD :: Delete an event via BFF
124       0.2        0.2       0.5   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-05c — Each flexibility envelope in /plan has energy_needed and rate range fields
125       0.1        0.4       0.5   ⊘  VEN User Request Manager — Stage 5 :: GET /flexibility returns a site-level flexibility object
126       0.1        0.4       0.5   ⊘  VEN User Request Manager — Stage 5 :: Request with tolerance_min and interruptible stores leeway fields
127       0.1        0.4       0.5   ⊘  VTN Program Management :: Unauthenticated request is rejected
128       0.2        0.3       0.5   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-01b— EV charge plan has FLEXIBLE envelopes for far-horizon energy
129       0.2        0.2       0.4   ⊘  BFF Program CRUD :: Delete a program via BFF
130       0.3        0.2       0.4   ⊘  Uniform-Grid Timeline API (RF-05c) :: All assets share the same ts at each index position
131       0.3        0.1       0.4   ⊘  Uniform-Grid Timeline API (RF-05c) :: resolution=30 returns 30-second spacing
132       0.1        0.3       0.4   ⊘  VEN User Request Manager — Stage 5 :: POST /user-requests creates a user request with a linked EV session
133       0.1        0.4       0.4   ⊘  VEN User Request Manager — Stage 5 :: Budget ceiling via budget_eur is reflected in user request
134       0.2        0.2       0.4   ⊘  Uniform-Grid Timeline API (RF-05c) :: Default auto-resolution targets approximately 300 points
135       0.1        0.3       0.4   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-02b — CONTINUE policy request appears in /user-requests list
136       0.1        0.3       0.4   ⊘  VEN User Request Manager — Stage 5 :: Multi-tier request has two deadline tiers in the linked packet
137       0.1        0.3       0.4   ⊘  VEN Entity Model — Stage 1 Foundation :: GET /sim includes battery field when battery is configured in profile
138       0.1        0.3       0.4   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-11c — Asset ledger tracks energy in active assets
139       0.1        0.3       0.4   ⊘  VEN User Request Manager — Stage 5 :: User request appears in GET /user-requests
140       0.1        0.3       0.4   ⊘  Uniform-Grid Timeline API (RF-05c) :: resolution takes precedence over max_points
141       0.1        0.2       0.4   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: EV session appears in /ev-session after POST
142       0.2        0.2       0.4   ⊘  Uniform-Grid Timeline API (RF-05c) :: The now-point ts is the same across all assets
143       0.1        0.3       0.4   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-02a — CONTINUE policy request has two deadline tiers
144       0.2        0.2       0.4   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-04b — Plan after PRICE event has rate-priced slots
145       0.2        0.2       0.4   ⊘  VEN EV Charging Scenarios (Chunk 4) :: (e) User request capped by IMPORT_CAPACITY_LIMIT event
146       0.1        0.2       0.3   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-05d — GET /flexibility returns site-level headroom with correct shape [phase-e]
147       0.1        0.2       0.3   ⊘  BFF Event CRUD :: Create an event via BFF
148       0.2        0.1       0.3   ⊘  OpenADR Use Cases — Full End-to-End :: UC1 - Emergency Load Shed (targeted to VEN-1 only)
149       0.1        0.3       0.3   ⊘  VEN Entity Model — Stage 1 Foundation :: Battery power_kw is present in sim response
150       0.1        0.3       0.3   ⊘  VEN Entity Model — Stage 1 Foundation :: Existing /sim endpoint returns structured ts + grid + assets
151       0.1        0.3       0.3   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-05a — Plan has slots covering the planning horizon
152       0.1        0.2       0.3   ⊘  Uniform-Grid Timeline API (RF-05c) :: Each asset array contains a now-point between history and future
153       0.2        0.1       0.3   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: Plan has flexibility envelopes for far-horizon unscheduled energy
154       0.1        0.2       0.3   ⊘  VEN Entity Model — Stage 1 Foundation :: GET /tariffs returns a JSON array
155       0.1        0.2       0.3   ⊘  VEN User Request Manager — Stage 5 :: User request with budget constraint includes max_total_cost in linked packet
156       0.1        0.2       0.3   ⊘  Phase A — Asset physics and capability coverage :: PV always reports fixed (non-curtailable) capability
157       0.1        0.2       0.3   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: DELETE /heater-target clears the target
158       0.1        0.2       0.3   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-09b — Tight budget tier-1 request still creates a valid EV session link
159       0.1        0.2       0.3   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-11b — Plan runs without crashing when no packets match any slot
160       0.1        0.1       0.3   ⊘  Uniform-Grid Timeline API (RF-05c) :: max_points=150 produces equivalent resolution
161       0.1        0.2       0.3   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: EV session drives the planner to allocate EV power
162       0.1        0.2       0.3   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-01c — EV packet estimated cost is tracked in the plan
163       0.1        0.2       0.3   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: EV session drives dispatcher to allocate power to EV
164       0.1        0.2       0.3   ⊘  VEN-VTN Integration :: VEN generates sensor data automatically
165       0.1        0.2       0.3   ⊘  VEN Simulator :: Sim endpoint shows configured devices
166       0.1        0.2       0.3   ⊘  Asset Interface — forecast(timespan) :: Battery forecast returns power series with linear interpolation
167       0.1        0.2       0.3   ⊘  VEN Entity Model — Stage 1 Foundation :: Existing /health endpoint still works
168       0.1        0.2       0.3   ⊘  VEN Entity Model — Stage 1 Foundation :: GET /trace/events endpoint returns a JSON array
169       0.1        0.2       0.3   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: 3-interval PRICE event produces 3 rate snapshots
170       0.1        0.2       0.3   ⊘  VEN Simulator :: Sim endpoint top-level shape is ts + grid + assets
171       0.1        0.2       0.3   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/ev returns a sorted JSON array
172       0.1        0.2       0.3   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-11a — Planner produces a valid plan even with no new user requests
173       0.1        0.2       0.3   ⊘  Uniform-Grid Timeline API (RF-05c) :: GET /timeline/ev returns uniformly spaced ts with now-point
174       0.1        0.1       0.3   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: GET /ledger returns per-asset energy accumulation after charging
175       0.1        0.1       0.3   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-08a — Unplugging the EV via override drops EV current to zero
176       0.1        0.2       0.3   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-09a — Multi-tier request with tight tier-1 budget has tier_count 2
177       0.1        0.2       0.2   ⊘  Asset Interface — forecast(timespan) :: Heater forecast returns linear-interpolated power series
178       0.1        0.1       0.2   ⊘  BFF Program CRUD :: Update a program via BFF
179       0.1        0.1       0.2   ⊘  Uniform-Grid Timeline API (RF-05c) :: Empty future buckets have values null
180       0.1        0.2       0.2   ⊘  VEN Entity Model — Stage 1 Foundation :: Battery SoC stays within valid range over time
181       0.1        0.1       0.2   ⊘  Heater tank MILP trajectory model :: Cheap PRICE event attracts heater into cheap tariff window
182       0.1        0.2       0.2   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: GET /capacity returns a JSON object with expected fields
183       0.1        0.1       0.2   ⊘  Shiftable Load Lifecycle (Plan B) :: POST rejects duplicate asset_id with 409
184       0.1        0.1       0.2   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/all returns all configured assets and grid
185       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: GET /ev-session returns the active session after POST
186       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: POST /shiftable-loads adds a shiftable load
187       0.1        0.2       0.2   ⊘  VEN Entity Model — Stage 1 Foundation :: GET /trace/history returns asset history rows for EV
188       0.1        0.1       0.2   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: Plan slots cover the planning horizon
189       0.1        0.2       0.2   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/grid returns a sorted array
190       0.1        0.1       0.2   ⊘  Phase A — Asset physics and capability coverage :: Battery at full SoC reports zero import capability
191       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: POST /heater-target creates a heater target
192       0.1        0.2       0.2   ⊘  VEN Simulator :: Sim grid object has expected fields
193       0.1        0.2       0.2   ⊘  VEN Simulator :: EV asset has expected fields in sim response
194       0.1        0.1       0.2   ⊘  VEN Entity Model — Stage 1 Foundation :: Existing /events endpoint still works
195       0.1        0.1       0.2   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: GET /plan returns a non-null plan after VEN starts
196       0.0        0.2       0.2   ⊘  VEN Simulator :: Heater sim schema exposes all four controls
197       0.1        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: EV charger forecast returns step-interpolated power series
198       0.1        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: Base load forecast returns constant step-interpolated power series
199       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: DELETE /shiftable-loads/{id} removes the load
200       0.1        0.2       0.2   ⊘  VEN Simulator :: Battery asset has expected fields in sim response
201       0.0        0.2       0.2   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-07 — GET /capacity returns a valid capacity state object
202       0.1        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: PV forecast during daytime returns non-empty power series
203       0.1        0.1       0.2   ⊘  Phase A — Asset physics and capability coverage :: Battery at empty SoC reports zero export capability
204       0.1        0.1       0.2   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: EvSession appears in GET /ev-session after POST
205       0.0        0.2       0.2   ⊘  VEN Simulator :: Heater asset has expected fields in sim response
206       0.1        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: PV forecast boundary point is present at end of timespan
207       0.1        0.1       0.2   ⊘  BFF Reports :: List reports via BFF
208       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: DELETE /ev-session removes the session
209       0.0        0.1       0.2   ⊘  VEN Health Check :: Health endpoint returns ok
210       0.1        0.1       0.2   ⊘  VEN Sensor Data :: POST sensor data and GET it back
211       0.1        0.1       0.2   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/ev response points have ts and values fields
212       0.1        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: Heater forecast has non-zero average power
213       0.0        0.1       0.2   ⊘  BFF Program CRUD :: Create a program via BFF
214       0.1        0.1       0.2   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: POST /ev-session creates a new EvSession
215       0.1        0.1       0.2   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: Heater is scheduled autonomously when below comfort floor (no HeaterTarget needed)
216       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: POST /ev-session creates an EV session
217       0.0        0.1       0.2   ⊘  VEN Simulator :: PV asset has expected fields in sim response
218       0.0        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: Zero timespan returns empty series
219       0.0        0.1       0.1   ⊘  Uniform-Grid Timeline API (RF-05c) :: GET /timeline/unknown_asset_xyz returns 404
220       0.1        0.1       0.1   ⊘  BFF VEN Access :: List VENs via BFF
221       0.1        0.1       0.1   ⊘  Heater tank MILP trajectory model :: Plan schedules heater when tank is below T_min
222       0.1        0.1       0.1   ⊘  Uniform-Grid Timeline API (RF-05c) :: GET /timeline/ev with resolution=30 returns 30-second spacing
223       0.1        0.1       0.1   ⊘  VEN Simulator :: Base load asset has expected fields in sim response
224       0.1        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/ev with hours_back=0 returns no past points
225       0.0        0.1       0.1   ⊘  VEN Sensor Data :: POST partial sensor data (temperature only)
226       0.0        0.1       0.1   ⊘  VEN Simulator :: Sensor values come from simulator
227       0.0        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: Future battery timeline points carry planner SoC forecast
228       0.0        0.1       0.1   ⊘  BFF VEN Access :: BFF health includes VTN status
229       0.0        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: Future EV timeline points carry planner SoC forecast
230       0.0        0.1       0.1   ⊘  VEN Sensor Data :: POST partial sensor data (power only)
231       0.0        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/ev with extended window returns more points
232       0.0        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/unknown_asset_xyz returns 404
233       0.0        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: Future heater timeline points carry planner T_tank forecast
----------------------------------------------------------------------------------------------------

SCENARIO STEPS:          4244.3s
AFTER-SCENARIO CLEANUP:    86.0s
BEFORE-FEATURE CLEANUP:    24.1s  (across 45 features)
TOTAL ACCOUNTED:         4354.3s
Scenarios: 233 | Avg total per scenario: 18.6s

Failing scenarios:
  features/ven_ui_raw_diagnostics.feature:21  Timeline cell shows series dropdown and refreshes
  features/ven_ui_raw_diagnostics.feature:31  Timeline dropdown filters series

42 features passed, 1 failed, 2 skipped
231 scenarios passed, 2 failed, 9 skipped
1320 steps passed, 2 failed, 109 skipped, 0 undefined
Took 69m26.821s

========================================================================
TEST TIMING SUMMARY
========================================================================
Rust unit tests:              build=4s  tests=1619s  container=1622s
VEN/ui vitest:                build=17s  tests=243s
VTN/ui vitest:                build=7s  tests=95s
BDD integration (wall):       4463s  accounted=4354.3s  scenarios=233
  Behave: 42 features passed, 1 failed, 2 skipped
------------------------------------------------------------------------
TOTAL WALL CLOCK:             6451s  (107m 31s)

FAILED TIERS: BDD




0ab6e1cba42e5595e6473358741dfa6bd97e762b
====================================================================================================
TEST TIMING SUMMARY — ALL SCENARIOS (slowest first)
====================================================================================================
  #   steps_s  cleanup_s   total_s  st  feature :: scenario
----------------------------------------------------------------------------------------------------
  1     326.9        0.3     327.2   ⊘  Planner Visualization Page :: Decision matrix collapses and expands [ven-ui]
  2     239.9        0.9     240.8   ⊘  Planner Visualization Page :: Clicking a matrix cell with a step opens the step detail drawer [ve
  3     193.7        0.2     193.9   ⊘  Shiftable Load Lifecycle (Plan B) :: Shiftable load auto-completes and disappears from GET /sim [
  4     178.0        1.0     179.0   ⊘  Planner Visualization Page :: Plan header shows trigger badge and summary values [ven-ui]
  5     127.2        0.2     127.4   ⊘  Shiftable Load Lifecycle (Plan B) :: Running shiftable load appears in GET /sim
  6     118.0        0.3     118.3   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-12a — Multi-asset plan with import cap allo
  7     105.6        1.2     106.9   ⊘  Planner Visualization Page :: Decision matrix renders asset rows and tariff header [ven-ui]
  8     100.9        0.3     101.2   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-12c — Ledger accumulates energy for all acty
  9      94.2        2.3      96.5   ⊘  Controller V2 — Simulation Controls :: Toggling EV plugged switch triggers a POST to sim override
 10      91.9        0.3      92.2   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-12b — Plan warnings are accessible when cap
 11      91.0        0.1      91.2   ⊘  Shiftable Load Lifecycle (Plan B) :: Deleting a running shiftable load removes it from GET /sim
 12      89.8        1.1      90.8   ⊘  Planner Visualization Page :: Clicking a trigger chip shows detail popover [ven-ui]
 13      84.5        1.6      86.0   ⊘  Controller V2 — Simulation Controls :: EV plugged toggle is visible in right section [ven-ui]
 14      80.8        0.2      80.9   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: Plan allocates EV to slots given a cheap PRIC
 15      80.6        0.1      80.7   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: EV session drives the planner to allocate EV
 16      77.8        0.3      78.1   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-10a — Plan slots reflect the import capacity limit from 
 17      75.6        0.2      75.8   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-01a — Explicit EV session is planned and allocate
 18      73.4        2.2      75.6   ⊘  Controller V2 — Navigation and Layout Controls :: Right section starts collapsed and can be expann-ui]
 19      75.4        0.2      75.6   ⊘  Shiftable Load Lifecycle (Plan B) :: Shiftable load appears in plan allocations after POST
 20      72.1        2.3      74.4   ⊘  Controller V2 — Simulation Controls :: EV SoC slider is visible in the Status Settings accordion 
 21      69.3        1.1      70.4   ⊘  Planner Visualization Page :: Trigger timeline shows at least one event chip [ven-ui]
 22      66.7        1.4      68.1   ⊘  Planner Visualization Page :: Trigger timeline section is visible on Planner page [ven-ui]
 23      66.8        1.1      67.9   ⊘  Controller V2 — Asset Cell Content :: Asset cell left section shows power, cost rate, and CO2eq r
 24      67.7        0.1      67.9   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-10b — Plan net_import_kw does not exceed the capacity li
 25      66.2        0.9      67.2   ⊘  Planner Visualization Page :: Decision matrix section is visible on Planner page [ven-ui]
 26      65.1        1.5      66.6   ⊘  Planner Visualization Page :: Plan header section is visible on Planner page [ven-ui]
 27      61.2        1.6      62.8   ⊘  Controller V2 — Asset Cell Content :: Asset cell mid section shows a NOW reference line [ven-ui]
 28      61.7        0.3      62.0   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-06b — Plan slots respect an import capacity limit
 29      61.3        0.3      61.6   ⊘  VEN User Request Manager — Stage 5 :: Interruptible scheduled EV session contributes to up_kw in 
 30      60.8        0.3      61.1   ⊘  VEN EV Charging Scenarios (Chunk 4) :: (b) IMPORT_CAPACITY_LIMIT event caps net import in plan sl
 31      58.3        1.6      59.9   ⊘  Planner Visualization Page :: Decision matrix collapse button is present [ven-ui]
 32      53.7        1.5      55.2   ⊘  Controller V2 — Navigation and Layout Controls :: Pinning a cell moves it to the pinned zone [ven
 33      53.3        1.1      54.5   ⊘  Controller V2 — Navigation and Layout Controls :: Unpinning a cell removes it from the pinned zon
 34      52.4        1.9      54.3   ⊘  Controller V2 — Page Layout :: Grid cells appear above asset cells by default [ven-ui]
 35      49.2        0.9      50.1   ⊘  Controller V2 — Navigation and Layout Controls :: Pin button is present on each cell [ven-ui]
 36      49.6        0.1      49.7   ⊘  Heater tank MILP trajectory model :: Plan uses only mid-tier heater (not full-tier) near T_max
 37      43.6        1.7      45.3   ⊘  Controller V2 — Asset Cell Content :: Global time range extend button is visible in the title bar
 38      43.4        0.1      43.6   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: Layer 2 triggers a DeviceDeviation rerid deviation
 39      41.0        1.8      42.7   ⊘  Controller V2 — Asset Cell Content :: Base load asset cell mid section shows a timeline chart [ve
 40      41.4        1.3      42.7   ⊘  Controller V2 — Asset Cell Content :: Battery asset cell mid section shows a timeline chart [ven-
 41      36.1        1.6      37.7   ⊘  Controller V2 — Page Layout :: Page is scrollable and grid cells are not fixed by default [ven-ui
 42      35.4        1.4      36.8   ⊘  Controller V2 — Page Layout :: At least one asset cell is present [ven-ui]
 43      34.5        1.6      36.1   ⊘  Controller V2 — Asset Cell Content :: Battery asset cell shows State of Charge [ven-ui]
 44      34.6        0.9      35.5   ⊘  Planner Visualization Page :: Navigate to Planner page [ven-ui]
 45      33.7        0.4      34.1   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC7 - Connectivity Check (open, round-trip) [ui]
 46      30.5        0.8      31.3   ⊘  VEN EV Charging Scenarios (Chunk 4) :: (c) Zero IMPORT_CAPACITY_LIMIT is reflected in plan slots
 47      30.1        0.1      30.2   ⊘  Asset Interface — history(timespan) :: Requesting more history than available returns partial res
 48      30.1        0.1      30.2   ⊘  Asset Interface — history(timespan) :: Battery 30-minute history returns samples
 49      30.1        0.1      30.2   ⊘  Asset Interface — history(timespan) :: PV history boundary point is present at start of window
 50      30.1        0.1      30.2   ⊘  Asset Interface — history(timespan) :: No future-timestamped entries in history response
 51      30.1        0.1      30.2   ⊘  Asset Interface — history(timespan) :: PV 30-minute history returns samples
 52      26.0        0.3      26.3   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC8 - Event Cancellation (delete via UI, VEN los
 53      25.4        0.2      25.6   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC6 - Battery Dispatch (targeted to VEN-1 only) 
 54      24.8        0.4      25.2   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC4 - Peak Shaving (targeted to VEN-1 and VEN-2)
 55      22.8        0.2      23.1   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC5 - EV Charging (targeted to VEN-2 with event-
 56      22.2        0.2      22.4   ⊘  Reporter multi-interval resampling (RF-05e) :: Obligation-based report contains multiple interval]
 57      20.4        0.1      20.5   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC2 - Export Limitation (targeted to VEN-2 only)
 58      19.5        0.4      19.8   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC3 - Dynamic Pricing (open to all VENs) [ui]
 59      19.1        0.5      19.6   ⊘  UI Use Cases — Full End-to-End via Browser :: UI-UC1 - Emergency Load Shed (targeted to VEN-1 onl
 60      18.5        0.5      19.1   ⊘  VEN Raw Data Diagnostics Page :: Timeline dropdown filters series [ven-ui]
 61      13.1        0.6      13.6   ⊘  VEN Raw Data Diagnostics Page :: Timeline cell shows series dropdown and refreshes [ven-ui]
 62      10.9        0.2      11.0   ⊘  Failure Recovery :: VEN recovers after its own restart [resilience]
 63      10.4        0.2      10.5   ⊘  Reporter multi-interval resampling (RF-05e) :: Timer-driven report without reportDescriptor is sir-resampling]
 64       9.7        0.6      10.3   ⊘  VEN Raw Data Diagnostics Page :: Tariffs cell refreshes on button click [ven-ui]
 65       8.5        0.2       8.7   ⊘  VEN Raw Data Diagnostics Page :: Sim cell refreshes on button click [ven-ui]
 66       8.2        0.5       8.6   ⊘  VEN Raw Data Diagnostics Page :: Each cell refreshes independently [ven-ui]
 67       7.9        0.4       8.3   ⊘  Planner Visualization Page :: Planner tab appears in navigation [ven-ui]
 68       7.2        0.3       7.5   ⊘  VEN Raw Data Diagnostics Page :: Page renders three diagnostic cells [ven-ui]
 69       6.5        0.2       6.7   ⊘  VEN Simulator :: Auto-report submitted for active event
 70       6.0        0.1       6.2   ⊘  Failure Recovery :: VEN re-syncs after VTN restart [resilience]
 71       3.2        2.7       5.9   ⊘  Failure Recovery :: VEN retains cached events when VTN goes down [resilience]
 72       5.4        0.1       5.5   ⊘  Failure Recovery :: Both VENs converge after VTN restart [resilience]
 73       5.2        0.2       5.4   ⊘  Phase A — Asset physics and capability coverage :: pv_irradiance override to zero silences PV out
 74       5.3        0.1       5.4   ⊘  Phase A — Asset physics and capability coverage :: pv_irradiance override to full produces nonzer
 75       4.4        0.1       4.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC3c - Price correction after initial publish
 76       4.3        0.1       4.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC8 - Event Cancellation (VEN-1 sees then loses event)
 77       3.5        0.1       3.6   ⊘  VEN Reports :: Submit report via VEN and verify round-trip
 78       3.3        0.2       3.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC4b - Modify peak shaving limit mid-flight
 79       2.5        0.2       2.6   ⊘  OpenADR Use Cases — Full End-to-End :: UC5 - EV Charging (targeted to VEN-2 with event-level targ
 80       2.3        0.3       2.6   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-06a — IMPORT_CAPACITY_LIMIT event updates /capaci
 81       2.4        0.2       2.6   ⊘  OpenADR Use Cases — Full End-to-End :: UC2 - Export Limitation (targeted to VEN-2 only)
 82       2.3        0.3       2.5   ⊘  VEN Program Enrollment :: Targeted program is visible only to enrolled VEN
 83       2.3        0.2       2.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC6b - Conflicting charge and discharge events
 84       2.3        0.2       2.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC5b - Overlapping EV events with different priorities
 85       2.2        0.2       2.5   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: IMPORT_CAPACITY_LIMIT event updates the capacity
 86       2.2        0.2       2.4   ⊘  Phase A — Asset physics and capability coverage :: ev_plugged false stops EV charging capability
 87       2.2        0.2       2.4   ⊘  Phase A — Asset physics and capability coverage :: EV unplugged reports zero capability in both d
 88       2.3        0.1       2.4   ⊘  OpenADR Use Cases — Full End-to-End :: UC3 - Dynamic Pricing (open to all VENs)
 89       2.1        0.2       2.3   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-08b — Re-plugging the EV resumes charging
 90       1.5        0.2       1.7   ⊘  OpenADR Use Cases — Full End-to-End :: UC6 - Battery Dispatch (targeted to VEN-1 only)
 91       1.4        0.2       1.6   ⊘  OpenADR Use Cases — Full End-to-End :: UC7 - Connectivity Check (open, no-op round-trip)
 92       1.3        0.2       1.5   ⊘  OpenADR Use Cases — Full End-to-End :: UC4 - Peak Shaving (targeted to VEN-1 and VEN-2)
 93       1.3        0.2       1.5   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: GET /obligations returns empty list when events ors
 94       1.3        0.2       1.4   ⊘  VEN Program Enrollment :: Open program is visible to all VENs
 95       1.1        0.2       1.4   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: GHG event produces rate snapshots with co2_g_kwh
 96       1.2        0.2       1.4   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-04a — PRICE event from VTN populates /tariffs wit
 97       1.1        0.2       1.4   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-03 — PV surplus accumulates in the asset ledger
 98       1.2        0.2       1.4   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: EXPORT_PRICE event produces rate snapshots with 
 99       1.2        0.2       1.3   ⊘  VEN-VTN Integration :: VEN reflects events created in VTN
100       1.1        0.2       1.3   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: Layer 1 corrects grid deviation immed
101       1.2        0.1       1.3   ⊘  OpenADR Use Cases — Full End-to-End :: UC3b - Day-ahead pricing with 24 hourly intervals
102       1.1        0.1       1.2   ⊘  VEN-VTN Integration :: VEN reflects programs created in VTN
103       0.3        0.9       1.2   ⊘  VTN Event Management :: List events returns the created event
104       0.9        0.2       1.1   ⊘  VEN Isolation :: VEN can only see its own reports
105       0.4        0.5       0.9   ⊘  VEN EV Charging Scenarios (Chunk 4) :: (f) User request with zero IMPORT_CAPACITY_LIMIT is reflec
106       0.8        0.2       0.9   ⊘  VEN Isolation :: VEN cannot retrieve another VEN's report by ID
107       0.6        0.2       0.8   ⊘  VEN Isolation :: VEN can only see its own VEN record
108       0.2        0.6       0.8   ⊘  VTN Event Active Filter :: active=true returns only current events
109       0.2        0.6       0.8   ⊘  VTN Event Active Filter :: no active filter returns all events
110       0.1        0.6       0.8   ⊘  VTN Event Management :: Create an event for a program
111       0.2        0.5       0.7   ⊘  VTN Event Active Filter :: active=false returns only past events
112       0.2        0.5       0.7   ⊘  VTN Program Management :: Create a program
113       0.1        0.6       0.7   ⊘  VTN Authentication :: Valid credentials return an access token
114       0.1        0.6       0.7   ⊘  VEN User Request Manager — Stage 5 :: Cancelling a user request clears the linked EV session
115       0.1        0.6       0.7   ⊘  VTN Authentication :: Invalid credentials are rejected
116       0.1        0.5       0.6   ⊘  VEN User Request Manager — Stage 5 :: Request for a non-storage asset is rejected
117       0.5        0.1       0.6   ⊘  Uniform-Grid Timeline API (RF-05c) :: Grid timestamps are snapped to round boundaries
118       0.3        0.2       0.6   ⊘  Uniform-Grid Timeline API (RF-05c) :: GET /timeline/all returns arrays of equal length for all as
119       0.3        0.3       0.6   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-09c — Cancelled request clears the linked EV session
120       0.1        0.4       0.5   ⊘  VTN Program Management :: List programs includes the created program
121       0.4        0.1       0.5   ⊘  Uniform-Grid Timeline API (RF-05c) :: Grid-portion timestamps are uniformly spaced
122       0.3        0.2       0.5   ⊘  Uniform-Grid Timeline API (RF-05c) :: Response format is unchanged
123       0.2        0.2       0.5   ⊘  BFF Event CRUD :: Delete an event via BFF
124       0.2        0.2       0.5   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-05c — Each flexibility envelope in /plan has energe fields
125       0.1        0.4       0.5   ⊘  VEN User Request Manager — Stage 5 :: GET /flexibility returns a site-level flexibility object
126       0.1        0.4       0.5   ⊘  VEN User Request Manager — Stage 5 :: Request with tolerance_min and interruptible stores leeway 
127       0.1        0.4       0.5   ⊘  VTN Program Management :: Unauthenticated request is rejected
128       0.2        0.3       0.5   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-01b— EV charge plan has FLEXIBLE envelopes for fa
129       0.2        0.2       0.4   ⊘  BFF Program CRUD :: Delete a program via BFF
130       0.3        0.2       0.4   ⊘  Uniform-Grid Timeline API (RF-05c) :: All assets share the same ts at each index position
131       0.3        0.1       0.4   ⊘  Uniform-Grid Timeline API (RF-05c) :: resolution=30 returns 30-second spacing
132       0.1        0.3       0.4   ⊘  VEN User Request Manager — Stage 5 :: POST /user-requests creates a user request with a linked EV
133       0.1        0.4       0.4   ⊘  VEN User Request Manager — Stage 5 :: Budget ceiling via budget_eur is reflected in user request
134       0.2        0.2       0.4   ⊘  Uniform-Grid Timeline API (RF-05c) :: Default auto-resolution targets approximately 300 points
135       0.1        0.3       0.4   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-02b — CONTINUE policy request appears in /user-re
136       0.1        0.3       0.4   ⊘  VEN User Request Manager — Stage 5 :: Multi-tier request has two deadline tiers in the linked pac
137       0.1        0.3       0.4   ⊘  VEN Entity Model — Stage 1 Foundation :: GET /sim includes battery field when battery is configur
138       0.1        0.3       0.4   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-11c — Asset ledger tracks energy in active 
139       0.1        0.3       0.4   ⊘  VEN User Request Manager — Stage 5 :: User request appears in GET /user-requests
140       0.1        0.3       0.4   ⊘  Uniform-Grid Timeline API (RF-05c) :: resolution takes precedence over max_points
141       0.1        0.2       0.4   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: EV session appears in /ev-session after POST
142       0.2        0.2       0.4   ⊘  Uniform-Grid Timeline API (RF-05c) :: The now-point ts is the same across all assets
143       0.1        0.3       0.4   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-02a — CONTINUE policy request has two deadline ti
144       0.2        0.2       0.4   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-04b — Plan after PRICE event has rate-priced slot
145       0.2        0.2       0.4   ⊘  VEN EV Charging Scenarios (Chunk 4) :: (e) User request capped by IMPORT_CAPACITY_LIMIT event
146       0.1        0.2       0.3   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-05d — GET /flexibility returns site-level headroophase-e]
147       0.1        0.2       0.3   ⊘  BFF Event CRUD :: Create an event via BFF
148       0.2        0.1       0.3   ⊘  OpenADR Use Cases — Full End-to-End :: UC1 - Emergency Load Shed (targeted to VEN-1 only)
149       0.1        0.3       0.3   ⊘  VEN Entity Model — Stage 1 Foundation :: Battery power_kw is present in sim response
150       0.1        0.3       0.3   ⊘  VEN Entity Model — Stage 1 Foundation :: Existing /sim endpoint returns structured ts + grid + as
151       0.1        0.3       0.3   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-05a — Plan has slots covering the planning horizo
152       0.1        0.2       0.3   ⊘  Uniform-Grid Timeline API (RF-05c) :: Each asset array contains a now-point between history and f
153       0.2        0.1       0.3   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: Plan has flexibility envelopes for far-horizo
154       0.1        0.2       0.3   ⊘  VEN Entity Model — Stage 1 Foundation :: GET /tariffs returns a JSON array
155       0.1        0.2       0.3   ⊘  VEN User Request Manager — Stage 5 :: User request with budget constraint includes max_total_cost
156       0.1        0.2       0.3   ⊘  Phase A — Asset physics and capability coverage :: PV always reports fixed (non-curtailable) capa
157       0.1        0.2       0.3   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: DELETE /heater-target clears the target
158       0.1        0.2       0.3   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-09b — Tight budget tier-1 request still creates a valid 
159       0.1        0.2       0.3   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-11b — Plan runs without crashing when no pa
160       0.1        0.1       0.3   ⊘  Uniform-Grid Timeline API (RF-05c) :: max_points=150 produces equivalent resolution
161       0.1        0.2       0.3   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: EV session drives the planner to allocate EV 
162       0.1        0.2       0.3   ⊘  UC-01..UC-04 — Normal Operation Use Cases :: UC-01c — EV packet estimated cost is tracked in the 
163       0.1        0.2       0.3   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: EV session drives dispatcher to alloc
164       0.1        0.2       0.3   ⊘  VEN-VTN Integration :: VEN generates sensor data automatically
165       0.1        0.2       0.3   ⊘  VEN Simulator :: Sim endpoint shows configured devices
166       0.1        0.2       0.3   ⊘  Asset Interface — forecast(timespan) :: Battery forecast returns power series with linear interpo
167       0.1        0.2       0.3   ⊘  VEN Entity Model — Stage 1 Foundation :: Existing /health endpoint still works
168       0.1        0.2       0.3   ⊘  VEN Entity Model — Stage 1 Foundation :: GET /trace/events endpoint returns a JSON array
169       0.1        0.2       0.3   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: 3-interval PRICE event produces 3 rate snapshots
170       0.1        0.2       0.3   ⊘  VEN Simulator :: Sim endpoint top-level shape is ts + grid + assets
171       0.1        0.2       0.3   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/ev returns a sorted JSON array
172       0.1        0.2       0.3   ⊘  UC-11..UC-12 — Stress and Multi-Asset Use Cases :: UC-11a — Planner produces a valid plan even wis
173       0.1        0.2       0.3   ⊘  Uniform-Grid Timeline API (RF-05c) :: GET /timeline/ev returns uniformly spaced ts with now-point
174       0.1        0.1       0.3   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: GET /ledger returns per-asset energy rging
175       0.1        0.1       0.3   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-08a — Unplugging the EV via override drops EV current to
176       0.1        0.2       0.3   ⊘  UC-08..UC-10 — Edge Case Use Cases :: UC-09a — Multi-tier request with tight tier-1 budget has ti
177       0.1        0.2       0.2   ⊘  Asset Interface — forecast(timespan) :: Heater forecast returns linear-interpolated power series
178       0.1        0.1       0.2   ⊘  BFF Program CRUD :: Update a program via BFF
179       0.1        0.1       0.2   ⊘  Uniform-Grid Timeline API (RF-05c) :: Empty future buckets have values null
180       0.1        0.2       0.2   ⊘  VEN Entity Model — Stage 1 Foundation :: Battery SoC stays within valid range over time
181       0.1        0.1       0.2   ⊘  Heater tank MILP trajectory model :: Cheap PRICE event attracts heater into cheap tariff window
182       0.1        0.2       0.2   ⊘  VEN Rate System — OpenADR Interface (Stage 2) :: GET /capacity returns a JSON object with expecte
183       0.1        0.1       0.2   ⊘  Shiftable Load Lifecycle (Plan B) :: POST rejects duplicate asset_id with 409
184       0.1        0.1       0.2   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/all returns all configured assets and grid
185       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: GET /ev-session returns the active session a
186       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: POST /shiftable-loads adds a shiftable load
187       0.1        0.2       0.2   ⊘  VEN Entity Model — Stage 1 Foundation :: GET /trace/history returns asset history rows for EV
188       0.1        0.1       0.2   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: Plan slots cover the planning horizon
189       0.1        0.2       0.2   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/grid returns a sorted array
190       0.1        0.1       0.2   ⊘  Phase A — Asset physics and capability coverage :: Battery at full SoC reports zero import capabi
191       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: POST /heater-target creates a heater target
192       0.1        0.2       0.2   ⊘  VEN Simulator :: Sim grid object has expected fields
193       0.1        0.2       0.2   ⊘  VEN Simulator :: EV asset has expected fields in sim response
194       0.1        0.1       0.2   ⊘  VEN Entity Model — Stage 1 Foundation :: Existing /events endpoint still works
195       0.1        0.1       0.2   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: GET /plan returns a non-null plan after VEN s
196       0.0        0.2       0.2   ⊘  VEN Simulator :: Heater sim schema exposes all four controls
197       0.1        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: EV charger forecast returns step-interpolated power serie
198       0.1        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: Base load forecast returns constant step-interpolated pow
199       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: DELETE /shiftable-loads/{id} removes the loa
200       0.1        0.2       0.2   ⊘  VEN Simulator :: Battery asset has expected fields in sim response
201       0.0        0.2       0.2   ⊘  UC-05..UC-07 — VTN Coordination Use Cases :: UC-07 — GET /capacity returns a valid capacity state
202       0.1        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: PV forecast during daytime returns non-empty power series
203       0.1        0.1       0.2   ⊘  Phase A — Asset physics and capability coverage :: Battery at empty SoC reports zero export capab
204       0.1        0.1       0.2   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: EvSession appears in GET /ev-session 
205       0.0        0.2       0.2   ⊘  VEN Simulator :: Heater asset has expected fields in sim response
206       0.1        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: PV forecast boundary point is present at end of timespan
207       0.1        0.1       0.2   ⊘  BFF Reports :: List reports via BFF
208       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: DELETE /ev-session removes the session
209       0.0        0.1       0.2   ⊘  VEN Health Check :: Health endpoint returns ok
210       0.1        0.1       0.2   ⊘  VEN Sensor Data :: POST sensor data and GET it back
211       0.1        0.1       0.2   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/ev response points have ts and values fields
212       0.1        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: Heater forecast has non-zero average power
213       0.0        0.1       0.2   ⊘  BFF Program CRUD :: Create a program via BFF
214       0.1        0.1       0.2   ⊘  VEN Dispatcher — Stage 4 (Plan Execution + Asset Ledger) :: POST /ev-session creates a new EvSess
215       0.1        0.1       0.2   ⊘  VEN Planner — Stage 3 (EnergyPacket + Algorithm) :: Heater is scheduled autonomously when below crTarget needed)
216       0.1        0.1       0.2   ⊘  Device Sessions API — EV, Heater, Shiftable Loads :: POST /ev-session creates an EV session
217       0.0        0.1       0.2   ⊘  VEN Simulator :: PV asset has expected fields in sim response
218       0.0        0.1       0.2   ⊘  Asset Interface — forecast(timespan) :: Zero timespan returns empty series
219       0.0        0.1       0.1   ⊘  Uniform-Grid Timeline API (RF-05c) :: GET /timeline/unknown_asset_xyz returns 404
220       0.1        0.1       0.1   ⊘  BFF VEN Access :: List VENs via BFF
221       0.1        0.1       0.1   ⊘  Heater tank MILP trajectory model :: Plan schedules heater when tank is below T_min
222       0.1        0.1       0.1   ⊘  Uniform-Grid Timeline API (RF-05c) :: GET /timeline/ev with resolution=30 returns 30-second spaci
223       0.1        0.1       0.1   ⊘  VEN Simulator :: Base load asset has expected fields in sim response
224       0.1        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/ev with hours_back=0 returns no past points
225       0.0        0.1       0.1   ⊘  VEN Sensor Data :: POST partial sensor data (temperature only)
226       0.0        0.1       0.1   ⊘  VEN Simulator :: Sensor values come from simulator
227       0.0        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: Future battery timeline points carry planner SoC forecast
228       0.0        0.1       0.1   ⊘  BFF VEN Access :: BFF health includes VTN status
229       0.0        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: Future EV timeline points carry planner SoC forecast
230       0.0        0.1       0.1   ⊘  VEN Sensor Data :: POST partial sensor data (power only)
231       0.0        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/ev with extended window returns more points
232       0.0        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: GET /timeline/unknown_asset_xyz returns 404
233       0.0        0.1       0.1   ⊘  VEN Asset Timeline Endpoints :: Future heater timeline points carry planner T_tank forecast
----------------------------------------------------------------------------------------------------

SCENARIO STEPS:          4244.3s
AFTER-SCENARIO CLEANUP:    86.0s
BEFORE-FEATURE CLEANUP:    24.1s  (across 45 features)
TOTAL ACCOUNTED:         4354.3s
Scenarios: 233 | Avg total per scenario: 18.6s

Failing scenarios:
  features/ven_ui_raw_diagnostics.feature:21  Timeline cell shows series dropdown and refreshes
  features/ven_ui_raw_diagnostics.feature:31  Timeline dropdown filters series

42 features passed, 1 failed, 2 skipped
231 scenarios passed, 2 failed, 9 skipped
1320 steps passed, 2 failed, 109 skipped, 0 undefined
Took 69m26.821s

========================================================================
TEST TIMING SUMMARY
========================================================================
Rust unit tests:              build=4s  tests=1619s  container=1622s
VEN/ui vitest:                build=17s  tests=243s
VTN/ui vitest:                build=7s  tests=95s
BDD integration (wall):       4463s  accounted=4354.3s  scenarios=233
  Behave: 42 features passed, 1 failed, 2 skipped
------------------------------------------------------------------------
TOTAL WALL CLOCK:             6451s  (107m 31s)

FAILED TIERS: BDD
a5049ff1604281193bb4a853c05dda0fd646b6c0
PS C:\DriveD\Tinker\OpenAdr-Lab> ssh Pi4-Server "tail -f /tmp/run-all-tests.log"
========================================================================
Rust unit tests:              build=3s  tests=1531s  container=1534s
VEN/ui vitest:                build=54s  tests=218s
VTN/ui vitest:                build=4s  tests=93s
BDD integration (wall):       4444s  accounted=4259.5s  scenarios=233
  Behave: 42 features passed, 1 failed, 2 skipped
------------------------------------------------------------------------
TOTAL WALL CLOCK:             6350s  (105m 50s)

FAILED TIERS: BDD