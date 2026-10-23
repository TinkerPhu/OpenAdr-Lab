Feature: What the owners are left short of (service level, UC-19)
  Every asset declares, on its snapshot, the energy its owner is missing against what the asset's
  own rules say is due (`unserved_energy_kwh`) and the energy its store holds
  (`stored_energy_kwh`). An EV owes nothing while it is plugged in; the moment it leaves below its
  target it owes the gap, for the whole trip. A battery declares no unserved energy: its
  minimum state of charge is a limit it holds itself. The fleet report adds the increases of
  these figures, so an objective that serves less is visible beside the energy it saves.

  Scenario: An EV that leaves below its target reports the missing energy
    Given the site has no PV, the EV plugged at 0.3 SoC, the heater in its band and the battery at 0.1 SoC
    And the EV is plugged in on the snapshot
    When the EV leaves
    Then within 20 seconds the "ev" reports more than 1 kWh unserved

  Scenario: An EV that leaves at its target reports none
    Given the site has no PV, the EV plugged at 1 SoC, the heater in its band and the battery at 0.1 SoC
    And the EV is plugged in on the snapshot
    When the EV leaves
    Then within 20 seconds the "ev" reports no unserved energy

  Scenario: The battery stores energy and declares no unserved energy
    Given the site has no PV, the EV plugged at 0.3 SoC, the heater in its band and the battery at 0.5 SoC
    Then within 20 seconds the "battery" reports stored energy
    And the "battery" declares no unserved energy
