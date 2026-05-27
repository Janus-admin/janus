#!/usr/bin/env bash
# sample-proc.sh — sample CPU% and RSS of a process every second.
#
# Usage:   ./sample-proc.sh <pid> <output_csv>
# Output:  a CSV with columns: timestamp_iso,cpu_pct,rss_kb
#
# Stops automatically when the target process disappears, or when the script
# is killed (SIGTERM/SIGINT). Designed to be launched in the background by
# run.sh and killed at the end of the measurement window.
#
# Why a custom sampler? Tools like `pidstat` would work but aren't installed
# everywhere (especially macOS). `ps` is universal and good enough at 1 Hz.

set -euo pipefail

PID="${1:?usage: sample-proc.sh <pid> <output_csv>}"
OUT="${2:?usage: sample-proc.sh <pid> <output_csv>}"

# Header — overwrite any existing file so a stale sampler can't poison results.
printf 'timestamp_iso,cpu_pct,rss_kb\n' > "$OUT"

# Trap so the file is flushed if we get killed mid-write.
trap 'exit 0' TERM INT

while kill -0 "$PID" 2>/dev/null; do
    # `ps -o %cpu,rss` formats are portable between macOS and Linux.
    # %cpu is normalised to a single CPU (so 200% means 2 cores fully loaded).
    # rss is in kilobytes.
    if line=$(ps -o %cpu=,rss= -p "$PID" 2>/dev/null); then
        # Squeeze whitespace and parse.
        line=$(echo "$line" | tr -s ' ' | sed 's/^ //')
        cpu="${line%% *}"
        rss="${line##* }"
        ts=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
        printf '%s,%s,%s\n' "$ts" "$cpu" "$rss" >> "$OUT"
    fi
    sleep 1
done
