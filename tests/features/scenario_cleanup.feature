@cleanup
Feature: Every scenario leaves the VEN as it found it (GB-35)
  State one scenario sets and leaves behind decides what the next one sees: a battery SoC, a
  comfort curve or an inject left set made later scenarios fail by time of day and by order, not
  by their own behaviour. `features/helpers/scenario_state.py` records what each scenario changes
  through the VEN API and puts it back after the scenario. These two scenarios run in file order:
  the first changes three kinds of state, the second finds each as the first one found it.

  Scenario: A scenario changes the battery SoC, a comfort curve and a sim inject
    Given the VEN is running with profile "test"
    And I remember the VEN state a scenario can change
    And the battery SoC is reset to 0.93
    When I set a comfort curve for asset "ev" with points "0.5:0.40,0.9:0.25,1.0:0.10"
    And I POST a sim override setting ev_plugged to false
    Then the comfort curve for asset "ev" reports source "override"

  Scenario: The next scenario finds the VEN as the previous one found it
    Given the VEN is running with profile "test"
    Then the VEN state a scenario can change is back to what was remembered
