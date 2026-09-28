# Arcade Bar

The page shows the bar from `assets/maps/bar.ron` (made with `make editor`) and a placeholder
player: arrows or WASD walk, and floor tiles without an object are walkable. The FBNeo emulator
(`web/emulator/`, `client/src/emulator.rs`) and the games in R2 are kept for cabinets, but the
page doesn't start them yet.

| Path | What | Built with |
| --- | --- | --- |
| `client/` | Bevy app, mounted on `<canvas id="bevy">` | `cargo` + `wasm-bindgen` → `web/pkg/` |
| `server/` | Worker + `Room` Durable Object (WebSocket Hibernation) | `workers-rs` template, `wrangler` |
| `emulator/` | Per-system FBNeo libretro cores as Emscripten ES modules | emsdk + FBNeo's Makefile → `emulator/dist/<core>/` |
| `world/` | Map format, isometric grid math, tile drawing (shared by the editor and, later, the client) | |
| `tools/editor/` | Bar layout editor, desktop only (`make editor`) | `cargo`, `bevy_egui` |
| `assets/` | Tile art (`tiles/`) and maps (`maps/`) | |
| `web/` | Static assets: `index.html`, Bevy's `pkg/`, the emulator worker + libretro frontend in `emulator/` | |

Routes: static files from `web/`, `GET /ws/:room` (WebSocket to that room's Durable Object), `GET /fbneo/<core>/fbneo.{mjs,wasm}` (FBNeo cores) and `GET /roms/<file>` (ROM sets), both from R2.

## Setup

Needs `rustup`, Node/npm, `git` and `make`. The Rust toolchain (`rust-toolchain.toml`), emsdk and
`wasm-bindgen-cli` are pinned and installed into the project on first use.

```sh
(cd server && npm install)
make emulator   # FBNeo cores -> emulator/dist, uploaded to local R2. First run takes a while.
# Each ROM set and BIOS set (neogeo.zip for Neo Geo games), zipped and named as FBNeo expects:
make upload-rom ROM=$HOME/Downloads/mk2.zip
# Optional start-up state per game: skips boot screens, inserts 9 coins. Redo after rebuilding cores.
node emulator/snapshot.mjs emulator/dist/midway/fbneo.mjs $HOME/Downloads/mk2.zip emulator/dist/mk2.state
make upload-rom ROM=emulator/dist/mk2.state
make dev        # builds the client, serves everything at http://localhost:8787
```

`make client PROFILE=dev` skips the size optimizations for faster iteration.

## Dev tools

`make editor` opens the bar layout editor, a desktop app that is never part of the web build.

- The palette is every PNG in `assets/tiles/floor/` and `assets/tiles/objects/`. Floor tiles are
  32×16 diamonds drawn centered on their cell. Objects are 32 px wide and any height: the bottom
  point of the image sits on the bottom point of the cell's diamond.
- Left click paints, right click erases, scroll / arrows / WASD pan, `+` / `-` zoom,
  Cmd+S saves `assets/maps/bar.ron`.
- Cabinets get the ROM set typed in "Cabinet game" (e.g. `mk2`).
- Images reload when their files change, so you can edit art in a pixel-art app with the editor
  open. The current tiles are placeholders.

## Deploy

```sh
cd server && npx wrangler r2 bucket create vab && cd ..   # once
make emulator-remote                                      # cores
make upload-rom R2_TARGET=--remote ROM=$HOME/Downloads/mk2.zip   # each ROM, BIOS and .state
make deploy
```

## Pinned versions

| | Version | Where |
| --- | --- | --- |
| Rust | 1.98.1 | `rust-toolchain.toml` |
| Bevy | 0.19.1 | `client/Cargo.toml` |
| workers-rs | 0.8 | `server/Cargo.toml` |
| Emscripten | 6.0.10 | `emulator/emsdk.sh` |
| FBNeo | `aceeebed` (libretro/FBNeo) | `emulator/build.sh` |

ROM sets must match the FBNeo commit; bump them together. FBNeo's license is non-commercial.

## Sizes

Players download each file once (compressed sizes; Workers static assets cap files at 25 MiB):

| File | Raw | gzip |
| --- | --- | --- |
| Bevy client (lean features, logs below `warn` compiled out, `wasm-opt -Oz`) | ~13.8 MiB | ~4.6 MB |
| One FBNeo core (neogeo, midway, snowbros, capcom, konami, classics) | ~5–6 MiB | ~3–3.3 MB |

A cabinet loads only its system's core. To add a system, add a line to `CORES` in
`emulator/build.sh` (driver files live under `src/burn/drv` in the FBNeo checkout).
