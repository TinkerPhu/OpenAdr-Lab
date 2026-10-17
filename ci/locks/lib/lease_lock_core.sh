#!/usr/bin/env bash
# lease_lock_core.sh — the one implementation of the lease lock's semantics.
#
# Runs ON the machine that owns the lock (piped there by docker_host_lock.sh over
# ssh, by wsl_lock.sh into WSL), so every check-and-act is atomic on that machine:
# mkdir is the mutex, the owner file is metadata. Never run it from the caller's
# side against a remote path — the atomicity is the whole point.
#
# Args: op owner lease_min description lock_path
#   op: try_acquire | release | refresh | status
op="$1"; owner="$2"; lease_min="$3"; desc="$4"; lock="$5"
now=$(date +%s)
expiry=$(( now + lease_min * 60 ))
# Written via a temp file and renamed: a plain `>` truncates in place, so a
# concurrent reader can see an empty or half-written file. rename(2) is atomic
# within one filesystem, so a reader sees the old owner or the new one, never a
# partial record.
write_owner() { printf '%s\n%s\n%s\n' "$owner" "$expiry" "$desc" > "$lock/owner.$$" && mv -f "$lock/owner.$$" "$lock/owner"; }
# True only for an expiry we actually parsed. "Could not read it" is not the
# same answer as "it expired", and conflating the two is what let two acquirers
# both believe they held the lock.
valid_expiry() { case "${cur_expiry:-}" in ''|*[!0-9]*) return 1 ;; *) return 0 ;; esac; }
read_owner()  { cur_owner=$(sed -n 1p "$lock/owner" 2>/dev/null)
                cur_expiry=$(sed -n 2p "$lock/owner" 2>/dev/null)
                cur_desc=$(sed -n 3p "$lock/owner" 2>/dev/null)
                left_min=$(( (${cur_expiry:-0} - now) / 60 ))
                expiry_utc=$(date -u -d "@${cur_expiry:-0}" +'%Y-%m-%d %H:%MZ' 2>/dev/null || echo '?'); }
held_msg() { echo "$1 by $cur_owner: ${cur_desc:-no description} (lease ends $expiry_utc, in ${left_min}min)"; }
case "$op" in
  try_acquire)
    if mkdir "$lock" 2>/dev/null; then write_owner; echo "ACQUIRED (lease ${lease_min}min)"; exit 0; fi
    read_owner
    if [ "$cur_owner" = "$owner" ]; then write_owner; echo "ACQUIRED (re-entrant, lease renewed ${lease_min}min)"; exit 0; fi
    if valid_expiry; then
        if [ "$now" -ge "$cur_expiry" ]; then
            write_owner
            echo "ACQUIRED (stole dead lock from ${cur_owner:-unknown}, lease expired $(( -left_min ))min ago: ${cur_desc:-?})"
            exit 0
        fi
        held_msg "HELD"; exit 1
    fi
    # Owner missing or unparsable. Either the winner of the mkdir is still writing
    # it (microseconds) or it died between the mkdir and the write (forever).
    # Waiting is right for the first and wrong for the second, so the lock's own
    # age decides; anything younger than the grace period is a claim in flight.
    lock_age=$(( now - $(stat -c %Y "$lock" 2>/dev/null || echo "$now") ))
    if [ "$lock_age" -ge 120 ]; then
        write_owner
        echo "ACQUIRED (stole a lock whose owner was never written, ${lock_age}s old)"
        exit 0
    fi
    echo "HELD by an acquirer still writing its claim (${lock_age}s old) - rerun"; exit 1 ;;
  release)
    [ -d "$lock" ] || { echo "NOT LOCKED"; exit 0; }
    read_owner
    if [ "$cur_owner" = "$owner" ]; then rm -rf "$lock"; echo "RELEASED"; exit 0; fi
    held_msg "NOT OWNER — held"; exit 1 ;;
  refresh)
    [ -d "$lock" ] || { echo "NOT LOCKED — nothing to refresh"; exit 1; }
    read_owner
    if [ "$cur_owner" = "$owner" ]; then desc="$cur_desc"; write_owner; echo "REFRESHED (lease ${lease_min}min from now)"; exit 0; fi
    held_msg "NOT OWNER — held"; exit 1 ;;
  status)
    [ -d "$lock" ] || { echo "FREE"; exit 0; }
    read_owner
    if ! valid_expiry; then
        echo "HELD (owner unreadable - an acquirer mid-write, or one that died before writing)"
    elif [ "$now" -ge "$cur_expiry" ]; then
        echo "DEAD (stealable) — was $cur_owner: ${cur_desc:-no description} (lease expired $(( -left_min ))min ago)"
    else
        held_msg "HELD"
    fi
    exit 0 ;;
  *) echo "lease_lock_core: unknown op '$op'"; exit 64 ;;
esac
