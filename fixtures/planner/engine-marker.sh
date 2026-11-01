#!/bin/sh
# The acceptance runner copies this into its temporary data tree. Never invoke it.
printf '%s\n' 'unexpected engine execution' > "$0.executed"
exit 97
