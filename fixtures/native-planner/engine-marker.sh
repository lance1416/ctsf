#!/bin/sh
# Copied into disposable QA data, never launched by the harness.
printf '%s\n' 'unexpected native planner engine execution' > "$0.executed"
exit 97
