# Tasks — what is left of `opportunistic-objective`

Everything else is implemented, tested and described in the current-state docs:
`docs/architecture/VEN_ARCHITECTURE.md` ("The opportunistic objective: dispatch without a plan",
§2.3.3), `docs/use-cases/HEMS-USE-CASE-OBSERVATION-MANUAL.md` UC-19,
`docs/use-cases/SYSTEM-USE-CASES.md` §13, `docs/plans/fleet-monitor/phase-0-foundation.md` §6.2,
`docs/reference/WIRE_PROFILE.md` v3. Open debts it left: R-145..R-149.

- [ ] 8.0 Deploy `VTN/mosquitto/fleet-acl.conf` (the BFF may write
      `openadr-lab/fleet/+/command/objective`, that leaf only) to the Node1 broker and run
      `scripts/test_fleet_acl.sh` against it: the BFF's command reaches the VEN, a VEN cannot
      command another. Needs the user's go-ahead (live broker). Until then a fleet command from
      the VTN UI is dropped by the broker's ACL in production; nothing else is affected. The
      probes cannot run in CI (anonymous test broker, GB-52).
- [ ] 10.3 After merge and deploy: switch the Node2 fleet to opportunistic from the VTN UI for
      one day and back; check the fleet forecast and its `before` sum against what the fleet did;
      record in `docs/history/project_journal.md`.
