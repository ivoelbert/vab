// Makes a start-up save state for a game: boots it, presses a button to get past any
// "settings restored" screen, inserts coins and saves the machine. The page loads this state
// right after the game, so it starts ready to play with credits.
//
//   node emulator/snapshot.mjs <core.mjs> <rom.zip> <out.state> [coins] [bios.zip ...]
//   node emulator/snapshot.mjs emulator/dist/midway/fbneo.mjs ~/Downloads/mk2.zip emulator/dist/mk2.state 9
//   node emulator/snapshot.mjs emulator/dist/neogeo/fbneo.mjs ~/Downloads/mslug.zip emulator/dist/mslug.state 9 ~/Downloads/neogeo.zip
//
// States only load into the same core build: rerun this after rebuilding the cores.
import { readFileSync, writeFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { Core } from "../web/emulator/libretro.js";

const [corePath, romPath, outPath, coins = "9", ...biosPaths] = process.argv.slice(2);
const { default: createFBNeo } = await import(resolve(corePath));
const core = await Core.create(createFBNeo, { onFrame() {}, onAudio() {} });
for (const path of biosPaths) core.addFile(basename(path), readFileSync(path));
const { fps } = core.loadGame(basename(romPath), readFileSync(romPath));

// RetroPad bits (libretro.h): B is any button, SELECT inserts a coin.
const B = 1 << 0;
const SELECT = 1 << 2;
const run = (seconds, mask = 0) => {
  core.inputs[0] = mask;
  for (let i = 0; i < Math.round(seconds * fps); i++) core.run();
};

run(10); // boot
run(0.2, B); // "any button to continue"
run(5);
for (let i = 0; i < Number(coins); i++) {
  run(0.2, SELECT);
  run(0.3);
}
run(2);
writeFileSync(outPath, core.serialize());
console.log(`Saved ${outPath}`);
