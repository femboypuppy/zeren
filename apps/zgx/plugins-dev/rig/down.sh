#!/bin/sh
# Stops the rig's engines only: the PIDs it tracks under $ZEREN_RIG_STATE,
# and only while they are still zeren-engine processes.
exec node "$(dirname "$0")/rig.mjs" down "$@"
