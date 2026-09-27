#!/usr/bin/env bash
# Builds per-system FBNeo libretro cores as standalone Emscripten modules in
# emulator/dist/<core>/. They are served from R2 (`make emulator` uploads them to local R2).
#
#   1. Installs the pinned emsdk (emsdk.sh)
#   2. Checks out a pinned libretro/FBNeo commit into emulator/.cache/FBNeo
#   3. Builds each core with FBNeo's own Makefile (platform=emscripten), as documented in
#      src/burner/libretro/README.md, keeping only that core's drivers
#   4. Links it into fbneo.mjs + fbneo.wasm, exporting the libretro API (exports.json) so
#      our own frontend can drive retro_run / retro_serialize directly
#
# ROM sets must match the pinned FBNeo commit: bump FBNEO_COMMIT and the ROMs together.
set -euo pipefail

FBNEO_REPO="https://github.com/libretro/FBNeo.git"
FBNEO_COMMIT="aceeebed9e7edc8a28652365a064baee9a16e274"

# One core per system, so a cabinet downloads ~3 MB instead of the full ~10 MB core.
# "name|regex": the regex picks the driver files (under src/burn/drv) the core keeps.
CORES=(
  "neogeo|/drv/neogeo/"                   # Metal Slug and the rest of the Neo Geo library
  "midway|/drv/midway/"                   # Mortal Kombat 1-3, UMK3, NBA Jam, ...
  "snowbros|/drv/pst90s/d_hyperpac\.cpp$" # Snow Bros 1-3, Hyper Pacman, ...
  "capcom|/drv/capcom/"                   # CPS1/CPS2: Marvel vs. Capcom, Street Fighter II, ...
  "konami|/drv/konami/"                   # Sunset Riders, TMNT, The Simpsons, ...
)
# FBNeo's libretro frontend always references PGM2 and the Neo Geo machine (not its games),
# so every core keeps those files.
ALWAYS_KEEP='/drv/pgm2/|/drv/neogeo/(neo_|neogeo\.cpp)'

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="$ROOT/emulator/.cache"
EMSDK_DIR="$CACHE/emsdk"
FBNEO_DIR="$CACHE/FBNeo"
LIBRETRO_DIR="$FBNEO_DIR/src/burner/libretro"
OUT_DIR="$ROOT/emulator/dist"
JOBS="${JOBS:-$(sysctl -n hw.ncpu 2>/dev/null || nproc)}"

mkdir -p "$CACHE"

# 1. emsdk
"$ROOT/emulator/emsdk.sh"
# shellcheck disable=SC1091
source "$EMSDK_DIR/emsdk_env.sh"

# 2. FBNeo sources at the pinned commit
if [ ! -d "$FBNEO_DIR/.git" ]; then
  git init -q "$FBNEO_DIR"
  git -C "$FBNEO_DIR" remote add origin "$FBNEO_REPO"
fi
if [ "$(git -C "$FBNEO_DIR" rev-parse HEAD 2>/dev/null)" != "$FBNEO_COMMIT" ]; then
  git -C "$FBNEO_DIR" fetch --depth 1 origin "$FBNEO_COMMIT"
  git -C "$FBNEO_DIR" checkout -q FETCH_HEAD
fi

# The emscripten platform sets STATIC_LINKING=1, which leaves out the libretro-common
# helpers RetroArch normally provides (Makefile.common). We link without RetroArch, so
# build that same list ourselves.
COMMON_DIR="$LIBRETRO_DIR/libretro-common"
COMMON_OBJ_DIR="$CACHE/libretro-common"
mkdir -p "$COMMON_OBJ_DIR"
COMMON_OBJS=()
for src in \
  file/file_path.c file/file_path_io.c file/retro_dirent.c encodings/encoding_utf.c \
  compat/compat_posix_string.c compat/compat_strcasestr.c compat/compat_strl.c \
  compat/compat_strldup.c compat/fopen_utf8.c string/stdstring.c streams/file_stream.c \
  streams/file_stream_transforms.c features/features_cpu.c file/config_file.c \
  file/config_file_userdata.c lists/string_list.c memmap/memalign.c time/rtime.c \
  vfs/vfs_implementation.c; do
  obj="$COMMON_OBJ_DIR/${src//\//_}.o"
  emcc -O3 -D__LIBRETRO__ -I"$COMMON_DIR/include" -c "$COMMON_DIR/$src" -o "$obj"
  COMMON_OBJS+=("$obj")
done

rm -rf "$OUT_DIR"
for core in "${CORES[@]}"; do
  name="${core%%|*}"
  keep="${core#*|}"

  # 3. Core archive. BURN_BLACKLIST (read by Makefile.all) drops every other driver, and
  #    generate-files rebuilds the driver list to match. The archive is removed first:
  #    `ar rcs` only adds members, so objects from the previous core would linger.
  BURN_BLACKLIST="$(cd "$LIBRETRO_DIR" && find ../../burn/drv -mindepth 2 \( -name '*.c' -o -name '*.cpp' \) |
    grep -vE "$ALWAYS_KEEP|$keep" | tr '\n' ' ')"
  export BURN_BLACKLIST
  emmake make -C "$LIBRETRO_DIR" platform=emscripten generate-files
  rm -f "$LIBRETRO_DIR/fbneo_libretro_emscripten.bc"
  emmake make -C "$LIBRETRO_DIR" platform=emscripten -j"$JOBS"
  # fbneo_libretro_emscripten.bc is an `ar` archive despite the name.
  cp "$LIBRETRO_DIR/fbneo_libretro_emscripten.bc" "$CACHE/$name.a"

  # 4. Standalone ES module. Memory/stack sizes follow RetroArch's Makefile.emscripten.
  mkdir -p "$OUT_DIR/$name"
  em++ "$CACHE/$name.a" "${COMMON_OBJS[@]}" -O3 -o "$OUT_DIR/$name/fbneo.mjs" \
    -sMODULARIZE=1 -sEXPORT_ES6=1 -sEXPORT_NAME=createFBNeo \
    -sENVIRONMENT=web,worker,node \
    -sINITIAL_MEMORY=134217728 -sALLOW_MEMORY_GROWTH=1 -sSTACK_SIZE=4194304 \
    -sALLOW_TABLE_GROWTH=1 -sFORCE_FILESYSTEM=1 -sUSE_ZLIB=1 \
    -sEXPORTED_FUNCTIONS=@"$ROOT/emulator/exports.json" \
    -sEXPORTED_RUNTIME_METHODS=FS,addFunction,removeFunction,UTF8ToString,stringToUTF8,lengthBytesUTF8,getValue,setValue,HEAPU8,HEAP16,HEAPU16,HEAP32,HEAPU32
done
unset BURN_BLACKLIST

# Put FBNeo's generated driver list back to the pinned commit's version.
git -C "$FBNEO_DIR" checkout -- gamelist.txt src/dep/generated/driverlist.h

echo "FBNeo ($FBNEO_COMMIT) built with $(emcc --version | head -1):"
ls -lh "$OUT_DIR"/*/
