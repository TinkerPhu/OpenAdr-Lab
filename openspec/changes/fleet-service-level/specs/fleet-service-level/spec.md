## ADDED Requirements

### Requirement: An asset declares the energy its owner is missing
An asset that serves an owner's need SHALL declare, on every tick, its unserved energy: the energy in kWh,
electric-equivalent, that its owner is missing against what the asset's own rules say is due now. The value is
zero when nothing is due or the need is met. The declaration SHALL be the asset's own, and no other component
SHALL compute it from raw asset values. An asset that serves no owner need SHALL declare none, and "none" is
not the same as zero.

#### Scenario: An EV leaves short of its target
- **WHEN** an EV with a 60 kWh pack and an 80 % target leaves at 55 % state of charge
- **THEN** from the tick it leaves, its unserved energy is 15 kWh and it stays 15 kWh until the vehicle is plugged in again

#### Scenario: An EV leaves at its target
- **WHEN** the same EV leaves at 80 % or more
- **THEN** its unserved energy is 0 kWh

#### Scenario: A plugged-in EV below target is not yet short
- **WHEN** the EV is plugged in at 55 % and has not left
- **THEN** its unserved energy is 0 kWh, because nothing is due until it leaves

#### Scenario: A heater below its comfort floor
- **WHEN** a heater whose tank holds 0.233 kWh per degree sits 4 degrees below its comfort floor
- **THEN** its unserved energy is the energy to lift the tank to the floor (0.93 kWh for a resistive element) while it stays below, and 0 kWh once it is back at or above the floor

#### Scenario: A shiftable load past its deadline
- **WHEN** a shiftable load still owes 0.8 kWh of its run when its deadline passes
- **THEN** its unserved energy is 0.8 kWh until the job is finished or removed

#### Scenario: The battery declares none
- **WHEN** a battery is at its reserve floor
- **THEN** it declares no unserved energy, because the floor is a limit the battery holds itself and no owner need is missed

### Requirement: An asset declares the energy it stores
An asset that stores energy SHALL declare, on every tick, its stored energy in kWh, electric-equivalent, with the
reference point stated by the asset's own definition. The battery and the EV declare their state of charge times
their capacity (the EV also while away). The heater declares the energy in its tank above its comfort floor. An
asset that stores no energy declares none.

#### Scenario: A battery
- **WHEN** a 10 kWh battery is at 60 %
- **THEN** its stored energy is 6 kWh

#### Scenario: An EV away
- **WHEN** a 60 kWh EV is away at 40 % state of charge
- **THEN** its stored energy is 24 kWh

#### Scenario: A heater tank
- **WHEN** a heater whose tank holds 0.233 kWh per degree is 6 degrees above its comfort floor
- **THEN** its stored energy is 1.4 kWh

### Requirement: The declarations are visible where the asset is
Both declarations SHALL be part of the asset's snapshot, so they appear in the VEN UI's view of the asset and in
the fleet telemetry each VEN publishes, under names that carry their unit.

#### Scenario: On the VEN
- **WHEN** a VEN's `/sim` snapshot is read
- **THEN** each asset that declares them carries `unserved_energy_kwh` and `stored_energy_kwh`, and each asset that declares none omits them

#### Scenario: In the VEN UI
- **WHEN** the Devices page shows an EV that left 15 kWh short
- **THEN** its card shows the unserved energy and the stored energy with their units

### Requirement: The fleet report accumulates unserved energy and carry-over from stored telemetry
For a window, the fleet report SHALL give, per VEN and per asset and summed for the fleet: the unserved energy
accrued in the window and the carry-over. Unserved energy accrued is the sum of the increases of the declared
unserved energy between consecutive stored samples (a decrease adds nothing). Carry-over is the declared stored
energy at the last sample of the window minus that at the first. The report SHALL read stored telemetry, not
plans, and SHALL state how many VENs contribute to each total. It SHALL NOT treat a missing sample or a missing
declaration as zero.

#### Scenario: Two departures
- **WHEN** an EV's declared unserved energy reads 0, then 15 kWh for the length of a trip, then 0 after it is plugged in, then 6 kWh for a second trip
- **THEN** the unserved energy accrued over the window is 21 kWh

#### Scenario: A lost sample does not lose a departure
- **WHEN** the samples between 0 kWh and 15 kWh are missing from storage
- **THEN** the increase is still counted, from the neighbouring samples

#### Scenario: Carry-over
- **WHEN** a fleet's stored energy over battery, EV and tank is 120 kWh at the first sample and 96 kWh at the last
- **THEN** the carry-over is -24 kWh

#### Scenario: A VEN that stopped publishing
- **WHEN** one of 20 VENs has no sample in the window
- **THEN** the totals are built from 19 VENs and say so

#### Scenario: A window older than the raw telemetry
- **WHEN** the window begins before the retention of the raw telemetry
- **THEN** the report says the figure is unavailable and gives no number

### Requirement: The Fleet page shows the service delivered beside the plan comparison
The Fleet page SHALL show, for the latest objective switch, the unserved energy accrued and the carry-over before
and after it, over windows of equal length, so a comparison of planned energy and cost carries the service level
next to it. A VEN or asset behind a total SHALL be reachable from the card.

#### Scenario: The first opportunistic day
- **WHEN** ten EVs charged nothing after a switch that the window before charged 80 kWh for
- **THEN** the card shows the unserved energy after the switch higher than before and the carry-over lower, beside the plan comparison

#### Scenario: Equal windows
- **WHEN** the switch was 16 hours ago
- **THEN** the after window is the 16 hours since the switch and the before window the 16 hours preceding it

#### Scenario: Drill-down
- **WHEN** a user opens a VEN on the card
- **THEN** its assets are listed with each asset's unserved energy and carry-over for both windows

### Requirement: A new asset kind joins by declaring the same two quantities
An asset kind added later (heat pump, air conditioner) SHALL take part in the report and the card by declaring
unserved energy and stored energy in electric-equivalent kWh, without a change to the report or the card.

#### Scenario: A kind the report has never seen
- **WHEN** a test asset of a new kind declares unserved energy and stored energy
- **THEN** the report includes it in the totals and the drill-down with no code that names its kind
