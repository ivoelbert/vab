// Checks that a game can run with rollback netplay: that re-running frames after a rollback
// gives the same game as playing straight through, on a separate core instance, and how long
// the worst case (a rollback on every frame) takes against the frame budget.
//
//   node emulator/rollback-check.mjs <core.mjs> <rom.zip> [state] [bios.zip ...]
//   node emulator/rollback-check.mjs emulator/dist/midway/fbneo.mjs ~/Downloads/mk2.zip emulator/dist/mk2.state
//
// FRAMES (default 1200) and ROLLBACK (default 8) env vars change the run. Rerun after
// rebuilding the cores: determinism is a property of the core build.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { Core } from "../web/emulator/libretro.js";

const [corePath, romPath, statePath, ...biosPaths] = process.argv.slice(2);
const FRAMES = Number(process.env.FRAMES ?? 1200);
const ROLLBACK = Number(process.env.ROLLBACK ?? 8); // frames re-run on every frame
const CHECK_EVERY = 60; // frames between comparisons of the game's RAM

const { default: createFBNeo } = await import(resolve(corePath));

// Each call is a separate wasm instance, like the other player's browser.
async function boot() {
  const core = await Core.create(createFBNeo, { onFrame() {}, onAudio() {} });
  core.netplay = true;
  for (const path of biosPaths) core.addFile(basename(path), readFileSync(path));
  const { fps } = core.loadGame(basename(romPath), readFileSync(romPath));
  if (statePath) core.unserialize(readFileSync(statePath));
  core.allocSlots(ROLLBACK + 1);
  return { core, fps };
}

// Human-ish input: random buttons and directions (diagonals and opposites too) held for
// 2-20 frames. Leaves out SELECT (coin) and L3/R3.
function inputs(seed) {
  let s = seed;
  const random = () => {
    s = (s + 0x6d2b79f5) | 0;
    let t = Math.imul(s ^ (s >>> 15), 1 | s);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  const USABLE = 0b0000_1111_1111_1011; // B Y START UP DOWN LEFT RIGHT A X L R
  const out = [];
  let mask = 0;
  let hold = 0;
  for (let f = 0; f < FRAMES; f++) {
    if (hold-- <= 0) {
      mask = Math.floor(random() * 0x10000) & USABLE;
      hold = 2 + Math.floor(random() * 18);
    }
    out.push(mask);
  }
  return out;
}
const p1 = inputs(1);
const p2 = inputs(2);
const hash = (bytes) => createHash("sha1").update(bytes).digest("hex");

// 1. Straight through, as if player 2's input always arrived on time.
const straight = await boot();
const ramAfter = new Map(); // frame -> RAM hash after it ran
let t0 = performance.now();
let hashMs = 0;
for (let f = 0; f < FRAMES; f++) {
  straight.core.inputs[0] = p1[f];
  straight.core.inputs[1] = p2[f];
  straight.core.run();
  if (f % CHECK_EVERY === CHECK_EVERY - 1) {
    const t = performance.now();
    ramAfter.set(f, hash(straight.core.systemRam()));
    hashMs += performance.now() - t;
  }
}
const runMs = (performance.now() - t0 - hashMs) / FRAMES;

// 2. A second instance where player 2's input always arrives ROLLBACK frames late: each frame
// runs on a guess (their last known input), then rolls back and re-runs with the real one.
const { core } = await boot();
const late = (f, now) => (f <= now - ROLLBACK ? p2[f] : p2[Math.max(0, now - ROLLBACK)]);
const slot = (f) => f % (ROLLBACK + 1);
const frameMs = [];
let resimMs = 0;
let resims = 0;
let diverged;
const compare = (f) => {
  if (diverged === undefined && ramAfter.has(f) && ramAfter.get(f) !== hash(core.systemRam())) {
    diverged = f;
  }
};
for (let now = 0; now < FRAMES; now++) {
  const start = performance.now();
  let checkMs = 0;
  // Player 2's input for frame `now - ROLLBACK` just arrived: go back and redo from there.
  if (now > 0) {
    const from = Math.max(0, now - ROLLBACK);
    core.loadSlot(slot(from));
    core.present = false;
    for (let f = from; f < now; f++) {
      if (f > from) core.saveSlot(slot(f));
      core.inputs[0] = p1[f];
      core.inputs[1] = late(f, now);
      const t = performance.now();
      core.run();
      resimMs += performance.now() - t;
      resims++;
      if (f === from && now >= ROLLBACK) {
        // Frame `from` now ran with both players' real input: it must match the straight run.
        const c = performance.now();
        compare(f);
        checkMs += performance.now() - c;
      }
    }
  }
  core.saveSlot(slot(now));
  core.present = true;
  core.inputs[0] = p1[now];
  core.inputs[1] = late(now, now);
  core.run();
  frameMs.push(performance.now() - start - checkMs);
}
// The last inputs arrive: redo the unconfirmed tail.
core.loadSlot(slot(FRAMES - ROLLBACK));
for (let f = FRAMES - ROLLBACK; f < FRAMES; f++) {
  core.inputs[0] = p1[f];
  core.inputs[1] = p2[f];
  core.run();
  compare(f);
}

const sorted = [...frameMs].sort((a, b) => a - b);
const percentile = (p) => sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))];
const round = (value, digits = 1) => Number(value.toFixed(digits));
const report = {
  game: basename(romPath, ".zip"),
  budgetMs: round(1000 / straight.fps), // one frame at the game's rate
  runMs: round(runMs, 2), // one frame, drawn and with sound
  resimMs: round(resimMs / resims, 2), // one re-run frame, no drawing or sound output
  [`rollback${ROLLBACK}Ms`]: round(percentile(50)), // a whole frame with a ROLLBACK-frame rollback
  [`rollback${ROLLBACK}P99Ms`]: round(percentile(99)),
  stateKiB: Math.round(core.slotBytes(0).length / 1024),
  ramKiB: Math.round(core.systemRam().length / 1024),
  inSync: diverged === undefined,
};
if (diverged !== undefined) report.divergedAfterFrame = diverged;
console.log(JSON.stringify(report));
if (!report.inSync) process.exitCode = 1;
