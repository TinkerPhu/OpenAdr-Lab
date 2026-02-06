Feature: VEN Sensor Data
  Sensor data can be posted and retrieved from the VEN.

  Scenario: POST sensor data and GET it back
    Given I post sensor data with temperature 22.5 and power 150.0
    When I GET the VEN sensor snapshot
    Then the sensor temperature is 22.5
    And the sensor power is 150.0
