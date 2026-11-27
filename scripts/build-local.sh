#!/usr/bin/env bash
# Local release build with the verifier models (`semantic`) and the release
# features. On macOS the ONNX runtime's CoreML objects need the compiler
# runtime (`___isPlatformVersionAtLeast`), which rustc does not link by
# default, so it is added for the final binary only.
#
#   scripts/build-local.sh            # -> target/release/reasoning-engine
#   FEATURES=local-host,trust,tasks scripts/build-local.sh
set -euo pipefail
cd "$(dirname "$0")/.."

FEATURES="${FEATURES:-local-host,trust,tasks,semantic,fulltext,graph,learn,fa,reuse}"
LINK_ARGS=()
if [ "$(uname -s)" = "Darwin" ]; then
  rt_dir="$(dirname "$(xcrun --find clang)")/../lib/clang"
  rt="$(find "$rt_dir" -name libclang_rt.osx.a 2>/dev/null | head -1 || true)"
  if [ -n "$rt" ]; then
    LINK_ARGS=(-C "link-arg=-L$(dirname "$rt")" -C "link-arg=-lclang_rt.osx")
  fi
fi

cargo rustc --release --locked -p re-cli --bin reasoning-engine --features "$FEATURES" -- "${LINK_ARGS[@]}"
ls -l target/release/reasoning-engine
