# Arcade Bar

The page runs arcade games full screen (FBNeo in a Web Worker, drawn by Bevy): Mortal Kombat II,
Metal Slug, Snow Bros., Marvel vs. Capcom, Sunset Riders, Street Fighter II′ CE, Pac-Man, Tetris
and Wonder Boy. The list is `GAMES` in `web/index.html`.

Controls: `[` / `]` previous / next game, arrows move, `5` coin, `1` start, buttons on `A S D` and
`Z X C` (MK II: high punch / high kick / block, low punch / low kick / block). Sound starts on the
first key press or click.

| Path | What | Built with |
| --- | --- | --- |
| `client/` | Bevy app, mounted on `<canvas id="bevy">` | `cargo` + `wasm-bindgen` → `web/pkg/` |
| `server/` | Worker + `Room` Durable Object (WebSocket Hibernation) | `workers-rs` template, `wrangler` |
| `emulator/` | Per-system FBNeo libretro cores as Emscripten ES modules | emsdk + FBNeo's Makefile → `emulator/dist/<core>/` |
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
