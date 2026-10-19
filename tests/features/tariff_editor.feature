Feature: Tariff editor — a tariff that repeats forever
  The VTN UI's tariff editor publishes a day of prices once and lets it repeat, using the
  spec's own control (event.duration = P9999Y). The BFF resolves a draft with the same
  interval rule the VEN plans against, and declares what a price means on the wire.

  Background:
    Given I have a VTN token as "bl-client"

  Scenario: The wire profile declares what a price means
    When I request the signal registry from the BFF
    Then the signal registry declares "PRICE" in "KWH" and "EUR"
    And the signal registry declares "EXPORT_PRICE" in "KWH" and "EUR"

  Scenario: A draft that repeats forever previews as repeating
    When I preview a 24-hour tariff that repeats forever over 48 hours through the BFF
    Then the preview holds 48 bands
    And the second day of the preview repeats the first
    And the preview is not truncated

  Scenario: A tariff published by the editor is in force on the second day
    Given I create a rate-system program and save its ID
    And the tariff editor publishes a daily PRICE pattern that repeats forever
    When I wait for the VEN /tariffs endpoint to show the pattern's price on the second day
    Then the VEN /tariffs price on the second day equals the pattern's price at that hour
    And the VEN /tariffs price on the second day differs from the one an hour earlier

  Scenario: The editor loads a repeating tariff as its pattern, not an expansion
    Given I create a rate-system program and save its ID
    And the tariff editor publishes a daily PRICE pattern that repeats forever
    When I ask the BFF for the pattern of that event
    Then the pattern holds 24 intervals
    And the pattern's prices are the ones published

  # The operator's own path, in a browser: the pointer edits the draft (handles drawn over the
  # chart), Publish sends it, and the fleet's plans answer. DraftEditLayer.test.tsx proves the
  # pointer-to-value mapping with hand-made geometry; only a real chart shows that a handle can
  # be grabbed and dragged, and only a real publish shows the event wins.
  @ui
  Scenario: A price dragged up on the chart and published changes what the fleet plans
    Given I create a rate-system program and save its ID
    And the tariff editor publishes a daily PRICE pattern that repeats forever
    When I wait for the VEN /tariffs endpoint to show the pattern's price on the second day
    And I open the tariff editor on that event
    And I drag a price handle on the chart to the top of the plot
    Then exactly one grid price changed, to the value the drag readout showed
    When I publish the draft from the editor
    Then the VTN holds a second event for the program carrying the dragged price
    When I wait for the VEN /tariffs endpoint to show the dragged price
    Then the fleet plan holds a plan adopted after the editor's publish that the plan of a second before it did not
