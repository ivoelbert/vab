// Plays a game online between emulator workers (web/emulator/worker.js, each in its own thread
// like separate browsers) over a simulated network, with random buttons for everyone. Player 1
// starts alone, the others drop in one by one, then player 2 leaves; each time player 1 hands
// the game as it is to everyone, as the page does. GGRS compares the machines every 60 frames:
// any "desync" means they drifted apart.
//
//   node emulator/netplay-check.mjs <core.mjs> <rom.zip> [state] [bios.zip ...]
//   PLAYERS=4 node emulator/netplay-check.mjs emulator/dist/konami/fbneo.mjs ~/Downloads/ssriders.zip emulator/dist/ssriders.state
//
// Env: PLAYERS (2), SECONDS per stage (10), LATENCY one way in ms (40), JITTER ms (10), LOSS
// fraction (0.02), TURNS=1 for turn-based games, BREAK=1 hands the last player the start-up
// state instead of the game (must desync). Needs the netplay module built: make netplay.
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { MessageChannel, Worker, isMainThread, parentPort, workerData } from "node:worker_threads";

if (!isMainThread) {
  // Enough of a browser worker for worker.js: postMessage/onmessage, and fetch for file URLs.
  globalThis.postMessage = (message, transfer) => parentPort.postMessage(message, transfer);
  globalThis.onmessage = null;
  parentPort.on("message", (data) => globalThis.onmessage?.({ data }));
  globalThis.fetch = async (url) => {
    try {
      const bytes = await readFile(new URL(url));
      const type = String(url).endsWith(".wasm") ? "application/wasm" : "application/octet-stream";
      return new Response(bytes, { headers: { "content-type": type } });
    } catch {
      return new Response(null, { status: 404 });
    }
  };
  await import(workerData.worker);
} else {
  const [corePath, romPath, statePath, ...biosPaths] = process.argv.slice(2);
  const PLAYERS = Number(process.env.PLAYERS ?? 2);
  const SECONDS = Number(process.env.SECONDS ?? 10);
  const LATENCY = Number(process.env.LATENCY ?? 40);
  const JITTER = Number(process.env.JITTER ?? 10);
  const LOSS = Number(process.env.LOSS ?? 0.02);
  const url = (path) => pathToFileURL(resolve(path)).href;
  const wait = (seconds) => new Promise((done) => setTimeout(done, seconds * 1000));

  const players = [];
  const seated = () => players.filter((player) => !player.left);

  // One worker per seat, as the page starts it.
  function sit(seat, hold) {
    const worker = new Worker(new URL(import.meta.url), {
      workerData: { worker: new URL("../web/emulator/worker.js", import.meta.url).href },
    });
    const { port1: inside, port2: outside } = new MessageChannel();
    const player = { seat, worker, outside, frames: 0, events: {}, captures: new Map(), errors: [] };
    worker.on("message", (message) => {
      if (message.type === "frame") player.frames++;
      if (message.type === "captured") player.captures.set(message.epoch, message.state);
      if (message.type === "netplay" && message.event === "stats") player.stats = message;
      else if (message.type === "netplay") player.events[message.event] = (player.events[message.event] ?? 0) + 1;
    });
    worker.on("error", (error) => player.errors.push(error.message));
    // The network: each packet arrives LATENCY ± JITTER ms later, or not at all.
    outside.on("message", ([to, packet]) => {
      const other = players[to];
      if (!other || other.left || Math.random() < LOSS) return;
      const delay = Math.max(0, LATENCY + (Math.random() * 2 - 1) * JITTER);
      setTimeout(() => other.left || other.outside.postMessage([seat, packet], [packet]), delay);
    });
    worker.postMessage(
      {
        type: "start",
        core: url(corePath),
        rom: url(romPath),
        files: biosPaths.map(url),
        state: statePath && url(statePath),
        seat,
        turns: Boolean(process.env.TURNS),
        port: inside,
        hold,
      },
      [inside],
    );
    players[seat] = player;
    return player;
  }

  // What the page does when the players change: the lowest seat playing captures its machine
  // and everyone starts a new session from that capture.
  let epoch = 0;
  async function handOver(breakLast) {
    const [coordinator, ...others] = seated();
    if (!others.length) return coordinator.worker.postMessage({ type: "solo" });
    epoch++;
    coordinator.worker.postMessage({ type: "capture", epoch });
    while (!coordinator.captures.has(epoch)) await wait(0.05);
    const seats = seated().map((player) => player.seat);
    const state = coordinator.captures.get(epoch);
    coordinator.worker.postMessage({ type: "online", epoch, seats });
    for (const player of others) {
      const given = breakLast && player === others.at(-1) ? await readFile(statePath) : state;
      player.worker.postMessage({ type: "online", epoch, seats, state: given });
    }
  }

  // Button mashing: a random mask held for 2-20 frames, per player.
  const USABLE = 0b0000_1111_1111_1011; // B Y START UP DOWN LEFT RIGHT A X L R
  const mashing = setInterval(() => {
    for (const player of seated()) {
      if (--player.hold > 0) continue;
      player.hold = 2 + Math.floor(Math.random() * 18);
      player.worker.postMessage({ type: "input", mask: Math.floor(Math.random() * 0x10000) & USABLE });
    }
  }, 1000 / 60);

  const report = (stage) => {
    for (const player of players) {
      const { ping, delay, rollback } = player.stats ?? {};
      console.log(stage, JSON.stringify({ seat: player.seat, left: player.left, frames: player.frames, ping, delay, rollback, ...player.events, errors: player.errors }));
    }
  };
  sit(0, false);
  await wait(SECONDS / 2);
  for (let seat = 1; seat < PLAYERS; seat++) {
    sit(seat, true);
    await handOver(process.env.BREAK && seat === PLAYERS - 1);
    await wait(SECONDS);
    report(`${seat + 1} players:`);
  }
  const leaving = players[1];
  leaving.left = true;
  await leaving.worker.terminate();
  await handOver();
  const before = seated().map((player) => player.frames);
  await wait(SECONDS);
  report("player 2 left:");
  const stuck = seated().some((player, i) => player.frames - before[i] < SECONDS * 30);
  if (stuck) players[0].errors.push("stopped after player 2 left");
  clearInterval(mashing);
  for (const player of seated()) await player.worker.terminate();
  const desynced = players.some((player) => player.events.desync);
  const broken = players.some((player) => player.errors.length || !player.frames);
  process.exitCode = !broken && desynced === Boolean(process.env.BREAK) ? 0 : 1;
  console.log(process.exitCode ? "FAIL" : "ok");
}
