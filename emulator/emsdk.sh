#!/usr/bin/env bash
# Installs the pinned emsdk into emulator/.cache/emsdk
# (https://emscripten.org/docs/getting_started/downloads.html).
# Also provides wasm-opt (Binaryen) for the Bevy client build.
set -euo pipefail

EMSDK_VERSION="6.0.10"
EMSDK_DIR="$(cd "$(dirname "$0")" && pwd)/.cache/emsdk"

if [ ! -d "$EMSDK_DIR" ]; then
  git clone https://github.com/emscripten-core/emsdk.git "$EMSDK_DIR"
fi
"$EMSDK_DIR/emsdk" install "$EMSDK_VERSION"
"$EMSDK_DIR/emsdk" activate "$EMSDK_VERSION"
