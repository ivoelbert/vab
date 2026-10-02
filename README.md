# Arcade Bar

The page shows the bar from `assets/maps/bar.ron` (made with `make editor`) and a placeholder
player: arrows or WASD walk, and floor tiles without an object are walkable. Everyone on the page
is in the same bar room and sees the others walk around (`?room=<name>` opens a separate one).
E next to a cabinet sits you at it: you start its game, or join the one being played there
(see [Online play](#online-play)). F watches the game being played there, as does E once every
seat is taken (see [Watching](#watching)). Esc stands up. Y opens the chat for everyone in the room;
`/name <name>` there sets the name shown above your head, and a cookie keeps it. The controls
show on a first visit and with `/help`; when a game starts, a card lists its buttons as the game
names them (the core reports them, e.g. "Z  Low Punch").

On a phone or tablet (a screen whose main pointer is a finger) the controls are on the screen
instead (`client/src/touch.rs`): a thumb dragged anywhere walks, Play and Watch show by a cabinet,
and at one a d-pad sits under the left thumb (fixed in place, so a move's sequence can be tapped
or rolled through) and the game's buttons under the right, named as the game names them, with
Coin, Start and Leave. The chat is typed in the page's own box, since
a canvas can't bring up a phone's keyboard. Chrome's device toolbar shows all of it on a computer,
as long as its pixel ratio is left at the computer's own (an emulated one gets the canvas size wrong).

| Path | What | Built with |
| --- | --- | --- |
| `client/` | Bevy app, mounted on `<canvas id="bevy">`; its text font (Fira Mono cut to Latin-1, OFL) is in `fonts/` | `cargo` + `wasm-bindgen` → `web/pkg/` |
| `server/` | Worker + `Room` Durable Object (WebSocket Hibernation) | `workers-rs` template, `wrangler` |
| `netplay/` | Rollback for two players at a cabinet (GGRS), run by the emulator worker | `cargo` + `wasm-bindgen` → `web/netplay/` |
| `emulator/` | Per-system FBNeo libretro cores as Emscripten ES modules | emsdk + FBNeo's Makefile → `emulator/dist/<core>/` |
| `world/` | Map format, isometric grid math, tile drawing (shared by the editor and, later, the client) | |
| `tools/editor/` | Bar layout editor, desktop only (`make editor`) | `cargo`, `bevy_egui` |
| `assets/` | Tile art (`tiles/`) and maps (`maps/`) | |
| `web/` | Static assets: `index.html`, the room connection (`room.js`), Bevy's `pkg/`, the emulator worker + libretro frontend in `emulator/` | |

Routes: static files from `web/`, `GET /ws/:room` (WebSocket to that room's Durable Object), `GET /ice` (WebRTC servers), `GET /fbneo/<core>/fbneo.{mjs,wasm}` (FBNeo cores) and `GET /roms/<file>` (ROM sets), both from R2.

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

Merging to `main` deploys the site (`.github/workflows/ci.yml`): the Worker, its Room Durable
Object and everything in `web/`. Pull requests get the same build and checks without deploying.
The workflow needs two repository secrets: `CLOUDFLARE_API_TOKEN` (a token made from the "Edit
Cloudflare Workers" template) and `CLOUDFLARE_ACCOUNT_ID`.

Cores, ROMs and start-up states live in R2 and go up by hand, when they change:

```sh
cd server && npx wrangler r2 bucket create vab && cd ..   # once
make emulator-remote                                      # cores
make upload-rom R2_TARGET=--remote ROM=$HOME/Downloads/mk2.zip   # each ROM, BIOS and .state
make deploy                                               # or merge to main
```

A preview Worker, `vab-preview`, runs the same site with its own rooms and its own R2 bucket, for
trying changes on several computers before they reach the site:

```sh
cd server && npx wrangler r2 bucket create vab-preview && cd ..   # once
make emulator-remote R2_BUCKET=vab-preview
make upload-rom R2_TARGET=--remote R2_BUCKET=vab-preview ROM=$HOME/Downloads/mk2.zip   # each
make preview    # https://vab-preview.<account>.workers.dev
```

TURN (optional, per Worker): create a TURN key in the Cloudflare dashboard (Realtime → TURN
Server), then `npx wrangler secret put TURN_KEY_ID` and `npx wrangler secret put
TURN_KEY_API_TOKEN` in `server/` (add `--env preview` for the preview). Without it, players whose
browsers can't connect directly play through the room instead, which is slower.

## Pinned versions

| | Version | Where |
| --- | --- | --- |
| Rust | 1.98.1 | `rust-toolchain.toml` |
| Bevy | 0.19.1 | `client/Cargo.toml` |
| workers-rs | 0.8 | `server/Cargo.toml` |
| Emscripten | 6.0.10 | `emulator/emsdk.sh` |
| FBNeo | `aceeebed` (libretro/FBNeo) + `emulator/patches/` | `emulator/build.sh` |

ROM sets must match the FBNeo commit; bump them together. FBNeo's license is non-commercial.

## Rollback netplay

Online play re-runs frames from save states, so both players' machines must end up identical.
`emulator/patches/` adds state FBNeo keeps outside its save states (the 4-way joystick's last
direction, the YM2151's render position). After rebuilding the cores or bumping FBNeo, check each
game (a rollback on every frame, compared with playing straight through on another instance):

```sh
node emulator/rollback-check.mjs emulator/dist/midway/fbneo.mjs $HOME/Downloads/mk2.zip emulator/dist/mk2.state
```

## Online play

Players take a cabinet's free seats in order: as many as its game takes (`players` in
`assets/games.ron`, 2 unless said, up to 4 as in Sunset Riders), each on their seat's controls.
The first plays alone right away. Whoever sits down later joins that game as it is: the lowest
seat among those playing captures its machine and hands it to everyone through the room, and all
of them start a new GGRS session (`netplay/`) from it. Someone leaving works the same way. Each
machine guesses the others' input and re-runs frames when the real one arrives; the worker picks
the rollback limit from how fast the machine runs the game (Mortal Kombat II gets 3 frames and 3
frames of input delay on an M-series Mac, the rest 8 and 2). GGRS compares a hash of the game's
RAM every 60 frames and reports any desync.

Game packets go through the room's WebSocket at first and straight between the browsers over
WebRTC once that connects (`web/room.js`). Turn-based games (`turns`) use player 1's controls for
both players, like an upright cabinet.

## Watching

Anyone can watch a cabinet's game. Each watcher's browser runs the game itself, a little behind
the players: the lowest seat playing sends them a state, then every player's input for each frame
once GGRS has confirmed it, so no rollback can change it. Inputs go out about ten times a second,
to everyone watching through one message to the room (address 0), and each watcher keeps a few
frames in hand so they play evenly. When the players change, the new session starts a new stream
with a fresh state. Watchers cost the players nothing: their game never pauses for one.

`emulator/netplay-check.mjs` plays a game between workers in Node over a simulated network:
player 1 alone, the others dropping in one by one, then player 2 leaving. Player 1 streams to a
watcher the whole time, and a machine in the script checks the stream against fresh states from
player 1 every 1.5 s (needs `make netplay`):

```sh
PLAYERS=4 node emulator/netplay-check.mjs emulator/dist/konami/fbneo.mjs $HOME/Downloads/ssriders.zip emulator/dist/ssriders.state
```

## Sizes

Players download each file once (compressed sizes; Workers static assets cap files at 25 MiB):

| File | Raw | gzip |
| --- | --- | --- |
| Bevy client (lean features, logs below `warn` compiled out, `wasm-opt -Oz`) | ~13.8 MiB | ~4.6 MB |
| One FBNeo core (neogeo, midway, snowbros, capcom, konami, classics) | ~5–6 MiB | ~3–3.3 MB |

A cabinet loads only its system's core. To add a system, add a line to `CORES` in
`emulator/build.sh` (driver files live under `src/burn/drv` in the FBNeo checkout).
