#!/usr/bin/env bash
# The Linux Rust tests that used to run as separate `cargo test` invocations in
# session-sync-regressions, cursor-compatibility and preview-tests, as ONE
# build and ONE nextest run (separate invocations recompiled the same
# dependency graph with different feature sets).
#
# Same coverage as the old invocations:
#   zeren-harness  every test binary (cursor-compatibility ran `-p zeren-harness`
#                  plus `--test pi_rpc` with native-fixture; native-fixture only
#                  adds the pi_rpc fixture binaries, so it is on for the whole build)
#   zeren-preview  every test binary (preview-tests)
#   zeren-doc      lib + attachments_roundtrip (cursor-compatibility)
#   zeren-sync, zeren-update   lib only (session-sync-regressions)
#   zeren-engine   lib + the integration tests named below. Engine has many other
#                  integration binaries (live agents etc.) that no workflow ran, so
#                  they are listed instead of globbed to avoid compiling them.
# nextest does not run doctests; these crates have none (their doc comments
# contain no Rust code blocks).
# Extra arguments are passed to nextest (the nightly job adds `--release`).
set -euo pipefail
cd "$(dirname "$0")/../.."

tests=()
for f in crates/harness/tests/*.rs crates/preview/tests/*.rs; do
  tests+=(--test "$(basename "$f" .rs)")
done
for t in session_publication restart_resume codex_subagents local_profiles message_queue pi_resume attachments_roundtrip; do
  tests+=(--test "$t")
done

exec cargo nextest run --config-file scripts/ci/nextest.toml --locked --no-fail-fast \
  -p zeren-harness -p zeren-engine -p zeren-sync -p zeren-update -p zeren-doc -p zeren-preview \
  --features zeren-harness/native-fixture \
  --lib --bins "${tests[@]}" "$@"
