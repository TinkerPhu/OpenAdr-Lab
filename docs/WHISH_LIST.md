clean up docker orphans
make the ven-1 id a uuid and change it in all test and seed references.
instantiate ven-3 (it is offline in the VEN UI)
DB-level optimization for active event filter: add `ends_at timestamptz` computed column + index so the `?active=true` filter can run in SQL instead of post-filtering in Rust. Not needed until event tables grow large.
VEN polling with active filter: change VEN's `/events` poll to use `?active=true` to reduce traffic and exclude completed events automatically.
As a user I should be able to influence the siumulation setpoints of the simulated devices like user desired EV-Chager setpoint (overridden by events) and irradiance level.
create a separate tab in the VEN UI to graphically show the traces in a curve diagram, showing the values of the traces graphically.
Add a filter in VTN UI event table to omit the past events.
