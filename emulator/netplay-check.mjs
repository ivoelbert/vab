// Plays a game online between two emulator workers (web/emulator/worker.js, each in its own
// thread like two browsers) over a simulated network, with random buttons on both sides.
// GGRS compares the two machines every 60 frames: any "desync" means they drifted apart.
//
//   node emulator/netplay-check.mjs <core.mjs> <rom.zip> [state] [bios.zip ...]
//   node emulator/netplay-check.mjs emulator/dist/konami/fbneo.mjs ~/Downloads/ssriders.zip emulator/dist/ssriders.state
//
// Env: SECONDS (30), LATENCY one way in ms (40), JITTER ms (10), LOSS fraction (0.02),
// TURNS=1 for turn-based games, BREAK=1 starts player 2 without the state (must desync).
// Needs the netplay module built: make netplay.
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
  const SECONDS = Number(process.env.SECONDS ?? 30);
  const LATENCY = Number(process.env.LATENCY ?? 40);
  const JITTER = Number(process.env.JITTER ?? 10);
  const LOSS = Number(process.env.LOSS ?? 0.02);
  const url = (path) => pathToFileURL(resolve(path)).href;

  const players = [0, 1].map((seat) => {
    const worker = new Worker(new URL(import.meta.url), {
      workerData: { worker: new URL("../web/emulator/worker.js", import.meta.url).href },
    });
    const { port1: inside, port2: outside } = new MessageChannel();
    const player = { seat, worker, outside, frames: 0, events: {}, stats: undefined, errors: [] };
    worker.on("message", (message) => {
      if (message.type === "frame") player.frames++;
      if (message.type === "netplay" && message.event === "stats") player.stats = message;
      else if (message.type === "netplay") player.events[message.event] = (player.events[message.event] ?? 0) + 1;
    });
    worker.on("error", (error) => player.errors.push(error.message));
    const breakIt = process.env.BREAK && seat === 1;
    worker.postMessage(
      {
        type: "start",
        core: url(corePath),
        rom: url(romPath),
        files: biosPaths.map(url),
        state: statePath && !breakIt ? url(statePath) : undefined,
        seat,
        turns: Boolean(process.env.TURNS),
        netplay: { port: inside },
      },
      [inside],
    );
    return player;
  });

  // The network: each packet arrives LATENCY ± JITTER ms later, or not at all.
  for (const [from, to] of [players, [...players].reverse()]) {
    from.outside.on("message", (packet) => {
      if (Math.random() < LOSS) return;
      const delay = LATENCY + (Math.random() * 2 - 1) * JITTER;
      setTimeout(() => to.outside.postMessage(packet, [packet]), Math.max(0, delay));
    });
  }

  // Button mashing: a random mask held for 2-20 frames, per player.
  const USABLE = 0b0000_1111_1111_1011; // B Y START UP DOWN LEFT RIGHT A X L R
  const mashing = setInterval(() => {
    for (const player of players) {
      if (--player.hold > 0) continue;
      player.hold = 2 + Math.floor(Math.random() * 18);
      player.worker.postMessage({ type: "input", mask: Math.floor(Math.random() * 0x10000) & USABLE });
    }
  }, 1000 / 60);

  await new Promise((done) => setTimeout(done, SECONDS * 1000));
  clearInterval(mashing);
  for (const player of players) {
    const { ping, delay, rollback } = player.stats ?? {};
    console.log(
      JSON.stringify({ seat: player.seat, framesShown: player.frames, ping, delay, rollback, ...player.events, errors: player.errors }),
    );
    await player.worker.terminate();
  }
  const desynced = players.some((player) => player.events.desync);
  const broken = players.some((player) => player.errors.length || !player.frames);
  process.exitCode = !broken && desynced === Boolean(process.env.BREAK) ? 0 : 1;
}
