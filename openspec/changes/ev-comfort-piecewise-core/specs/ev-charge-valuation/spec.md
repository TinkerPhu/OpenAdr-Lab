# Spec Delta

## Purpose

How an EV charge request's energy is valued and delivered: the comfort curve as a per-kWh bid
over state of charge, the difference between a guarantee and a preference, and what the planner
must do when a user's bid covers only part of the energy they asked for.

## ADDED Requirements

### Requirement: Comfort bids are per kWh over state of charge

An EV comfort curve SHALL be read as a marginal bid: at each `fill` level, the most the user
will pay for the next kWh. For an EV, `fill` SHALL mean state of charge, where 0 is empty and 1
is full — not progress through a task. The curve SHALL value every kWh from the EV's current
state of charge up to full.

#### Scenario: A bid is read at the state of charge it applies to
- **WHEN** a curve bids 0.40 €/kWh at fill 0.2 and 0.10 €/kWh at fill 0.9, and the EV is at 50 %
- **THEN** the energy that takes it from 50 % toward 90 % is valued between those two bids by
  interpolation, and energy above 90 % is valued at 0.10 €/kWh

#### Scenario: A single-point curve values all energy equally
- **WHEN** a curve has one point
- **THEN** every kWh from the current state of charge to full is valued at that one bid

### Requirement: Charging degrades gracefully when a bid covers only part of the energy

For a request without a firm deadline, the planner SHALL deliver every kWh whose bid at its own
state-of-charge level is worth at least its cost, and SHALL NOT refuse the whole request because
the bid fails to cover all of it. Delivering nothing SHALL occur only when no kWh is worth its
cost.

#### Scenario: A low bid buys a partial charge instead of nothing
- **WHEN** a soft request's bid covers the cost of some but not all of the energy needed to
  reach the user's target
- **THEN** the plan charges the part that is covered, and the EV's state of charge rises by that
  amount

#### Scenario: A bid below the cost of all available energy buys nothing
- **WHEN** every kWh available in the window costs more than the user bids for it
- **THEN** the plan charges nothing, and the reason is reported as the bid being below the cost
  of energy rather than as an unmet obligation

#### Scenario: Cheap energy is taken even under a low bid
- **WHEN** a soft request carries a low bid and part of the window offers surplus or
  near-zero-cost energy
- **THEN** the plan charges during that part of the window

### Requirement: A target is a guarantee only with a firm deadline

A request with a firm deadline SHALL deliver at least the energy needed to reach its target
state of charge by that deadline, regardless of the comfort bid. A request with a soft deadline
SHALL treat its target as a preference expressed through the bid, not as an obligation.

#### Scenario: A firm deadline charges regardless of the bid
- **WHEN** a request has a firm deadline, a target above the current state of charge, and a bid
  below the cost of energy
- **THEN** the plan still delivers the energy needed to reach the target by the deadline

#### Scenario: A firm target that cannot be reached charges as far as possible and says so
- **WHEN** the window before a firm deadline cannot physically deliver the energy needed
- **THEN** the plan delivers as much as the window allows, remains solvable, and reports the
  shortfall with the delivered and required amounts

#### Scenario: A soft target is not an obligation
- **WHEN** a request has a soft deadline and its bid stops being worth the cost partway to the
  target
- **THEN** the plan stops charging there without reporting an unmet obligation

### Requirement: A comfort curve's bids must not rise with fill

The system SHALL reject a comfort curve whose bid increases as fill increases, naming the
offending point. A user's willingness to pay for the next kWh SHALL be non-increasing as the
battery fills.

#### Scenario: A rising curve is refused
- **WHEN** a curve bids 0.10 €/kWh at fill 0 and 0.40 €/kWh at fill 1 is submitted
- **THEN** it is rejected with an error naming the point whose bid rises, and the previously
  stored curve is unchanged

#### Scenario: A flat or falling curve is accepted
- **WHEN** a curve's bids stay equal or fall as fill increases
- **THEN** it is accepted

### Requirement: Energy the user did not pay for is not invented

Charging SHALL remain bounded by what the asset and the site can physically do — the charger's
power limits, the times the vehicle is present, and any site import limit — independently of
what the comfort curve offers.

#### Scenario: A generous bid cannot exceed physical limits
- **WHEN** a curve bids far above any plausible tariff
- **THEN** the plan still respects the charger's maximum power, the vehicle's availability, and
  the site's import limit
