#!/bin/bash
# Build the Rust mobile core (crates/mobile) for the active Xcode platform and
# generate its Swift bindings. Run by the Zeren target's "Rust core" build
# phase; also runnable by hand (defaults to the simulator, dev profile).
#
#   scripts/ios/build-core.sh [iphonesimulator|iphoneos]
#
# Outputs (target/ios-core/<platform>/):
#   libzeren_mobile.a            linked via LIBRARY_SEARCH_PATHS
#   include/module.modulemap     `import zeren_coreFFI` (SWIFT_INCLUDE_PATHS)
# and refreshes apps/ios/Zeren/Core/Generated/zeren_core.swift — committed so
# Xcode's synchronized folder always sees it — and apps/ios/CoreFFI/ (the C
# header and module map, committed so CI's simulator tests need no bindings
# build; outside the synchronized folders, so Xcode compiles nothing from it).
# CI fails if any of them drifts.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PLATFORM="${1:-${PLATFORM_NAME:-iphonesimulator}}"
case "$PLATFORM" in
  iphonesimulator)
    case "$(uname -m)" in
      x86_64) TARGET=x86_64-apple-ios ;;
      arm64) TARGET=aarch64-apple-ios-sim ;;
      *) echo "error: unsupported simulator host architecture" >&2; exit 1 ;;
    esac
    ;;
  iphoneos) TARGET=aarch64-apple-ios ;;
  *) echo "error: unsupported platform $PLATFORM" >&2; exit 1 ;;
esac
PROFILE=mobile
if [[ "${CONFIGURATION:-Debug}" == "Release" ]]; then PROFILE=mobile-dist; fi

OUT="$ROOT/target/ios-core/$PLATFORM"
mkdir -p "$OUT/include"

# Iterating on Swift while the core is mid-edit: reuse the last good build.
if [[ "${ZEREN_SKIP_CORE:-}" == "1" && -f "$OUT/libzeren_mobile.a" ]]; then
  echo "note: ZEREN_SKIP_CORE=1 — reusing $OUT/libzeren_mobile.a"
  exit 0
fi

# Xcode exports SDKROOT/deployment vars for the *app* SDK; host build scripts
# (proc macros, build.rs) must not see them, so cargo runs in a clean env.
# Keep whichever cargo the caller's PATH finds, and rustup's homes: rustup
# isn't always in ~/.cargo/bin (a CI runner image moved it).
CARGO_DIRS="$HOME/.cargo/bin"
if FOUND="$(command -v cargo)"; then CARGO_DIRS="$(dirname "$FOUND"):$CARGO_DIRS"; fi
KEEP=()
for name in CARGO_HOME RUSTUP_HOME; do
  if [[ -n "${!name:-}" ]]; then KEEP+=("$name=${!name}"); fi
done
run_cargo() {
  env -i HOME="$HOME" USER="${USER:-}" TERM="${TERM:-dumb}" \
    ${KEEP[@]+"${KEEP[@]}"} \
    PATH="$CARGO_DIRS:/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    IPHONEOS_DEPLOYMENT_TARGET=26.0 \
    CARGO_TARGET_DIR="$ROOT/target" \
    cargo "$@"
}

cd "$ROOT"
run_cargo build --locked -p zeren-mobile --lib --profile "$PROFILE" --target "$TARGET"
run_cargo build --locked -p zeren-mobile --bin uniffi-bindgen --features bindgen --profile mobile

LIB="$ROOT/target/$TARGET/$PROFILE/libzeren_mobile.a"
cp -p "$LIB" "$OUT/libzeren_mobile.a"

GEN="$OUT/gen"
"$ROOT/target/mobile/uniffi-bindgen" generate --library "$LIB" --language swift --out-dir "$GEN" >/dev/null
cp "$GEN/zeren_coreFFI.h" "$OUT/include/zeren_coreFFI.h"
cp "$GEN/zeren_coreFFI.modulemap" "$OUT/include/module.modulemap"
# Only touch the committed files when they changed so Xcode doesn't recompile them.
refresh() { mkdir -p "$(dirname "$2")"; cmp -s "$1" "$2" || cp "$1" "$2"; }
refresh "$GEN/zeren_core.swift" "$ROOT/apps/ios/Zeren/Core/Generated/zeren_core.swift"
refresh "$GEN/zeren_coreFFI.h" "$ROOT/apps/ios/CoreFFI/zeren_coreFFI.h"
refresh "$GEN/zeren_coreFFI.modulemap" "$ROOT/apps/ios/CoreFFI/module.modulemap"
