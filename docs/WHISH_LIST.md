clean up docker orphans

make the ven-1 id a uuid and change it in all test and seed references.

DB-level optimization for active event filter: add `ends_at timestamptz` computed column + index so the `?active=true` filter can run in SQL instead of post-filtering in Rust. Not needed until event tables grow large.


As a user I should be able to influence the siumulation setpoints of the simulated devices like user desired EV-Chager setpoint (overridden by events) and irradiance level.

create a separate tab in the VEN UI to graphically show the traces in a curve diagram, showing the values of the traces graphically.

Add a filter in VTN UI event table to omit the past events.

Add a DB-Reset script so it can be re-seeded easily.


add a setup script that docker composes all required containers.