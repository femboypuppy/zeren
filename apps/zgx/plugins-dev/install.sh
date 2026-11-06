#!/bin/sh
# Copies example plugins into a Zeren data dir's plugins folder and signs
# them as that engine's account (`zeren plugin sign`), for a scratch engine:
#
#   ./install.sh <data-dir> <port> [plugin ...]     (default: all five)
#
# The engine must be running (its IPC on <port>, its token in <data-dir>).
# Never the production engine: port 27654 and ~/.comet-native are refused.
# `focus` is meant to be signed by another account: install it on that
# account's engine, then copy its signed folder over (see rig/).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
data=${1:?usage: install.sh <data-dir> <port> [plugin ...]}
port=${2:?usage: install.sh <data-dir> <port> [plugin ...]}
shift 2
case "$port" in 27654) echo "refusing the production engine's port" >&2; exit 1 ;; esac
case "$(cd "$data" && pwd)" in "$HOME/.comet-native"*) echo "refusing the production engine's data" >&2; exit 1 ;; esac
[ $# -gt 0 ] || set -- compact-sidebar focus notes-tool ping session-status
zeren=${ZEREN_BIN:-$root/target/debug/zeren}
mkdir -p "$data/plugins"
for id in "$@"; do
  rm -rf "$data/plugins/$id"
  cp -R "$here/$id" "$data/plugins/$id"
  cp "$root/apps/zgx/plugins-sdk/zeren.luau" "$data/plugins/$id/zeren.luau"
done
sleep 1
for id in "$@"; do
  env -u ZEREN_IPC_TOKEN ZEREN_IPC_PORT="$port" ZEREN_DATA_DIR="$data" ZEREN_IPC_TOKEN_FILE="$data/ipc-token" "$zeren" plugin sign "$id"
done
env -u ZEREN_IPC_TOKEN ZEREN_IPC_PORT="$port" ZEREN_DATA_DIR="$data" ZEREN_IPC_TOKEN_FILE="$data/ipc-token" "$zeren" plugin list
