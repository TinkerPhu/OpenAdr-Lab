clean up docker orphans

ven-1 differs in naming scheme from othe VENs. this causes confusion and sometimes errors. can we unify them?

make the ven-1 id a uuid and change it in all test and seed references.

DB-level optimization for active event filter: add `ends_at timestamptz` computed column + index so the `?active=true` filter can run in SQL instead of post-filtering in Rust. Not needed until event tables grow large.


Add a filter in VTN UI event table to omit the past events.

Add a DB-Reset script so it can be re-seeded easily.


add a setup script that docker composes all required containers.


add code coverage tools to tests and formater and linter tools to be applied for each code change.


check and remove warnings in all builds.

check for code quality and refactoring possibilities.

write down all your findings to the test errors around VEN UI simulation tests into ven_ui_simulation_test_issues.md. 

The fix is there. Docker's layer cache is stale — it doesn't see the change to Simulation.tsx. Need to force a rebuild without cache


add time provider for simulation: 
pub trait TimeContext: Clone + Send + Sync + 'static {
    type Instant: Copy + Ord + Send + 'static;

    fn now(&self) -> Self::Instant;
    fn sleep_until(&self, deadline: Self::Instant) -> Pin<Box<dyn Future<Output = ()> + Send>>;
    fn sleep(&self, duration: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>>;

    fn pause(&self);
    fn resume(&self);
    fn set_rate(&self, rate: f64);
    fn advance(&self, delta: Duration);
}


how can I test the ven controller in ui?


also add ui tests for UserRequests and Controller in VEN\ui\src\__tests__   


the ven poll interval should be configurable in the config file so during test we can easily shorten it. or is there a better option? 

can we fix this issue? - **Windows SSH PATH issue** — Git Bash SSH (`C:\Program Files\Git\usr\bin\ssh.exe`) takes PATH precedence over Windows OpenSSH and cannot find `C:\Users\<user>\.ssh\config`. Use full path `"C:/Windows/System32/OpenSSH/ssh.exe"` in Claude Code Bash commands when SSH connections fail silently.