# Tasks

Operators, chart editing, the plan feed (`fleet-plan-feed`, now documented in
`docs/architecture/VTN_ARCHITECTURE.md`) and the before/after panel are implemented and tested (UI
unit tests; `fleet_plan_feed.feature` on Node2). What remains:

- [ ] 1. Measure the retained `plan` message and the broker's memory with 20 VENs on Node2 (open question 1 of the former `fleet-plan-feed` design) and record the numbers in the journal. A day of 5-minute slots with five assets is a unit-tested size (`a_days_plan_body_needs_the_raised_packet_limit_and_fits_it` asserts it is over the client's 10 KiB default and under a quarter of `MAX_PACKET_BYTES`, 4 MiB); the real bytes and the broker's heap are not measured, and the plan feed is not deployed on Node1 yet, so there is no table to read them from.
