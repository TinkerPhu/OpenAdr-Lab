# retry_failed.sh — flaky versus red for the behave passes in entrypoint.sh (sourced).
#
# retry_failed_scenarios <pass> <pass exit code> <rerun file> [behave args...]
# The rerun file is behave's `rerun` formatter output: one failed feature:line per line. Each
# failed scenario is run once more on its own. Returns 0 when the pass failed and every
# scenario passed on the retry (the pass is then not failed); 1 otherwise (nothing to
# retry, or a scenario failed again: red). A scenario that passed on retry is named in a
# `FLAKY_SCENARIO: <location>` line, which ci/runner.sh lists in result.md.
retry_failed_scenarios() {
  local pass="$1" rc="$2" file="$3" location still_failing=0
  shift 3
  [ "$rc" -ne 0 ] && [ -s "$file" ] || return 1
  echo ""
  echo "=== Retrying the failed $pass scenarios once, each on its own ==="
  while IFS= read -r location; do
    case "$location" in ''|'#'*) continue ;; esac   # behave's rerun header and blank lines
    if python -m behave -f pretty "$location" "$@"; then
      echo "FLAKY_SCENARIO: $location"
    else
      still_failing=1
    fi
  done < "$file"
  [ "$still_failing" -eq 0 ]
}
